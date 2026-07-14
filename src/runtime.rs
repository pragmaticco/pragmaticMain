//! The **Runtime** — record, resume, replay.
//!
//! Wrap your agent in a closure over [`Ctx`], hand it to the runtime under a
//! stable run id, and every oracle call, effect, receive, and clock read is
//! journaled as the run proceeds:
//!
//! ```
//! use pragmatic::{Runtime, SeededOracle};
//!
//! let mut rt = Runtime::in_memory(SeededOracle::new(42));
//! let report = rt.run("run-1", |ctx| {
//!     let plan = ctx.oracle("plan the task")?;
//!     let step = ctx.oracle(format!("execute: {plan}"))?;
//!     Ok(format!("done: {step}").into())
//! }).unwrap();
//!
//! // Crash anywhere; `resume` replays the journaled prefix (no model calls)
//! // and continues recording at the tail.
//! // Replay the whole run later, exactly — the model is never consulted.
//! let audit = rt.replay("run-1", |ctx| {
//!     let plan = ctx.oracle("plan the task")?;
//!     let step = ctx.oracle(format!("execute: {plan}"))?;
//!     Ok(format!("done: {step}").into())
//! }).unwrap();
//! assert_eq!(report.trace, audit.trace); // T1, empirically
//! ```

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use crate::ctx::{Ctx, RunOptions, TraceLabel};
use crate::fault::Fault;
use crate::journal::{Cursor, Journal};
use crate::oracle::{Oracle, RefusingOracle};
use crate::sha256::Digest;
use crate::value::Value;

/// How a dangling effect intent (a crash inside the write-ahead window) is
/// resolved on resume.
pub enum Recover {
    /// The effect is idempotent or known to have completed: close the intent
    /// with this committed result. Replay will reuse it without touching the
    /// world again.
    Commit(Value),
    /// Undo / write off the effect. The resumed run will perform it fresh.
    Compensate,
}

/// The result of driving one run to completion.
#[derive(Clone, Debug)]
pub struct RunReport {
    pub run_id: String,
    /// The agent's output.
    pub output: Value,
    /// The observable trace — identical between a recorded run and its
    /// replay (T1).
    pub trace: Vec<TraceLabel>,
    /// Journal length after this attempt.
    pub journal_len: u64,
    /// Head of the tamper-evident hash chain (the run's integrity
    /// commitment).
    pub chain_head: Option<Digest>,
    /// Steps served from the journal (zero model calls).
    pub replayed_steps: u64,
    /// Steps recorded fresh this attempt.
    pub fresh_steps: u64,
}

enum Storage {
    Memory,
    Dir(PathBuf),
}

/// The Pragmatic runtime: owns the oracle (your model client) and the
/// journals for every run it drives.
pub struct Runtime<O> {
    oracle: O,
    storage: Storage,
    key: Option<Vec<u8>>,
    /// Loaded journals by run id. In `Dir` mode this is a write-through
    /// cache over the journal files.
    journals: HashMap<String, Journal>,
    /// Per-run channel inboxes (single-process delivery; receives are
    /// journaled, so replay does not need the inbox).
    inboxes: HashMap<String, HashMap<String, VecDeque<Value>>>,
}

impl<O: Oracle> Runtime<O> {
    /// A runtime whose journals live in memory only (tests, ephemeral runs).
    pub fn in_memory(oracle: O) -> Self {
        Runtime {
            oracle,
            storage: Storage::Memory,
            key: None,
            journals: HashMap::new(),
            inboxes: HashMap::new(),
        }
    }

    /// A runtime that persists one append-only journal file per run under
    /// `dir`. Runs survive process crashes: reopen the runtime on the same
    /// directory and `resume`.
    pub fn on_dir(dir: impl Into<PathBuf>, oracle: O) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Runtime {
            oracle,
            storage: Storage::Dir(dir),
            key: None,
            journals: HashMap::new(),
            inboxes: HashMap::new(),
        })
    }

    /// Key the journals' hash chains with HMAC-SHA-256 under `key`, making
    /// the audit trail attributable to this runtime, not just tamper-evident.
    pub fn with_key(mut self, key: &[u8]) -> Self {
        self.key = Some(key.to_vec());
        self
    }

    fn path_for(dir: &std::path::Path, run_id: &str) -> PathBuf {
        // Run ids become filenames; keep them filesystem-safe.
        let safe: String = run_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        dir.join(format!("{safe}.journal"))
    }

    /// Ensure the journal for `run_id` is loaded (creating it if `create`).
    fn load(&mut self, run_id: &str, create: bool) -> Result<(), Fault> {
        if self.journals.contains_key(run_id) {
            return Ok(());
        }
        let journal = match &self.storage {
            Storage::Memory => {
                if !create {
                    return Err(Fault::Io(format!("no journal for run '{run_id}'")));
                }
                match &self.key {
                    Some(k) => Journal::in_memory_keyed(k),
                    None => Journal::in_memory(),
                }
            }
            Storage::Dir(dir) => {
                let path = Self::path_for(dir, run_id);
                if path.exists() {
                    let loaded = match &self.key {
                        Some(k) => Journal::open_keyed(&path, k)?,
                        None => Journal::open(&path)?,
                    };
                    loaded.journal
                } else if create {
                    match &self.key {
                        Some(k) => Journal::create_keyed(&path, k)?,
                        None => Journal::create(&path)?,
                    }
                } else {
                    return Err(Fault::Io(format!("no journal for run '{run_id}'")));
                }
            }
        };
        self.journals.insert(run_id.to_string(), journal);
        Ok(())
    }

    fn execute(
        &mut self,
        run_id: &str,
        opts: &RunOptions,
        strict: bool,
        agent: impl FnOnce(&mut Ctx) -> Result<Value, Fault>,
    ) -> Result<RunReport, Fault> {
        let journal = self
            .journals
            .get_mut(run_id)
            .expect("journal loaded by caller");
        let inbox = self.inboxes.entry(run_id.to_string()).or_default();

        let refusing = RefusingOracle;
        let oracle: &dyn Oracle = if strict { &refusing } else { &self.oracle };

        let mut ctx = Ctx::new(journal, oracle, inbox, strict, opts);
        let result = agent(&mut ctx);
        let (trace, replayed, fresh) = ctx.finish(result.as_ref().ok());

        // The attempt is over; make everything recorded so far durable.
        journal.sync()?;

        let output = result?;
        Ok(RunReport {
            run_id: run_id.to_string(),
            output,
            trace,
            journal_len: journal.len(),
            chain_head: journal.head(),
            replayed_steps: replayed,
            fresh_steps: fresh,
        })
    }

    /// Start (or continue) a durable run. If a journal for `run_id` already
    /// exists, its prefix is replayed first — `run` is safely re-enterable,
    /// which is what makes retries idempotent.
    pub fn run(
        &mut self,
        run_id: &str,
        agent: impl FnOnce(&mut Ctx) -> Result<Value, Fault>,
    ) -> Result<RunReport, Fault> {
        self.run_with(run_id, RunOptions::default(), agent)
    }

    /// [`run`](Self::run) with explicit budget / capability options.
    pub fn run_with(
        &mut self,
        run_id: &str,
        opts: RunOptions,
        agent: impl FnOnce(&mut Ctx) -> Result<Value, Fault>,
    ) -> Result<RunReport, Fault> {
        self.load(run_id, true)?;
        self.execute(run_id, &opts, false, agent)
    }

    /// Resume a crashed run: replay the journaled prefix (zero model calls,
    /// no duplicate effects), then continue recording where it stopped.
    /// Errors if no journal exists for `run_id`.
    pub fn resume(
        &mut self,
        run_id: &str,
        agent: impl FnOnce(&mut Ctx) -> Result<Value, Fault>,
    ) -> Result<RunReport, Fault> {
        self.resume_with(
            run_id,
            RunOptions::default(),
            |_, _| Recover::Compensate,
            agent,
        )
    }

    /// [`resume`](Self::resume) with options and an explicit recovery policy
    /// for dangling effect intents ([Eff-recover]): for each effect that
    /// crashed inside its write-ahead window, decide whether it completed
    /// (`Recover::Commit(result)` — e.g. after re-running an idempotent
    /// effect or checking the world) or must be written off
    /// (`Recover::Compensate`, the default — the resumed run performs it
    /// fresh).
    pub fn resume_with(
        &mut self,
        run_id: &str,
        opts: RunOptions,
        mut recovery: impl FnMut(&str, &Value) -> Recover,
        agent: impl FnOnce(&mut Ctx) -> Result<Value, Fault>,
    ) -> Result<RunReport, Fault> {
        self.load(run_id, false)?;
        // No-orphaned-effect: close every dangling intent before re-entry.
        let journal = self.journals.get_mut(run_id).expect("loaded above");
        let dangling = journal.dangling_intents();
        for (cursor, name) in dangling {
            let arg = match &journal.get(cursor).expect("cursor valid").event {
                crate::journal::Event::EffectIntent { arg, .. } => arg.clone(),
                _ => unreachable!("dangling_intents returns intent cursors"),
            };
            match recovery(&name, &arg) {
                Recover::Commit(result) => {
                    journal.append(crate::journal::Event::EffectCommit { name, result })?;
                }
                Recover::Compensate => {
                    journal.append(crate::journal::Event::EffectCompensated { name })?;
                }
            }
        }
        journal.sync()?;
        self.execute(run_id, &opts, false, agent)
    }

    /// Replay a recorded run, exactly, for debugging and audit — T1. Every
    /// outcome is read from the journal; the model is **never** called (the
    /// oracle is replaced by one that refuses). Errors with
    /// [`Fault::ReplayExhausted`] if the recorded run never completed.
    pub fn replay(
        &mut self,
        run_id: &str,
        agent: impl FnOnce(&mut Ctx) -> Result<Value, Fault>,
    ) -> Result<RunReport, Fault> {
        self.load(run_id, false)?;
        self.execute(run_id, &RunOptions::default(), true, agent)
    }

    /// Deliver a value to a run's channel inbox (single-process delivery;
    /// the receive itself is journaled when the agent consumes it).
    pub fn send(&mut self, run_id: &str, channel: &str, value: impl Into<Value>) {
        self.inboxes
            .entry(run_id.to_string())
            .or_default()
            .entry(channel.to_string())
            .or_default()
            .push_back(value.into());
    }

    /// Verify the run's hash chain end to end. `Err(cursor)` names the first
    /// tampered entry.
    pub fn verify(&mut self, run_id: &str) -> Result<Result<(), Cursor>, Fault> {
        self.load(run_id, false)?;
        Ok(self.journals[run_id].verify())
    }

    /// Direct access to a loaded journal (inspection, tests).
    pub fn journal(&mut self, run_id: &str) -> Result<&mut Journal, Fault> {
        self.load(run_id, false)?;
        Ok(self.journals.get_mut(run_id).expect("loaded above"))
    }

    /// Drop the in-memory cache for `run_id` (Dir mode: forces a reload from
    /// disk on next use — how tests model a process restart).
    pub fn evict(&mut self, run_id: &str) {
        self.journals.remove(run_id);
    }

    /// The oracle this runtime records with.
    pub fn oracle(&self) -> &O {
        &self.oracle
    }
}
