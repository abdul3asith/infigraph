//! Semantic diff between two git refs at the symbol level.
//!
//! Instead of a line diff, this compares the extracted symbol graphs of two
//! git tree-states and classifies each change as Added / Removed / BodyChanged /
//! SignatureChanged.  The caller supplies a project root and two git refs
//! (e.g. "HEAD~1", "main"); the module checks out each ref into a temp
//! worktree, indexes it with the current language registry, and returns a
//! structured `SymbolDiff`.

mod compute;
mod format;

pub use compute::*;
pub use format::*;

use serde::{Deserialize, Serialize};

/// How a symbol changed between two refs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChangeKind {
    /// Symbol exists in new ref but not in old ref.
    Added,
    /// Symbol exists in old ref but not in new ref.
    Removed,
    /// Symbol exists in both; signature_hash changed (parameter / return type change).
    SignatureChanged,
    /// Symbol exists in both; body changed but signature is the same.
    BodyChanged,
    /// Symbol moved to a different file.
    Moved { from_file: String },
    /// Symbol renamed in the same file (structurally similar body).
    Renamed { old_name: String },
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeKind::Added => write!(f, "ADDED"),
            ChangeKind::Removed => write!(f, "REMOVED"),
            ChangeKind::SignatureChanged => write!(f, "SIGNATURE_CHANGED"),
            ChangeKind::BodyChanged => write!(f, "BODY_CHANGED"),
            ChangeKind::Moved { from_file } => write!(f, "MOVED(from:{})", from_file),
            ChangeKind::Renamed { old_name } => write!(f, "RENAMED(from:{})", old_name),
        }
    }
}

/// A single symbol-level change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolChange {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub change: ChangeKind,
    /// Callers in the current graph (populated by caller when graph is available).
    pub caller_count: usize,
}

/// Full semantic diff result.
#[derive(Debug, Default)]
pub struct SymbolDiff {
    pub old_ref: String,
    pub new_ref: String,
    pub changes: Vec<SymbolChange>,
}

impl SymbolDiff {
    pub fn added(&self) -> impl Iterator<Item = &SymbolChange> {
        self.changes
            .iter()
            .filter(|c| c.change == ChangeKind::Added)
    }
    pub fn removed(&self) -> impl Iterator<Item = &SymbolChange> {
        self.changes
            .iter()
            .filter(|c| c.change == ChangeKind::Removed)
    }
    pub fn modified(&self) -> impl Iterator<Item = &SymbolChange> {
        self.changes.iter().filter(|c| {
            matches!(
                c.change,
                ChangeKind::BodyChanged
                    | ChangeKind::SignatureChanged
                    | ChangeKind::Moved { .. }
                    | ChangeKind::Renamed { .. }
            )
        })
    }
}

/// A flat symbol record used during diff.
#[derive(Clone)]
pub(crate) struct FlatSym {
    pub(crate) file: String,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) sig_hash: String,
    pub(crate) params: String,
    pub(crate) return_type: String,
}
