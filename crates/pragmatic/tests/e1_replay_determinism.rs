//! **E1 - Replay determinism (T1, empirically).**
//!
//! For 1000 independently seeded stochastic runs: record the run, then
//! replay it strictly (model never called) and assert the observable trace
//! and output are identical. The landing-page claim "1000 / 1000 replay
//! determinism across the reference test suite" is this test.

use pragmatic::{CountingOracle, Ctx, Fault, Runtime, SeededOracle, Value};

/// A branching, stochastic agent: samples a plan, branches on the realized
/// outcome (so control flow depends on nondeterminism), loops a data-driven
/// number of times, and commits an effect.
fn agent(ctx: &mut Ctx) -> Result<Value, Fault> {
    let plan = ctx.oracle("plan the task")?;
    // Branch on realized randomness: byte sum picks the strategy.
    let weight: u64 = plan.as_bytes().iter().map(|b| *b as u64).sum();
    let steps = (weight % 4) + 1;
    let mut findings = Vec::new();
    for i in 0..steps {
        let f = ctx.oracle(format!("probe step {i} of plan {plan}"))?;
        if f.as_str().starts_with("halt") {
            break; // data-dependent early exit
        }
        findings.push(f);
    }
    let t = ctx.now()?;
    let report = ctx.effect(
        "write_report",
        format!("{} findings @{t}", findings.len()),
        |arg| Ok(Value::from(format!("stored({arg})"))),
    )?;
    Ok(report)
}

#[test]
fn e1_replay_determinism_1000_of_1000() {
    let mut identical = 0u32;
    const N: u64 = 1000;
    for seed in 0..N {
        let mut rt = Runtime::in_memory(CountingOracle::new(SeededOracle::new(seed)));
        let run_id = format!("e1-{seed}");

        let recorded = rt.run(&run_id, agent).expect("record run succeeds");
        let calls_after_record = rt.oracle().calls();

        let replayed = rt.replay(&run_id, agent).expect("replay succeeds");

        // Zero model calls during replay - the oracle was never consulted.
        assert_eq!(
            rt.oracle().calls(),
            calls_after_record,
            "seed {seed}: replay called the model"
        );
        // The whole observable trace is byte-identical, output included.
        assert_eq!(
            recorded.trace, replayed.trace,
            "seed {seed}: trace diverged"
        );
        assert_eq!(
            recorded.output, replayed.output,
            "seed {seed}: output diverged"
        );
        assert_eq!(
            replayed.fresh_steps, 0,
            "seed {seed}: replay recorded fresh steps"
        );
        identical += 1;
    }
    assert_eq!(identical, N as u32, "replay determinism must be {N}/{N}");
}

#[test]
fn e1_distinct_seeds_actually_diverge() {
    // Sanity check that E1 is not vacuous: the oracle is genuinely
    // stochastic across seeds, so identical traces are earned, not implied.
    let mut outputs = std::collections::HashSet::new();
    for seed in 0..50 {
        let mut rt = Runtime::in_memory(SeededOracle::new(seed));
        let r = rt.run("x", agent).unwrap();
        outputs.insert(r.output.as_str().into_owned());
    }
    assert!(
        outputs.len() > 10,
        "seeded runs should diverge; got {} distinct outputs",
        outputs.len()
    );
}

#[test]
fn e1_replay_is_idempotent() {
    // Replaying a prefix does not extend the journal (Lemma 5.3): replay
    // twice, journal length unchanged both times.
    let mut rt = Runtime::in_memory(SeededOracle::new(99));
    let recorded = rt.run("idem", agent).unwrap();
    let len = recorded.journal_len;
    let r1 = rt.replay("idem", agent).unwrap();
    let r2 = rt.replay("idem", agent).unwrap();
    assert_eq!(r1.journal_len, len);
    assert_eq!(r2.journal_len, len);
    assert_eq!(r1.trace, r2.trace);
}
