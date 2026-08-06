# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com); versions follow
[SemVer](https://semver.org) (pre-1.0: minor bumps may break).

## [0.4.0] — 2026-08-06 · polyglot launch

### Added
- **C ABI** (`pragmatic-ffi`, `include/pragmatic.h`): the full runtime
  surface — run/resume/replay/send/verify plus ctx oracle/effect/recv/
  now/contract — as one flat `extern "C"` API with callback-based oracles,
  agents, and effects. Typed faults cross the boundary intact (budget stays
  budget, desync stays desync) via a pending-fault channel on the ctx.
  Zero dependencies; exercised by its own Rust-side C-discipline tests.
- **Language bindings** (`bindings/`), all on the C ABI, all journaling
  byte-compatibly with Rust and Python, each with a smoke test running the
  shared record → resume → replay → verify → typed-fault scenario:
  - **C++** (`bindings/cpp`): header-only C++17 RAII wrapper; lambdas as
    oracles/agents/effects; faults as `FaultError`; callback exceptions
    caught at the boundary.
  - **Java** (`bindings/java`): pure Java over `java.lang.foreign`
    (JDK 22+), no JNI glue to compile; try-with-resources runtimes and
    reports; faults as `FaultException`.
  - **Go** (`bindings/go`): cgo package with `cgo.Handle`-based callbacks;
    faults as `*Fault` with typed codes.
  - **Node.js** (`bindings/node`): pure-JS binding through koffi (Node has
    no built-in FFI); faults as `FaultError`; JS exceptions from callbacks
    rethrown intact.
- **`RetryOracle`** (core): wrap any oracle with bounded retries and
  doubling backoff on transient `OracleErr` faults — retries happen before
  anything is journaled, so a completion that finally succeeds is recorded
  once and replay never sees the failed attempts. Injectable sleeper for
  tests; provenance names the policy.
- **`pragmatic-openai`**: any OpenAI-compatible Chat Completions server as
  a journaled Oracle — OpenAI itself, or Ollama / vLLM / llama.cpp / Groq
  via `base_url` (keyless servers supported; no Authorization header sent
  without a key). Wire-tested over real sockets like the Anthropic
  adapter, including record-on-the-wire → kill-the-API → resume/replay
  from the journal; a `--ignored` test hits the live API when
  `OPENAI_API_KEY` is set.

## [0.3.0] — 2026-07-14 · customer-ready beta

### Added
- **Async runtime** (`AsyncOracle`, `AsyncCtx`, `AsyncRuntime`, `block_on`):
  the same guarantees for `async` agents, executor-agnostic (tokio, smol, or
  the built-in `block_on`), still zero dependencies. Every sync `Oracle` is
  automatically usable; `#[pragmatic::durable]` works on `async fn`.
- **Python bindings** (`pragmatic-python`, PyPI name `pragmatic-runtime`):
  `Runtime(dir, oracle)` with Python-callable oracles/agents/effects;
  run/resume/replay/verify/send; faults raise `RuntimeError`. Journals are
  byte-compatible with the Rust runtime and the CLI.
- **`pragmatic serve`**: a live, self-hostable web console over a journal
  directory (run index, chain status, dangling-effect warnings, per-run
  replay timelines). Loopback-only by default; zero dependencies.
- **Wire-level validation** of `pragmatic-anthropic`: real-socket tests
  against a Messages-API mock (success, 429, 529, malformed JSON,
  connection refused) plus the headline scenario — record on the wire,
  kill the API, resume and replay from the journal. A `--ignored` test
  hits the live API when `ANTHROPIC_API_KEY` is set.

### Fixed
- **Replay divergence** on journals containing a compensated effect
  followed by its re-performed pair (`Intent, Compensated, Intent,
  Commit`): the effect replay path now consumes every compensated pair
  and the final commit instead of desyncing later steps. Regression test
  included.

## [0.2.0] — 2026-07-13 · public beta

The first public release. Workspace of four crates:

### Added
- **`pragmatic`** (core, zero runtime dependencies)
  - `Journal`: append-only, cursor-keyed, SHA-256/HMAC hash-chained event
    log; in-memory and durable file backends; torn-tail truncation on open;
    tamper detection via `verify`.
  - `Oracle` trait: the LLM call as a first-class distribution; `SeededOracle`,
    `CountingOracle`, `RefusingOracle` for tests and audits.
  - `Ctx`: one agent code path for record and replay — journaled oracle
    draws, channel receives, clock reads; three-phase write-ahead durable
    effects; budget and effect-capability enforcement; `program_marker`.
  - `Runtime`: `run` / `resume` / `resume_with` / `replay` keyed by run id.
    Strict replay never calls the model. Dangling-intent recovery policies
    (`Recover::Commit` / `Recover::Compensate`).
  - `Supervisor`: fault → decision control plane (restart-from-journal,
    reverse-order saga compensation, escalation).
  - `Store`: IFC-labeled memory (no read-up / no write-down).
  - `Event::Program`: program identity journaled at step zero and verified
    on replay — changed agent code fails as `JournalDesync` instead of
    misreplaying.
- **`pragmatic-macros`**: `#[pragmatic::durable]` — hashes the function's
  source tokens and injects the program marker. Zero dependencies.
- **`pragmatic-anthropic`**: Claude as a journaled Oracle over the Messages
  API (model / system / temperature / max_tokens config, provenance
  metadata, faults mapped to `Fault::OracleErr`).
- **`pragmatic-cli`** (binary `pragmatic`): `runs`, `show`, `verify`, and
  `export` — a self-contained HTML replay console per run.

### Measured
- E1: 1000/1000 seeded stochastic runs replay byte-identically, zero model
  calls during replay.
- E2: crash at every journal prefix resumes exactly; no re-sampling of the
  surviving prefix; no orphaned effects; survives process restarts.
- E3: O(1) append, flat from 1k to 1M entries (~0.8 µs with full hash
  chaining, release build); ~1000× replay speedup under a 1 ms/draw oracle.

[0.4.0]: https://github.com/pragmaticco/pragmaticMain/releases/tag/v0.4.0
[0.3.0]: https://github.com/pragmaticco/pragmaticMain/releases/tag/v0.3.0
[0.2.0]: https://github.com/pragmaticco/pragmaticMain/releases/tag/v0.2.0
