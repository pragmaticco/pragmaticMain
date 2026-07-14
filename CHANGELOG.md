# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com); versions follow
[SemVer](https://semver.org) (pre-1.0: minor bumps may break).

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

[0.2.0]: https://github.com/pragmaticco/pragmaticMain/releases/tag/v0.2.0
