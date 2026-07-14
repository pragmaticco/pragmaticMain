//! `Ctx` — the durable execution context handed to your agent.
//!
//! One code path, two semantics. Your agent calls `ctx.oracle(...)`,
//! `ctx.effect(...)`, `ctx.recv(...)`, `ctx.now()`; the context decides,
//! per step, whether the outcome is *replayed* from the journal ([O-rep],
//! [Eff-rep], [Rcv-rep]) or *recorded* fresh ([O-rec], [Eff-intent/commit],
//! [Rcv-rec]). A resumed run replays its surviving prefix and falls through
//! to recording at the tail ([O-resume]) — the agent cannot tell the
//! difference, which is exactly Theorem T1's content.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::fault::Fault;
use crate::journal::{Cursor, Event, Journal};
use crate::oracle::Oracle;
use crate::value::Value;

/// One observable step of a run. The trace is the object T1 speaks about:
/// record and replay produce identical label sequences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceLabel {
    /// An oracle draw at `cursor` realized `outcome`.
    Oracle { cursor: Cursor, outcome: Value },
    /// A durable effect committed at `cursor` with `result`.
    Effect {
        cursor: Cursor,
        name: String,
        result: Value,
    },
    /// A channel receive at `cursor`.
    Recv {
        cursor: Cursor,
        channel: String,
        value: Value,
    },
    /// A journaled clock read.
    Clock { cursor: Cursor, nanos: u64 },
    /// The run halted with `output`.
    Done { output: Value },
}

/// Per-run execution limits and grants (capabilities).
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    /// Maximum number of oracle draws before `BudgetExhausted` fires. `None`
    /// = unbounded. (Paper Remark 3.14: converts a runaway loop into a
    /// well-typed fault.)
    pub budget: Option<u64>,
    /// Effect names this run holds capabilities for. `None` = all effects
    /// allowed; `Some` = allowlist, anything else is `CapabilityDenied`.
    pub caps: Option<HashSet<String>>,
}

impl RunOptions {
    pub fn with_budget(mut self, draws: u64) -> Self {
        self.budget = Some(draws);
        self
    }

    pub fn with_caps<I: IntoIterator<Item = S>, S: Into<String>>(mut self, caps: I) -> Self {
        self.caps = Some(caps.into_iter().map(Into::into).collect());
        self
    }
}

fn preview(v: &Value) -> String {
    let s = v.as_str();
    if s.len() > 48 {
        format!("{}…", &s[..48])
    } else {
        s.into_owned()
    }
}

/// The durable execution context. Borrow-scoped to one attempt of one run.
pub struct Ctx<'a> {
    journal: &'a mut Journal,
    oracle: &'a dyn Oracle,
    /// Strict replay: never fall through to recording; the model is never
    /// called ([O-resume] disabled).
    strict: bool,
    /// Replay read cursor: entries below `pos` have been consumed.
    pos: u64,
    trace: Vec<TraceLabel>,
    budget: Option<u64>,
    caps: Option<HashSet<String>>,
    inbox: &'a mut HashMap<String, VecDeque<Value>>,
    replayed: u64,
    fresh: u64,
    /// Injectable time source (journaled, so only consulted when recording).
    clock: fn() -> u64,
}

fn system_nanos() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(
        journal: &'a mut Journal,
        oracle: &'a dyn Oracle,
        inbox: &'a mut HashMap<String, VecDeque<Value>>,
        strict: bool,
        opts: &RunOptions,
    ) -> Self {
        Ctx {
            journal,
            oracle,
            strict,
            pos: 0,
            trace: Vec::new(),
            budget: opts.budget,
            caps: opts.caps.clone(),
            inbox,
            replayed: 0,
            fresh: 0,
            clock: system_nanos,
        }
    }

    /// Next journaled entry to replay, if the read cursor is inside the
    /// recorded prefix.
    fn replay_next(&mut self) -> Option<(Cursor, Event)> {
        if self.pos < self.journal.len() {
            let e = self.journal.get(self.pos).expect("pos < len");
            let out = (e.cursor, e.event.clone());
            self.pos += 1;
            self.replayed += 1;
            Some(out)
        } else {
            None
        }
    }

    fn charge_budget(&mut self) -> Result<(), Fault> {
        if let Some(b) = self.budget.as_mut() {
            if *b == 0 {
                return Err(Fault::BudgetExhausted);
            }
            *b -= 1;
        }
        Ok(())
    }

    /// `let x ⇐ oracle(e)` — one model call, journaled once.
    ///
    /// Record: sample from the oracle, append `OracleDraw` ([O-rec]).
    /// Replay: read the recorded outcome back; the model is not called
    /// ([O-rep]). A resumed run switches from the latter to the former
    /// exactly where the journal ends ([O-resume]).
    pub fn oracle(&mut self, prompt: impl Into<Value>) -> Result<Value, Fault> {
        let prompt = prompt.into();
        self.charge_budget()?;

        if let Some((cursor, event)) = self.replay_next() {
            return match event {
                Event::OracleDraw {
                    prompt: p, outcome, ..
                } if p == prompt => {
                    self.trace.push(TraceLabel::Oracle {
                        cursor,
                        outcome: outcome.clone(),
                    });
                    Ok(outcome)
                }
                other => Err(Fault::JournalDesync {
                    cursor,
                    expected: format!("OracleDraw({})", preview(&prompt)),
                    found: format!("{other:?}"),
                }),
            };
        }
        if self.strict {
            return Err(Fault::ReplayExhausted { cursor: self.pos });
        }

        // [O-rec]: the only probabilistic step. Sample, then journal the
        // realized outcome in the same atomic step it is consumed (I4).
        let outcome = self.oracle.call(&prompt)?;
        let cursor = self.journal.append(Event::OracleDraw {
            prompt,
            outcome: outcome.clone(),
            provenance: self.oracle.provenance(),
        })?;
        self.journal.sync()?;
        self.pos = self.journal.len();
        self.fresh += 1;
        self.trace.push(TraceLabel::Oracle {
            cursor,
            outcome: outcome.clone(),
        });
        Ok(outcome)
    }

    /// `do[f] e` — a durable effect under the three-phase write-ahead
    /// discipline (paper §4.5):
    ///
    /// 1. [Eff-intent]  journal `EffectIntent` *before* touching the world;
    /// 2. [Eff-perform] run `perform` (your tool call / external write);
    /// 3. [Eff-commit]  journal `EffectCommit` with the result.
    ///
    /// Replay reuses the committed result and does **not** re-perform
    /// ([Eff-rep]). A crash between 1 and 3 leaves a dangling intent that
    /// [`Runtime::resume_with`](crate::runtime::Runtime::resume_with)
    /// resolves before re-entering — the no-orphaned-effect property.
    pub fn effect(
        &mut self,
        name: &str,
        arg: impl Into<Value>,
        perform: impl FnOnce(&Value) -> Result<Value, Fault>,
    ) -> Result<Value, Fault> {
        let arg = arg.into();
        if let Some(caps) = &self.caps {
            if !caps.contains(name) {
                return Err(Fault::CapabilityDenied(name.to_string()));
            }
        }

        // Replay path: consume Intent, then dispatch on what closed it.
        if let Some((cursor, event)) = self.replay_next() {
            match event {
                Event::EffectIntent { name: n, arg: a } if n == name && a == arg => {
                    match self.replay_next() {
                        Some((c2, Event::EffectCommit { name: n2, result })) if n2 == name => {
                            // [Eff-rep]: committed effect replays by reusing
                            // its recorded result. The world is not touched.
                            self.trace.push(TraceLabel::Effect {
                                cursor: c2,
                                name: n2,
                                result: result.clone(),
                            });
                            return Ok(result);
                        }
                        Some((_, Event::EffectCompensated { name: n2 })) if n2 == name => {
                            // The intent was closed by a compensation: the
                            // effect was undone after a crash. Fall through
                            // and perform it fresh.
                        }
                        Some((c2, other)) => {
                            return Err(Fault::JournalDesync {
                                cursor: c2,
                                expected: format!("EffectCommit({name})"),
                                found: format!("{other:?}"),
                            });
                        }
                        None => {
                            // Dangling intent at the tail: recovery did not
                            // run. Refuse rather than guess whether the
                            // world saw the effect.
                            return Err(Fault::JournalDesync {
                                cursor,
                                expected: format!(
                                    "EffectCommit({name}) or EffectCompensated({name}) — \
                                     resume_with a recovery policy to close the dangling intent"
                                ),
                                found: "end of journal".to_string(),
                            });
                        }
                    }
                }
                other => {
                    return Err(Fault::JournalDesync {
                        cursor,
                        expected: format!("EffectIntent({name})"),
                        found: format!("{other:?}"),
                    });
                }
            }
        } else if self.strict {
            return Err(Fault::ReplayExhausted { cursor: self.pos });
        }

        // Fresh path — write-ahead: the intent is durable BEFORE the world
        // can change ([Eff-intent]).
        self.journal.append(Event::EffectIntent {
            name: name.to_string(),
            arg: arg.clone(),
        })?;
        self.journal.sync()?;

        // [Eff-perform]: the world acts. A failure here (or a crash) leaves
        // the dangling intent for recovery — the effect is never orphaned.
        let result = perform(&arg)?;

        // [Eff-commit].
        let cursor = self.journal.append(Event::EffectCommit {
            name: name.to_string(),
            result: result.clone(),
        })?;
        self.journal.sync()?;
        self.pos = self.journal.len();
        self.trace.push(TraceLabel::Effect {
            cursor,
            name: name.to_string(),
            result: result.clone(),
        });
        Ok(result)
    }

    /// `x ← c?` — receive on a channel. Receives are journaled because the
    /// value arrives from outside this replay scope (invariant I4); replay
    /// reads it back ([Rcv-rep]).
    pub fn recv(&mut self, channel: &str) -> Result<Value, Fault> {
        if let Some((cursor, event)) = self.replay_next() {
            return match event {
                Event::ChannelRecv { channel: c, value } if c == channel => {
                    self.trace.push(TraceLabel::Recv {
                        cursor,
                        channel: c,
                        value: value.clone(),
                    });
                    Ok(value)
                }
                other => Err(Fault::JournalDesync {
                    cursor,
                    expected: format!("ChannelRecv({channel})"),
                    found: format!("{other:?}"),
                }),
            };
        }
        if self.strict {
            return Err(Fault::ReplayExhausted { cursor: self.pos });
        }

        let value = self
            .inbox
            .get_mut(channel)
            .and_then(|q| q.pop_front())
            .ok_or(Fault::Timeout)?;
        let cursor = self.journal.append(Event::ChannelRecv {
            channel: channel.to_string(),
            value: value.clone(),
        })?;
        self.journal.sync()?;
        self.pos = self.journal.len();
        self.trace.push(TraceLabel::Recv {
            cursor,
            channel: channel.to_string(),
            value: value.clone(),
        });
        Ok(value)
    }

    /// A journaled clock read: time is externally determined, so it is
    /// recorded like any other draw and replays exactly.
    pub fn now(&mut self) -> Result<u64, Fault> {
        if let Some((cursor, event)) = self.replay_next() {
            return match event {
                Event::Clock { nanos } => {
                    self.trace.push(TraceLabel::Clock { cursor, nanos });
                    Ok(nanos)
                }
                other => Err(Fault::JournalDesync {
                    cursor,
                    expected: "Clock".to_string(),
                    found: format!("{other:?}"),
                }),
            };
        }
        if self.strict {
            return Err(Fault::ReplayExhausted { cursor: self.pos });
        }
        let nanos = (self.clock)();
        let cursor = self.journal.append(Event::Clock { nanos })?;
        self.journal.sync()?;
        self.pos = self.journal.len();
        self.trace.push(TraceLabel::Clock { cursor, nanos });
        Ok(nanos)
    }

    /// Assert an agent contract mid-run; a falsified contract is a
    /// well-typed `ContractViol` fault the Supervisor can dispatch on.
    pub fn contract(&self, holds: bool, msg: &str) -> Result<(), Fault> {
        if holds {
            Ok(())
        } else {
            Err(Fault::ContractViol(msg.to_string()))
        }
    }

    /// True while steps are being served from the journal (useful for
    /// logging; agent logic should never branch on this).
    pub fn is_replaying(&self) -> bool {
        self.pos < self.journal.len()
    }

    pub(crate) fn finish(mut self, output: Option<&Value>) -> (Vec<TraceLabel>, u64, u64) {
        if let Some(out) = output {
            self.trace.push(TraceLabel::Done {
                output: out.clone(),
            });
        }
        (self.trace, self.replayed, self.fresh)
    }
}
