//! The research agent, async: same three guarantees, `await`ed. Runs on the
//! built-in executor (`pragmatic::block_on`) - swap in tokio et al. freely;
//! the runtime's futures are executor-agnostic.
//!
//! ```sh
//! cargo run -p pragmatic --example async_research
//! ```

use pragmatic::{block_on, AsyncCtx, AsyncRuntime, Fault, SeededOracle, Value};

#[pragmatic::durable]
async fn research(ctx: &mut AsyncCtx<'_, SeededOracle>) -> Result<Value, Fault> {
    let plan = ctx
        .oracle("plan: survey durable execution for agents")
        .await?;

    let mut findings = Vec::new();
    for step in 0..12 {
        findings.push(ctx.oracle(format!("probe {step} of plan {plan}")).await?);
    }

    let stored = ctx
        .effect(
            "publish_report",
            format!("{} findings", findings.len()),
            async |arg| Ok(Value::from(format!("s3://reports/{arg}"))),
        )
        .await?;

    Ok(Value::from(format!("report at {stored}")))
}

fn main() {
    let dir = std::env::temp_dir().join("pragmatic-async-research");
    let _ = std::fs::remove_dir_all(&dir);

    let mut rt = AsyncRuntime::on_dir(&dir, SeededOracle::new(42)).expect("journal dir");

    let recorded = block_on(rt.run("research-42", research)).expect("run");
    println!("recorded  : {} journal entries", recorded.journal_len);
    println!("output    : {}", recorded.output);

    // Crash on step nine; resume on step nine.
    rt.journal("research-42").unwrap().truncate(9).unwrap();
    let resumed = block_on(rt.resume("research-42", research)).expect("resume");
    println!(
        "resumed   : {} steps replayed from journal, {} recorded fresh",
        resumed.replayed_steps, resumed.fresh_steps
    );

    // Replay bit-for-bit - the model is never consulted (T1).
    let audit = block_on(rt.replay("research-42", research)).expect("replay");
    assert_eq!(audit.trace, resumed.trace);
    println!("replayed  : trace identical, zero model calls");

    let _ = std::fs::remove_dir_all(&dir);
}
