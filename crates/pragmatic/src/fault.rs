//! Fault taxonomy (paper Definition 3.13, §4.1 fault tags).
//!
//! Faults are well-typed values, not panics: every failure an agent can hit
//! has a tag the Supervisor can dispatch on. This is what turns "the process
//! died somewhere" into "restart from the journal" as a defined transition.

use std::fmt;

use crate::journal::Cursor;

/// A fault raised during a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The oracle (model) call itself failed.
    OracleErr(String),
    /// A durable effect (tool call, external write) failed while performing.
    ToolErr(String),
    /// A deadline was exceeded.
    Timeout,
    /// A budget capability (step or cost limit) was consumed. Converts an
    /// unbounded loop into a well-typed fault with a supervisor response.
    BudgetExhausted,
    /// A verified agent contract was falsified.
    ContractViol(String),
    /// The agent attempted an effect it holds no capability for.
    CapabilityDenied(String),
    /// A fault propagated up from a child agent.
    Escalated(Box<Fault>),
    /// Replay found the journal disagreeing with the program: the re-executed
    /// code asked for a different step than the one recorded at `cursor`.
    /// This is assumption A2 (same term replayed) failing - the agent code
    /// changed between record and replay, or the journal belongs to a
    /// different program.
    JournalDesync {
        cursor: Cursor,
        expected: String,
        found: String,
    },
    /// Strict replay ran past the end of the journal: the recorded run never
    /// got this far (it crashed, or the journal is a prefix). `resume` is the
    /// right verb for continuing such a run; `replay` refuses to sample.
    ReplayExhausted { cursor: Cursor },
    /// Underlying storage failure.
    Io(String),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::OracleErr(m) => write!(f, "oracle error: {m}"),
            Fault::ToolErr(m) => write!(f, "tool error: {m}"),
            Fault::Timeout => write!(f, "timeout"),
            Fault::BudgetExhausted => write!(f, "budget exhausted"),
            Fault::ContractViol(m) => write!(f, "contract violated: {m}"),
            Fault::CapabilityDenied(m) => write!(f, "capability denied: {m}"),
            Fault::Escalated(inner) => write!(f, "escalated: {inner}"),
            Fault::JournalDesync {
                cursor,
                expected,
                found,
            } => write!(
                f,
                "journal desync at cursor {cursor}: program asked for {expected}, journal holds {found}"
            ),
            Fault::ReplayExhausted { cursor } => {
                write!(f, "replay exhausted at cursor {cursor}: recorded run ended here")
            }
            Fault::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for Fault {}

impl From<std::io::Error> for Fault {
    fn from(e: std::io::Error) -> Self {
        Fault::Io(e.to_string())
    }
}
