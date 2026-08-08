//! **Memory** - an IFC-labeled store (paper §3.6).
//!
//! Cells carry an information-flow label from a two-point lattice
//! (`Low ⊑ High`). Reads enforce *no read-up* (a Low-cleared reader cannot
//! see High data); writes enforce *no write-down* (a High-cleared writer
//! cannot leak into Low cells). This is the runtime-level discipline behind
//! the noninterference result (T2): secret inputs cannot flow into public
//! observations.
//!
//! Memory is internally determined, so it is **not** journaled - replay
//! reconstructs it by re-execution (paper §4.5, [Mem-rd]/[Mem-wr]).

use std::collections::HashMap;

use crate::fault::Fault;
use crate::value::Value;

/// The two-point IFC lattice. `Low ⊑ High`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Label {
    Low,
    High,
}

impl Label {
    /// The lattice order ⊑ ("no more secret than").
    pub fn flows_to(self, other: Label) -> bool {
        self <= other
    }

    /// Least upper bound ⊔.
    pub fn join(self, other: Label) -> Label {
        self.max(other)
    }
}

/// A labeled store: `key → (label, value)`.
#[derive(Default)]
pub struct Store {
    cells: HashMap<String, (Label, Value)>,
}

impl Store {
    pub fn new() -> Self {
        Store::default()
    }

    /// Read `key` with `clearance`. Permitted iff `label(key) ⊑ clearance`
    /// (no read-up).
    pub fn read(&self, key: &str, clearance: Label) -> Result<&Value, Fault> {
        match self.cells.get(key) {
            None => Err(Fault::ToolErr(format!("memory: no cell '{key}'"))),
            Some((label, value)) => {
                if label.flows_to(clearance) {
                    Ok(value)
                } else {
                    Err(Fault::CapabilityDenied(format!(
                        "memory: read-up denied on '{key}' ({label:?} ⋢ {clearance:?})"
                    )))
                }
            }
        }
    }

    /// Write `key` at `label` as a writer running at `writer_label`.
    /// Permitted iff `writer_label ⊑ label` (no write-down).
    pub fn write(
        &mut self,
        key: &str,
        label: Label,
        value: impl Into<Value>,
        writer_label: Label,
    ) -> Result<(), Fault> {
        if !writer_label.flows_to(label) {
            return Err(Fault::CapabilityDenied(format!(
                "memory: write-down denied on '{key}' ({writer_label:?} ⋢ {label:?})"
            )));
        }
        self.cells.insert(key.to_string(), (label, value.into()));
        Ok(())
    }

    /// The Low-observable view of the store - what a public observer sees
    /// (paper §4.4 `obs_Lo`). T2's claim is that High inputs never change
    /// this projection.
    pub fn low_view(&self) -> Vec<(&str, &Value)> {
        let mut v: Vec<_> = self
            .cells
            .iter()
            .filter(|(_, (l, _))| *l == Label::Low)
            .map(|(k, (_, val))| (k.as_str(), val))
            .collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_read_up() {
        let mut s = Store::new();
        s.write("secret", Label::High, "hunter2", Label::High)
            .unwrap();
        assert!(s.read("secret", Label::Low).is_err());
        assert!(s.read("secret", Label::High).is_ok());
    }

    #[test]
    fn no_write_down() {
        let mut s = Store::new();
        // A High-running writer must not create Low-visible cells.
        assert!(s.write("leak", Label::Low, "secret!", Label::High).is_err());
        assert!(s.write("ok", Label::High, "secret!", Label::High).is_ok());
        assert!(s.low_view().is_empty());
    }

    #[test]
    fn low_view_only_shows_low() {
        let mut s = Store::new();
        s.write("public", Label::Low, "hello", Label::Low).unwrap();
        s.write("secret", Label::High, "shh", Label::Low).unwrap(); // write-up is fine
        let view = s.low_view();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].0, "public");
    }
}
