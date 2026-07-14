//! `Value` — the realized outcome of a journaled step.
//!
//! The calculus treats realized oracle outcomes, channel messages, and effect
//! results as closed, irreducible *values* (paper §4.1). The runtime carries
//! them as owned byte strings: the Journal must be able to store and replay
//! any outcome byte-for-byte without interpreting it, and agents almost always
//! move text (prompts and completions), so `Value` is a thin newtype over
//! `Vec<u8>` with string conveniences.

use std::fmt;

/// A realized value: opaque bytes, usually UTF-8 text.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct Value(pub Vec<u8>);

impl Value {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Value(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Lossy UTF-8 view — realized model outputs are text in practice.
    pub fn as_str(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value(s.as_bytes().to_vec())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value(s.into_bytes())
    }
}

impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Self {
        Value(b)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Value({:?})", self.as_str())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
