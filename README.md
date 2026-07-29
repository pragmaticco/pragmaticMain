<div align="center">

# Pragmatic

**Durable execution for agents that don't run deterministically.**

A Rust runtime that journals every step an agent takes, so it survives any
crash and replays exactly — nondeterministic model behavior included.

[![CI](https://github.com/pragmaticco/pragmaticMain/actions/workflows/ci.yml/badge.svg)](https://github.com/pragmaticco/pragmaticMain/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
![Rust](https://img.shields.io/badge/rust-stable-orange.svg)
![status: public beta](https://img.shields.io/badge/status-public%20beta-brightgreen.svg)

[Usage guide](docs/usage.md) · [Quickstart](docs/quickstart.md) · [Concepts](docs/concepts.md) ·
[Research](https://aniketh.net/pragmatic#research) ·
[Early access / design partners](mailto:me@aniketh.net?subject=Pragmatic%20early%20access)

</div>

---

Durable execution made software reliable. But its replay model assumes
determinism, and LLM agents are not deterministic. Every existing engine
hacks around that by caching model outputs as opaque activities — the engine
never understands the call. **Pragmatic models it**: each LLM call is a
first-class probability distribution (the **Oracle**), every realized
outcome lands in an append-only, hash-chained log (the **Journal**), and
replay soundness (**T1**) is proved in the
[Agentical calculus](https://aniketh.net/pragmatic#research) — mechanized in
Lean 4, not assumed.

## Three guarantees, every run

1. **Journal every step.** Each model call's realized outcome is recorded to
   an append-only log as the run proceeds — the source of truth for what the
   agent actually did.
2. **Recover from any crash.** Resume exactly where the agent stopped;
   recorded outcomes are read back instead of re-sampled. No duplicate model
   calls, no divergent paths, no lost work.
3. **Replay faithfully.** Reproduce the entire run bit-for-bit for
   time-travel debugging and audit — with the model never consulted.

## Durable in one wrapper

```rust
use pragmatic::{Ctx, Fault, Runtime, Value};
use pragmatic_anthropic::AnthropicOracle;

// Wrap the run. Every oracle call is journaled once, and the program's
// identity is verified on every replay.
#[pragmatic::durable]
fn research(ctx: &mut Ctx) -> Result<Value, Fault> {
    let plan = ctx.oracle("plan the task")?;                    // journaled
    let mut out = Vec::new();
    for step in 0..1000 {
        out.push(ctx.oracle(format!("probe {step}: {plan}"))?); // journaled
    }
    ctx.effect("publish", format!("{} findings", out.len()), |arg| {
        Ok(Value::from(format!("s3://reports/{arg}")))          // write-ahead journaled
    })
}

let oracle = AnthropicOracle::from_env()?.model("claude-sonnet-5");
let mut rt = Runtime::on_dir("./journals", oracle)?;

let report  = rt.run("research-42", research)?;    // record
// Crash on step nine of a thousand?
let resumed = rt.resume("research-42", research)?; // resume ON step nine — no re-sampling
let audit   = rt.replay("research-42", research)?; // replay bit-for-bit — T1
assert_eq!(audit.trace, resumed.trace);
```

Try it without a model: `cargo run -p pragmatic --example research_agent`

**Async?** Same guarantees, `await`ed — executor-agnostic (tokio, smol, or
the built-in `block_on`), still zero dependencies:

```rust
#[pragmatic::durable]
async fn research(ctx: &mut AsyncCtx<'_, MyOracle>) -> Result<Value, Fault> {
    let plan = ctx.oracle("plan the task").await?;               // journaled
    ctx.effect("publish", plan, async |arg| { /* awaited tool call */
        Ok(Value::from(format!("s3://{arg}"))) }).await
}
let mut rt = AsyncRuntime::on_dir("./journals", oracle)?;
let report = rt.run("research-42", research).await?;
```

**Python?** The same runtime, importable:

```python
import pragmatic
rt = pragmatic.Runtime("./journals", my_model_fn)
report = rt.run("research-42", agent)      # record
report = rt.resume("research-42", agent)   # crash-recover, no re-sampling
audit  = rt.replay("research-42", agent)   # bit-for-bit, model never called
```

See [crates/pragmatic-python](crates/pragmatic-python) — journals are
byte-compatible across Rust and Python, and the CLI reads both.

## Measured, not promised

Every number below is a test in this repo (`cargo test --release`):

| Claim | Result |
|---|---|
| **Replay determinism** (E1) | 1000 / 1000 seeded stochastic runs replay byte-identically, zero model calls |
| **Crash recovery** (E2) | Crash at *every* journal prefix → exact resume; no re-sampling; no orphaned effects; survives process restarts |
| **Journal overhead** (E3) | O(1) append, flat from 1k to 1M entries (~0.8 µs incl. full SHA-256 chaining) |
| **Replay speedup** (E3) | ~1000× under a 1 ms/draw oracle — replay performs zero oracle calls |
| **Tamper evidence** | Any post-hoc edit breaks the chain at that cursor; HMAC-keyed journals refuse the wrong key |
| **Code-change safety** | A journal recorded under one agent version refuses to replay under another (`JournalDesync` at cursor 0) |

## The workspace

| Crate | What | Deps |
|---|---|---|
| [`pragmatic`](crates/pragmatic) | The runtime, sync **and async**: Journal, Oracle, Ctx, Runtime, AsyncRuntime, Supervisor, IFC memory, capabilities | **zero** |
| [`pragmatic-macros`](crates/pragmatic-macros) | `#[pragmatic::durable]` — program identity, journaled and verified (sync and async fns) | **zero** |
| [`pragmatic-anthropic`](crates/pragmatic-anthropic) | Claude as a journaled Oracle (Messages API), wire-tested over real sockets | `ureq`, `serde_json` |
| [`pragmatic-cli`](crates/pragmatic-cli) | `pragmatic runs / show / verify / export / serve` — HTML replay console, exportable or served live | **zero** |
| [`pragmatic-python`](crates/pragmatic-python) | The runtime from Python (`pip install pragmatic-runtime`); journals byte-compatible with Rust | `pyo3` |

Implement `Oracle` for any model client — Anthropic, OpenAI, a local server,
any step whose result is a draw from a distribution. The core links into the
agent you already run: Linux, containers, any cloud. Nothing to provision.

## The replay console

```sh
pragmatic serve --dir ./journals               # live console on 127.0.0.1:7171
pragmatic export research-42 --dir ./journals  # or a self-contained run.html
```

`serve` gives you a live, self-hostable console over your journal directory:
every run, its chain status, dangling-effect warnings, and a full timeline
per run — every prompt, outcome, effect intent/commit, and chain hash.
`export` produces the same timeline as one HTML file; attach it to the
incident ticket, it opens anywhere. Loopback-only by default: journals
contain prompts and outputs.

## A model, not a cache

| | Generic durable execution | Pragmatic |
|---|---|---|
| The model call | Opaque activity; cached output | A probability distribution the runtime understands |
| Replay soundness | Sound only if *you* keep surrounding code deterministic | Proved (T1), mechanized in Lean 4; determinism boundary is principled |
| Probabilistic reasoning | — | Compositional probability bounds (T3) |
| Information flow | — | Noninterference discipline at the runtime level (T2) |

## Honest scope

- **T1** is mechanized in Lean 4 (operational core, sorry-free) and
  demonstrated empirically here. **T2/T3** are pen-and-paper results; the
  runtime implements their disciplines, the theorems are not yet mechanized.
- Single-process journals on local disk. The managed cloud backend (hosted
  journaling at scale, shared console, alerting) is the commercial layer —
  it changes operations, not semantics.
- The Anthropic adapter is wire-tested against a Messages-API-shaped mock
  over real sockets on every CI run; the live-API test needs a key
  (`cargo test -p pragmatic-anthropic -- --ignored`).
- Journals store prompts/completions in plaintext by design (they *are* the
  audit trail). See [SECURITY.md](SECURITY.md).

## Research

Pragmatic's guarantees are real because they are proved. The theory is
**Agentical** — a probabilistic process calculus with six primitives (Agent,
Oracle, Channel, Memory, Journal, Supervisor), probabilistic small-step
semantics in record and replay modes, and three theorems stated with honest
scope. [Read the research](https://aniketh.net/pragmatic#research).

## License

MIT OR Apache-2.0, at your option.
