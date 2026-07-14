# Quickstart

Five minutes from `cargo add` to a crash-proof, replayable agent.

## 1. Install

```toml
# Cargo.toml
[dependencies]
pragmatic = "0.2"
pragmatic-anthropic = "0.2"   # or implement Oracle for your own model client
```

## 2. Write the agent

Agents are plain functions over `Ctx`. Route model calls through
`ctx.oracle`, external writes through `ctx.effect`, and mark the function
`#[pragmatic::durable]`:

```rust
use pragmatic::{Ctx, Fault, Value};

#[pragmatic::durable]
fn research(ctx: &mut Ctx) -> Result<Value, Fault> {
    let plan = ctx.oracle("plan the research task")?;          // journaled

    let mut findings = Vec::new();
    for step in 0..10 {
        findings.push(ctx.oracle(format!("probe {step}: {plan}"))?);
    }

    // A durable effect: journaled *before* it runs (write-ahead), its
    // result journaled after. Replay reuses the result; the world is
    // touched exactly once.
    ctx.effect("publish", format!("{} findings", findings.len()), |arg| {
        // your real side effect here: S3 write, webhook, DB insert...
        Ok(Value::from(format!("s3://reports/{arg}")))
    })
}
```

## 3. Run it durably

```rust
use pragmatic::Runtime;
use pragmatic_anthropic::AnthropicOracle;

let oracle = AnthropicOracle::from_env()?      // ANTHROPIC_API_KEY
    .model("claude-sonnet-5")
    .max_tokens(1024);

let mut rt = Runtime::on_dir("./journals", oracle)?;
let report = rt.run("research-42", research)?;
println!("{}", report.output);
```

Every oracle call's realized outcome now lands in
`./journals/research-42.journal` — append-only, hash-chained, synced to disk
as the run proceeds.

## 4. Crash. Recover.

Kill the process anywhere — step nine of a thousand. Then:

```rust
let report = rt.resume("research-42", research)?;
```

The nine recorded steps are **read back from the journal** (zero model
calls, zero duplicate effects); recording continues at step nine. The agent
function cannot tell the difference — that is Theorem T1's content.

## 5. Replay, exactly

```rust
let audit = rt.replay("research-42", research)?;   // the model is NEVER called
assert_eq!(audit.trace, report.trace);
```

Strict replay reproduces the run bit-for-bit — nondeterministic model
behavior included — for time-travel debugging and audit.

## 6. Inspect from the terminal

```sh
cargo install pragmatic-cli

pragmatic runs   --dir ./journals            # every run, chain status
pragmatic show   research-42 --dir ./journals
pragmatic verify --all --dir ./journals      # walk the tamper-evident chain
pragmatic export research-42 --dir ./journals -o run.html   # replay console
```

## Where next

- [Concepts](concepts.md) — the execution model: Oracle, Journal, effects,
  faults, capabilities.
- `examples/tool_use_retries.rs` — flaky tools under a Supervisor.
- `examples/map_reduce.rs` — multi-run coordination with per-agent recovery.
- The [research page](https://aniketh.net/pragmatic#research) — the calculus
  and proofs underneath.
