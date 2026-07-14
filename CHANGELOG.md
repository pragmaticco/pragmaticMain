# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com); versions follow
[SemVer](https://semver.org) (pre-1.0: minor bumps may break).

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
