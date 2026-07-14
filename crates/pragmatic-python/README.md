# pragmatic (Python)

**Durable execution for agents that don't run deterministically.**

Python bindings for the [Pragmatic](https://github.com/pragmaticco/pragmaticMain)
Rust runtime: every model call your agent makes is journaled to an
append-only, hash-chained log, so the run survives any crash and replays
exactly — nondeterministic model behavior included.

```python
import pragmatic

def oracle(prompt: str) -> str:
    # your model call — Anthropic SDK, OpenAI, a local server, anything
    ...

rt = pragmatic.Runtime("./journals", oracle)

def research(ctx):
    plan = ctx.oracle("plan the task")                      # journaled
    findings = [ctx.oracle(f"probe {i}: {plan}") for i in range(10)]
    return ctx.effect("publish", f"{len(findings)} findings",
                      lambda arg: f"s3://reports/{arg}")    # write-ahead journaled

report = rt.run("research-42", research)

# Crash anywhere. Resume exactly — recorded steps are read back from the
# journal, not re-sampled (and not re-paid-for):
report = rt.resume("research-42", research)

# Replay bit-for-bit for audit; the model is never called:
audit = rt.replay("research-42", research)
assert audit.trace == report.trace
assert rt.verify("research-42")   # tamper-evident hash chain
```

## Install

```sh
pip install pragmatic-runtime      # or: maturin develop (from source)
```

## Build from source

```sh
pip install maturin
cd crates/pragmatic-python
maturin develop --release
```

The journal files are the same format the Rust runtime and the `pragmatic`
CLI read — `pragmatic export research-42 --dir ./journals` gives you the
HTML replay console for runs recorded from Python.
