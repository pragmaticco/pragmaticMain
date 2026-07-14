//! The landing-page example, runnable: a research agent that plans, probes,
//! and synthesizes — then crashes on step nine of its probe loop, resumes
//! exactly (no re-sampling), and replays bit-for-bit for audit.
//!
//! ```sh
//! cargo run --example research_agent
//! ```

use pragmatic::{Ctx, Fault, Runtime, SeededOracle, Value};

/// Wrap the run. Every oracle call is journaled once.
fn research(ctx: &mut Ctx) -> Result<Value, Fault> {
    let plan = ctx.oracle("plan: survey durable execution for agents")?; // journaled

    let mut findings = Vec::new();
    for step in 0..12 {
        let f = ctx.oracle(format!("probe {step} of plan {plan}"))?; // journaled
        findings.push(f);
    }

    // A durable effect under the write-ahead discipline: intent journaled
    // before the world changes, result journaled after.
    let stored = ctx.effect(
        "publish_report",
        format!("{} findings", findings.len()),
        |arg| Ok(Value::from(format!("s3://reports/{arg}"))),
    )?;

    Ok(Value::from(format!("report at {stored}")))
}

fn main() {
    let dir = std::env::temp_dir().join("pragmatic-research-agent");
    let _ = std::fs::remove_dir_all(&dir);

    // ── Record ──────────────────────────────────────────────────────────
    let mut rt = Runtime::on_dir(&dir, SeededOracle::new(42))
        .expect("journal dir")
        .with_key(b"demo-signing-key");
    let recorded = rt.run("research-42", research).expect("run");
    println!("recorded  : {} journal entries", recorded.journal_len);
    println!("output    : {}", recorded.output);

    // ── Crash on step nine ──────────────────────────────────────────────
    // Simulate the process dying with only 9 entries durable on disk.
    rt.journal("research-42").unwrap().truncate(9).unwrap();
    drop(rt); // the process is gone

    // ── Recover from any crash ──────────────────────────────────────────
    // New process, same journal dir. The 9 surviving steps are read back
    // from the Journal instead of paid for again; the run continues on
    // step nine.
    let mut rt = Runtime::on_dir(&dir, SeededOracle::new(42))
        .expect("journal dir")
        .with_key(b"demo-signing-key");
    let resumed = rt.resume("research-42", research).expect("resume");
    println!(
        "resumed   : {} steps replayed from journal, {} recorded fresh",
        resumed.replayed_steps, resumed.fresh_steps
    );
    println!("output    : {}", resumed.output);

    // ── Replay faithfully (T1) ──────────────────────────────────────────
    // Reproduce the whole run for audit. The model is never called.
    let audit = rt.replay("research-42", research).expect("replay");
    assert_eq!(
        audit.trace, resumed.trace,
        "T1: replay reproduces the run exactly"
    );
    println!(
        "replayed  : trace identical, {} steps, zero model calls",
        audit.replayed_steps
    );

    // ── The signed audit trail ──────────────────────────────────────────
    let verified = rt.verify("research-42").unwrap().is_ok();
    println!(
        "audit log : hash chain verified = {verified}, head = {}",
        audit
            .chain_head
            .map(|h| pragmatic::sha256::hex(&h)[..16].to_string())
            .unwrap_or_default()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
