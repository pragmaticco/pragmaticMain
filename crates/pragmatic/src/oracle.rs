//! The **Oracle** — an LLM call modeled as a first-class probability
//! distribution (paper Definition 3.5), not an opaque activity.
//!
//! The Oracle is the *sole* source of probabilistic nondeterminism in the
//! runtime (design invariant I1). Every realized outcome flows through the
//! Journal; replay reads outcomes back instead of calling the model again.
//!
//! Implement [`Oracle`] for your model client (Anthropic, OpenAI, a local
//! server, a tool with stochastic output — anything whose result is a draw
//! from a distribution). The runtime does not depend on any particular model.

use std::cell::Cell;

use crate::fault::Fault;
use crate::sha256::sha256;
use crate::value::Value;

/// A model call: prompt in, one realized outcome out.
///
/// `call` is only invoked in record mode ([O-rec]) or when a resumed run
/// walks past its journaled prefix ([O-resume]). In replay mode the runtime
/// never calls it — that is the whole point.
pub trait Oracle {
    /// Sample one outcome for `prompt` from the model's distribution.
    fn call(&self, prompt: &Value) -> Result<Value, Fault>;

    /// Identifies the model/sampler for the journal's provenance field
    /// (e.g. `"claude-sonnet-5@temperature=0.7"`). Audit metadata; never
    /// consulted during replay.
    fn provenance(&self) -> String {
        "oracle".to_string()
    }
}

/// A seeded, genuinely stochastic test oracle.
///
/// Each call draws from a deterministic PRNG stream (xorshift64*, seeded),
/// mixed with the prompt digest — so different seeds give different runs,
/// the *sequence* of calls matters (stateful, like a real sampler), and
/// tests get reproducible nondeterminism without a live model.
pub struct SeededOracle {
    state: Cell<u64>,
    seed: u64,
}

impl SeededOracle {
    pub fn new(seed: u64) -> Self {
        // xorshift needs a nonzero state.
        let s = if seed == 0 { 0x9E3779B97F4A7C15 } else { seed };
        SeededOracle {
            state: Cell::new(s),
            seed: s,
        }
    }

    fn next(&self) -> u64 {
        // xorshift64* — small, fast, fine for test randomness.
        let mut x = self.state.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state.set(x);
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

impl Oracle for SeededOracle {
    fn call(&self, prompt: &Value) -> Result<Value, Fault> {
        // Mix the PRNG draw with the prompt so outcomes depend on both the
        // sampler state and what was asked.
        let draw = self.next();
        let d = sha256(prompt.as_bytes());
        let mix = draw ^ u64::from_le_bytes(d[..8].try_into().unwrap());
        // A small "completion": one of a few templated outputs plus a token
        // that varies with the draw. Enough entropy that distinct runs
        // genuinely diverge.
        let shape = mix % 4;
        let token = mix >> 2;
        let out = match shape {
            0 => format!("plan:{token:x}"),
            1 => format!("answer:{token:x}"),
            2 => format!("refine:{token:x}"),
            _ => format!("halt:{token:x}"),
        };
        Ok(Value::from(out))
    }

    fn provenance(&self) -> String {
        format!("seeded-oracle@{}", self.seed)
    }
}

/// Wraps any oracle and counts calls — the instrument for proving "replay
/// performs zero model calls" and "resume never re-samples the prefix".
pub struct CountingOracle<O> {
    inner: O,
    calls: Cell<u64>,
}

impl<O: Oracle> CountingOracle<O> {
    pub fn new(inner: O) -> Self {
        CountingOracle {
            inner,
            calls: Cell::new(0),
        }
    }

    /// Number of live model calls made through this oracle.
    pub fn calls(&self) -> u64 {
        self.calls.get()
    }
}

impl<O: Oracle> Oracle for CountingOracle<O> {
    fn call(&self, prompt: &Value) -> Result<Value, Fault> {
        self.calls.set(self.calls.get() + 1);
        self.inner.call(prompt)
    }

    fn provenance(&self) -> String {
        self.inner.provenance()
    }
}

/// Wraps any oracle with bounded retries and exponential backoff on
/// transient model failures.
///
/// Only [`Fault::OracleErr`] is retried — that is the tag for "the model
/// call itself failed" (rate limit, overload, transport). Every other fault
/// is a property of the run, not the call, and passes straight through.
/// Because the runtime journals the *realized* outcome, retries happen
/// entirely before anything is recorded: a completion that finally succeeds
/// is journaled once, and replay never sees the failed attempts.
///
/// ```
/// use pragmatic::{RetryOracle, SeededOracle};
/// use std::time::Duration;
///
/// let oracle = RetryOracle::new(SeededOracle::new(7))
///     .max_retries(4)
///     .base_delay(Duration::from_millis(250)); // 250ms, 500ms, 1s, 2s
/// ```
pub struct RetryOracle<O> {
    inner: O,
    max_retries: u32,
    base_delay: std::time::Duration,
    /// Injectable for tests; `std::thread::sleep` in production.
    sleep: fn(std::time::Duration),
}

impl<O: Oracle> RetryOracle<O> {
    /// Defaults: 3 retries, 250 ms base delay, doubling per attempt.
    pub fn new(inner: O) -> Self {
        RetryOracle {
            inner,
            max_retries: 3,
            base_delay: std::time::Duration::from_millis(250),
            sleep: std::thread::sleep,
        }
    }

    /// Retries after the first attempt (0 = fail immediately, like `inner`).
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Delay before the first retry; each further retry doubles it.
    pub fn base_delay(mut self, delay: std::time::Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Replace the sleeper (tests: record delays instead of waiting).
    pub fn with_sleep(mut self, sleep: fn(std::time::Duration)) -> Self {
        self.sleep = sleep;
        self
    }
}

impl<O: Oracle> Oracle for RetryOracle<O> {
    fn call(&self, prompt: &Value) -> Result<Value, Fault> {
        let mut delay = self.base_delay;
        let mut attempt = 0;
        loop {
            match self.inner.call(prompt) {
                Err(Fault::OracleErr(_)) if attempt < self.max_retries => {
                    attempt += 1;
                    (self.sleep)(delay);
                    delay = delay.saturating_mul(2);
                }
                other => return other,
            }
        }
    }

    fn provenance(&self) -> String {
        format!(
            "{}+retry({}x{}ms)",
            self.inner.provenance(),
            self.max_retries,
            self.base_delay.as_millis()
        )
    }
}

/// An oracle that refuses every call. Strict replay is defined by "the model
/// is never consulted"; internally, replay runs against this to make that a
/// hard guarantee rather than a convention.
pub struct RefusingOracle;

impl Oracle for RefusingOracle {
    fn call(&self, _prompt: &Value) -> Result<Value, Fault> {
        Err(Fault::OracleErr(
            "replay mode must never call the oracle".to_string(),
        ))
    }

    fn provenance(&self) -> String {
        "refusing-oracle".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    /// Fails with `fault` for the first `failures` calls, then succeeds.
    struct FlakyOracle {
        failures: Cell<u64>,
        calls: Cell<u64>,
        fault: fn() -> Fault,
    }

    impl FlakyOracle {
        fn new(failures: u64, fault: fn() -> Fault) -> Self {
            FlakyOracle {
                failures: Cell::new(failures),
                calls: Cell::new(0),
                fault,
            }
        }
    }

    impl Oracle for FlakyOracle {
        fn call(&self, _prompt: &Value) -> Result<Value, Fault> {
            self.calls.set(self.calls.get() + 1);
            if self.failures.get() > 0 {
                self.failures.set(self.failures.get() - 1);
                return Err((self.fault)());
            }
            Ok(Value::from("ok"))
        }
    }

    static SLEPT: Mutex<Vec<Duration>> = Mutex::new(Vec::new());

    fn record_sleep(d: Duration) {
        SLEPT.lock().unwrap().push(d);
    }

    #[test]
    fn retries_transient_oracle_errors_with_doubling_backoff() {
        SLEPT.lock().unwrap().clear();
        let oracle = RetryOracle::new(FlakyOracle::new(2, || {
            Fault::OracleErr("overloaded".to_string())
        }))
        .base_delay(Duration::from_millis(10))
        .with_sleep(record_sleep);

        let out = oracle.call(&Value::from("p")).unwrap();
        assert_eq!(out.as_str(), "ok");
        assert_eq!(oracle.inner.calls.get(), 3); // 2 failures + 1 success
        assert_eq!(
            *SLEPT.lock().unwrap(),
            vec![Duration::from_millis(10), Duration::from_millis(20)]
        );
    }

    #[test]
    fn gives_up_after_max_retries() {
        let oracle = RetryOracle::new(FlakyOracle::new(u64::MAX, || {
            Fault::OracleErr("down".to_string())
        }))
        .max_retries(2)
        .base_delay(Duration::ZERO)
        .with_sleep(|_| {});

        assert!(matches!(
            oracle.call(&Value::from("p")),
            Err(Fault::OracleErr(_))
        ));
        assert_eq!(oracle.inner.calls.get(), 3); // initial + 2 retries
    }

    #[test]
    fn non_transient_faults_pass_through_unretried() {
        let oracle =
            RetryOracle::new(FlakyOracle::new(1, || Fault::BudgetExhausted)).with_sleep(|_| {});
        assert!(matches!(
            oracle.call(&Value::from("p")),
            Err(Fault::BudgetExhausted)
        ));
        assert_eq!(oracle.inner.calls.get(), 1);
    }

    #[test]
    fn provenance_names_the_policy() {
        let oracle = RetryOracle::new(SeededOracle::new(1))
            .max_retries(4)
            .base_delay(Duration::from_millis(250));
        assert_eq!(oracle.provenance(), "seeded-oracle@1+retry(4x250ms)");
    }
}
