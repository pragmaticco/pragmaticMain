# Pragmatic

**Durable execution for agents that don't run deterministically.**

A Rust runtime that journals every step an agent takes, so it survives any
crash and replays exactly — nondeterministic model behavior included.

Durable execution made software reliable. But its replay model assumes
determinism, and LLM agents are not deterministic. Everyone else caches
around that. We modeled it: each LLM call is a first-class probability
distribution (the **Oracle**), every realized outcome lands in an append-only
log (the **Journal**), and replay soundness (T1) is proved in the
[Agentical calculus](https://aniketh.net/pragmatic#research) — mechanized in
Lean 4, not assumed.

## Three guarantees, every run

1. **Journal every step.** Each LLM call's realized outcome is recorded to an
   append-only log as the run proceeds. The journal is the source of truth
   for what the agent actually did.
2. **Recover from any crash.** Resume exactly where the agent stopped.
   Recorded outcomes are read back instead of re-sampled — no duplicate model
   calls, no divergent paths, no lost work.
3. **Replay faithfully.** Reproduce the entire run exactly for debugging and
   audit, nondeterministic LLM behavior included (T1, replay soundness).

## In your code

```rust
use pragmatic::{Ctx, Fault, Runtime, SeededOracle, Value};

// Wrap the run. Every oracle call is journaled once.
fn research(ctx: &mut Ctx) -> Result<Value, Fault> {
    let plan = ctx.oracle("plan the task")?;                 // journaled
    let mut out = Vec::new();
    for step in 0..12 {
        out.push(ctx.oracle(format!("probe {step}: {plan}"))?); // journaled
    }
    ctx.effect("publish_report", format!("{} findings", out.len()), |arg| {
        Ok(Value::from(format!("s3://reports/{arg}")))       // write-ahead journaled
    })
}

let mut rt = Runtime::on_dir("./journals", MyModelOracle::new())?;

// Record. Crash anywhere — say, step nine of a thousand.
let report = rt.run("research-42", research)?;

// Resume on step nine, reading every prior call back from the Journal
// instead of paying for it again.
let resumed = rt.resume("research-42", research)?;

// Replay the whole run, bit for bit, for audit. The model is never called.
let audit = rt.replay("research-42", research)?;   // T1
assert_eq!(audit.trace, resumed.trace);
```

Implement [`Oracle`](src/oracle.rs) for your model client — Anthropic,
OpenAI, a local server, any step whose result is a draw from a distribution.
The runtime is a **zero-dependency** library (std only) that links into the
agent you already run: Linux, containers, any cloud. Nothing to provision, no
operating system to adopt.

Try it: `cargo run --example research_agent`

## Architecture

| Component | What it is |
|---|---|
| **Oracle** ([`src/oracle.rs`](src/oracle.rs)) | The LLM call as a probability distribution the runtime understands — the sole source of probabilistic nondeterminism. |
| **Journal** ([`src/journal.rs`](src/journal.rs)) | Append-only, cursor-keyed, SHA-256/HMAC hash-chained event history. O(1) append, torn-tail crash recovery, tamper-evident audit trail. |
| **Ctx** ([`src/ctx.rs`](src/ctx.rs)) | The durable execution context: one code path, record and replay semantics (`[O-rec]`/`[O-rep]`/`[O-resume]`). |
| **Effects** | Three-phase write-ahead discipline: intent → perform → commit. Crashes inside the window leave a *dangling intent* that recovery must resolve — the no-orphaned-effect property. |
| **Supervisor** ([`src/supervisor.rs`](src/supervisor.rs)) | Fault → decision control plane: restart-from-journal, reverse-order saga compensation, escalation. |
| **Capabilities** | Per-run effect allowlists and draw budgets (`BudgetExhausted` turns runaway loops into well-typed faults). |
| **Memory** ([`src/memory.rs`](src/memory.rs)) | IFC-labeled store (`Low ⊑ High`): no read-up, no write-down — the runtime discipline behind noninterference (T2). |

## Measured (this repo, `cargo test --release`)

| Claim | Result |
|---|---|
| Replay determinism (E1) | **1000 / 1000** seeded stochastic runs replay with byte-identical traces and zero model calls |
| Crash recovery (E2) | Crash at *every* journal prefix → resume is exact; no re-sampling of the surviving prefix; no orphaned effects; survives real process restarts via the file journal |
| Journal append (E3) | **O(1)**, flat from 1k to 1M entries (~0.8 µs/append including full SHA-256 chaining; ~230 ns for the unchained reference journal in the [agentical](https://github.com/vayusys) prototype) |
| Replay speedup (E3) | **~1000×** under a 1 ms/draw latency-injected oracle — replay performs zero oracle calls |
| Tamper evidence | Any post-hoc mutation of a committed entry breaks the hash chain at that cursor; keyed (HMAC) journals refuse to open under the wrong key |

## Honest scope

- **T1 (replay soundness)** is proved in the paper and mechanized in Lean 4
  (operational core, sorry-free). This runtime demonstrates it empirically
  across the reference suite.
- **T2 (probabilistic noninterference)** and **T3 (compositional bounds)**
  are pen-and-paper results; the `memory` module implements T2's runtime
  discipline but the theorems are not mechanized.
- The v0.1 API is synchronous and closure-based. The `#[pragmatic::durable]`
  attribute macro and async surface are roadmap; they change ergonomics, not
  semantics.
- Assumption A2 (same code replayed) is enforced, not assumed: replay
  detects a changed program as a `JournalDesync` fault instead of silently
  misreplaying.

## Research

Pragmatic's guarantees are real because they are proved. The theory is
**Agentical**, a probabilistic process calculus for durable multi-agent LLM
systems: six primitives (Agent, Oracle, Channel, Memory, Journal,
Supervisor), a probabilistic small-step semantics in record and replay
modes, and three theorems with honest scope. See the
[research page](https://aniketh.net/pragmatic#research).

## License

MIT OR Apache-2.0.
