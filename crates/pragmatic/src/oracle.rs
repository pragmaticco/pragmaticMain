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
