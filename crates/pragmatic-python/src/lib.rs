//! Python bindings for Pragmatic — durable execution for agents that don't
//! run deterministically.
//!
//! ```python
//! import pragmatic
//!
//! def oracle(prompt: str) -> str:
//!     ...  # your model call (Anthropic SDK, OpenAI, local server, ...)
//!
//! rt = pragmatic.Runtime("./journals", oracle)
//!
//! def research(ctx):
//!     plan = ctx.oracle("plan the task")                     # journaled
//!     findings = [ctx.oracle(f"probe {i}: {plan}") for i in range(10)]
//!     return ctx.effect("publish", f"{len(findings)} findings",
//!                       lambda arg: f"s3://reports/{arg}")   # write-ahead journaled
//!
//! report = rt.run("research-42", research)     # record
//! report = rt.resume("research-42", research)  # crash-recover, no re-sampling
//! audit  = rt.replay("research-42", research)  # bit-for-bit, model never called
//! ```
//!
//! The runtime is the Rust core; these bindings add Python callables as
//! oracles/agents and translate `Fault` into `RuntimeError`.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use pragmatic_rt::{Fault, Oracle, TraceLabel, Value};

fn fault_err(f: Fault) -> PyErr {
    PyRuntimeError::new_err(f.to_string())
}

/// A `pragmatic_rt::Oracle` backed by a Python callable `(str) -> str`.
struct PyCallableOracle {
    func: PyObject,
}

impl Oracle for PyCallableOracle {
    fn call(&self, prompt: &Value) -> Result<Value, Fault> {
        Python::with_gil(|py| {
            let out = self
                .func
                .call1(py, (prompt.as_str().into_owned(),))
                .map_err(|e| Fault::OracleErr(format!("python oracle raised: {e}")))?;
            let text: String = out
                .extract(py)
                .map_err(|e| Fault::OracleErr(format!("python oracle must return str: {e}")))?;
            Ok(Value::from(text))
        })
    }

    fn provenance(&self) -> String {
        Python::with_gil(|py| {
            self.func
                .bind(py)
                .getattr("__name__")
                .and_then(|n| n.extract::<String>())
                .map(|n| format!("python:{n}"))
                .unwrap_or_else(|_| "python-oracle".to_string())
        })
    }
}

/// The durable execution context handed to Python agents. Valid only during
/// the `run`/`resume`/`replay` call that created it; the pointer is nulled
/// when the attempt finishes, so a leaked reference fails loudly instead of
/// dangling.
#[pyclass(unsendable)]
struct Ctx {
    ptr: *mut pragmatic_rt::Ctx<'static>,
}

impl Ctx {
    fn get(&mut self) -> PyResult<&mut pragmatic_rt::Ctx<'static>> {
        unsafe { self.ptr.as_mut() }
            .ok_or_else(|| PyRuntimeError::new_err("ctx used outside its run"))
    }
}

#[pymethods]
impl Ctx {
    /// One model call, journaled once. Record: sample; replay: read back —
    /// the model is not called.
    fn oracle(&mut self, prompt: &str) -> PyResult<String> {
        let v = self.get()?.oracle(prompt).map_err(fault_err)?;
        Ok(v.as_str().into_owned())
    }

    /// A durable effect under the write-ahead discipline. `perform` is a
    /// Python callable `(str) -> str` — your tool call / external write.
    /// Replay reuses the recorded result without re-performing.
    fn effect(&mut self, name: &str, arg: &str, perform: PyObject) -> PyResult<String> {
        let ctx = self.get()?;
        let result = ctx
            .effect(name, arg, |a| {
                Python::with_gil(|py| {
                    let out = perform
                        .call1(py, (a.as_str().into_owned(),))
                        .map_err(|e| Fault::ToolErr(format!("python effect raised: {e}")))?;
                    let text: String = out.extract(py).map_err(|e| {
                        Fault::ToolErr(format!("python effect must return str: {e}"))
                    })?;
                    Ok(Value::from(text))
                })
            })
            .map_err(fault_err)?;
        Ok(result.as_str().into_owned())
    }

    /// Receive on a channel (journaled).
    fn recv(&mut self, channel: &str) -> PyResult<String> {
        let v = self.get()?.recv(channel).map_err(fault_err)?;
        Ok(v.as_str().into_owned())
    }

    /// A journaled clock read (nanoseconds since epoch).
    fn now(&mut self) -> PyResult<u64> {
        self.get()?.now().map_err(fault_err)
    }

    /// Assert an agent contract; a falsified contract raises.
    fn contract(&mut self, holds: bool, msg: &str) -> PyResult<()> {
        self.get()?.contract(holds, msg).map_err(fault_err)
    }

    /// True while steps are served from the journal.
    fn is_replaying(&mut self) -> PyResult<bool> {
        Ok(self.get()?.is_replaying())
    }
}

/// The result of driving one run to completion.
#[pyclass]
struct RunReport {
    #[pyo3(get)]
    run_id: String,
    #[pyo3(get)]
    output: String,
    #[pyo3(get)]
    journal_len: u64,
    #[pyo3(get)]
    replayed_steps: u64,
    #[pyo3(get)]
    fresh_steps: u64,
    /// Hex head of the tamper-evident hash chain, or None for an empty run.
    #[pyo3(get)]
    chain_head: Option<String>,
    /// The observable trace: (cursor, kind, summary) triples. Identical
    /// between a recorded run and its replay (T1).
    #[pyo3(get)]
    trace: Vec<(u64, String, String)>,
}

fn convert_report(r: pragmatic_rt::RunReport) -> RunReport {
    let trace = r
        .trace
        .iter()
        .map(|l| match l {
            TraceLabel::Oracle { cursor, outcome } => {
                (*cursor, "oracle".to_string(), outcome.as_str().into_owned())
            }
            TraceLabel::Effect {
                cursor,
                name,
                result,
            } => (
                *cursor,
                "effect".to_string(),
                format!("{name} -> {}", result.as_str()),
            ),
            TraceLabel::Recv {
                cursor,
                channel,
                value,
            } => (
                *cursor,
                "recv".to_string(),
                format!("{channel} ? {}", value.as_str()),
            ),
            TraceLabel::Clock { cursor, nanos } => {
                (*cursor, "clock".to_string(), nanos.to_string())
            }
            TraceLabel::Done { output } => {
                (u64::MAX, "done".to_string(), output.as_str().into_owned())
            }
        })
        .collect();
    RunReport {
        run_id: r.run_id,
        output: r.output.as_str().into_owned(),
        journal_len: r.journal_len,
        replayed_steps: r.replayed_steps,
        fresh_steps: r.fresh_steps,
        chain_head: r.chain_head.map(|h| pragmatic_rt::sha256::hex(&h)),
        trace,
    }
}

enum Mode {
    Run,
    Resume,
    Replay,
}

/// The Pragmatic runtime: journals every step an agent takes under a stable
/// run id, so it survives any crash and replays exactly.
#[pyclass(unsendable)]
struct Runtime {
    inner: pragmatic_rt::Runtime<PyCallableOracle>,
}

impl Runtime {
    fn drive(
        &mut self,
        py: Python<'_>,
        run_id: &str,
        agent: PyObject,
        mode: Mode,
    ) -> PyResult<RunReport> {
        let agent_fn = |ctx: &mut pragmatic_rt::Ctx| -> Result<Value, Fault> {
            // The Python-visible ctx borrows this attempt's Ctx. The pointer
            // is nulled before the attempt returns, so references Python
            // keeps around cannot dangle.
            let ptr: *mut pragmatic_rt::Ctx<'static> = (ctx as *mut pragmatic_rt::Ctx<'_>).cast();
            let pyctx = Py::new(py, Ctx { ptr })
                .map_err(|e| Fault::ToolErr(format!("ctx allocation failed: {e}")))?;
            let result = agent.call1(py, (pyctx.clone_ref(py),));
            pyctx.borrow_mut(py).ptr = std::ptr::null_mut();
            match result {
                Ok(v) => {
                    let text: String = v.extract(py).map_err(|e| {
                        Fault::ToolErr(format!("python agent must return str: {e}"))
                    })?;
                    Ok(Value::from(text))
                }
                // A Fault we raised (oracle/effect/contract) round-trips as
                // its message; a genuine Python error is a tool fault.
                Err(e) => Err(Fault::ToolErr(format!("python agent raised: {e}"))),
            }
        };
        let report = match mode {
            Mode::Run => self.inner.run(run_id, agent_fn),
            Mode::Resume => self.inner.resume(run_id, agent_fn),
            Mode::Replay => self.inner.replay(run_id, agent_fn),
        }
        .map_err(fault_err)?;
        Ok(convert_report(report))
    }
}

#[pymethods]
impl Runtime {
    /// `Runtime(dir, oracle, key=None)` — journals persist under `dir` (one
    /// append-only file per run); `oracle` is a callable `(str) -> str`
    /// wrapping your model; `key` (bytes) HMAC-signs the journals.
    #[new]
    #[pyo3(signature = (dir, oracle, key=None))]
    fn new(dir: &str, oracle: PyObject, key: Option<Bound<'_, PyBytes>>) -> PyResult<Self> {
        let mut inner = pragmatic_rt::Runtime::on_dir(dir, PyCallableOracle { func: oracle })
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        if let Some(k) = key {
            inner = inner.with_key(k.as_bytes());
        }
        Ok(Runtime { inner })
    }

    /// Start (or continue) a durable run. Re-enterable: an existing journal
    /// prefix is replayed first, so retries are idempotent.
    fn run(&mut self, py: Python<'_>, run_id: &str, agent: PyObject) -> PyResult<RunReport> {
        self.drive(py, run_id, agent, Mode::Run)
    }

    /// Resume a crashed run: the journaled prefix is read back (zero model
    /// calls, no duplicate effects); recording continues at the tail.
    fn resume(&mut self, py: Python<'_>, run_id: &str, agent: PyObject) -> PyResult<RunReport> {
        self.drive(py, run_id, agent, Mode::Resume)
    }

    /// Replay a recorded run bit-for-bit for debugging and audit. The model
    /// is never called.
    fn replay(&mut self, py: Python<'_>, run_id: &str, agent: PyObject) -> PyResult<RunReport> {
        self.drive(py, run_id, agent, Mode::Replay)
    }

    /// Deliver a value to a run's channel inbox.
    fn send(&mut self, run_id: &str, channel: &str, value: &str) {
        self.inner.send(run_id, channel, value);
    }

    /// Walk the run's tamper-evident hash chain. Returns True if intact.
    fn verify(&mut self, run_id: &str) -> PyResult<bool> {
        Ok(self.inner.verify(run_id).map_err(fault_err)?.is_ok())
    }
}

/// Durable execution for agents that don't run deterministically.
#[pymodule]
fn pragmatic(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Runtime>()?;
    m.add_class::<Ctx>()?;
    m.add_class::<RunReport>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
