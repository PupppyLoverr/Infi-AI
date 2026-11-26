//! Binary-tree split model for terminal panes.
//!
//! A workspace holds one tree. Leaves are terminal ids. Splits are horizontal
//! (stacked) or vertical (side-by-side). The whole window may host at most
//! [`MAX_PANES`] live terminals across every workspace.

use std::collections::HashSet;

/// Hard cap on terminal panes in one window.
pub const MAX_PANES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDir {
    /// Top / bottom.
    Horizontal,
    /// Left / right.
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaneTree {
    Leaf {
        id: String,
    },
    Split {
        dir: SplitDir,
        /// Stable id so the UI can keep a drag-size signal across rebuilds.
        key: String,
        /// Fraction of space given to `a` (0.15..=0.85); seed for new splits.
        ratio: f32,
        a: Box<PaneTree>,
        b: Box<PaneTree>,
    },
}

impl PaneTree {
    pub fn leaf(id: impl Into<String>) -> Self {
        Self::Leaf { id: id.into() }
    }

    pub fn count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Split { a, b, .. } => a.count() + b.count(),
        }
    }

    pub fn ids(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        self.collect_ids(&mut out);
        out
    }

    fn collect_ids(&self, out: &mut HashSet<String>) {
        match self {
            Self::Leaf { id } => {
                out.insert(id.clone());
            }
            Self::Split { a, b, .. } => {
                a.collect_ids(out);
                b.collect_ids(out);
            }
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        match self {
            Self::Leaf { id: leaf } => leaf == id,
            Self::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    /// Split the leaf `id` in `dir`, inserting `new_id` as the sibling.
    /// `split_key` identifies the new divider for persisted drag sizes.
    /// Returns false if `id` is missing.
    pub fn split(&mut self, id: &str, new_id: String, dir: SplitDir, split_key: String) -> bool {
        match self {
            Self::Leaf { id: leaf } if leaf == id => {
                let old = std::mem::replace(self, Self::leaf("_"));
                *self = Self::Split {
                    dir,
                    key: split_key,
                    ratio: 0.5,
                    a: Box::new(old),
                    b: Box::new(Self::leaf(new_id)),
                };
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { a, b, .. } => {
                a.split(id, new_id.clone(), dir, split_key.clone())
                    || b.split(id, new_id, dir, split_key)
            }
        }
    }

    /// Remove leaf `id`. If this empties the tree, returns `None`.
    pub fn close(self, id: &str) -> Option<Self> {
        match self {
            Self::Leaf { id: leaf } if leaf == id => None,
            Self::Leaf { .. } => Some(self),
            Self::Split {
                dir,
                key,
                ratio,
                a,
                b,
            } => {
                let a = a.close(id);
                let b = b.close(id);
                match (a, b) {
                    (None, None) => None,
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (Some(a), Some(b)) => Some(Self::Split {
                        dir,
                        key,
                        ratio,
                        a: Box::new(a),
                        b: Box::new(b),
                    }),
                }
            }
        }
    }

    /// First leaf id in tree order (for focus fallback).
    pub fn first_id(&self) -> Option<&str> {
        match self {
            Self::Leaf { id } => Some(id),
            Self::Split { a, .. } => a.first_id(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub name: String,
    pub tree: PaneTree,
    pub focused: String,
}

impl Workspace {
    pub fn new(name: impl Into<String>, first_id: String) -> Self {
        Self {
            name: name.into(),
            focused: first_id.clone(),
            tree: PaneTree::leaf(first_id),
        }
    }
}

/// All workspaces in the window; enforces [`MAX_PANES`].
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub workspaces: Vec<Workspace>,
    pub active: usize,
}

impl Session {
    pub fn pane_count(&self) -> usize {
        self.workspaces.iter().map(|w| w.tree.count()).sum()
    }

    pub fn can_add_pane(&self) -> bool {
        self.pane_count() < MAX_PANES
    }

    pub fn active_mut(&mut self) -> Option<&mut Workspace> {
        self.workspaces.get_mut(self.active)
    }

    pub fn active(&self) -> Option<&Workspace> {
        self.workspaces.get(self.active)
    }

    /// New workspace with one pane. No-op if at cap.
    pub fn add_workspace(&mut self, name: String, id: String) -> bool {
        if !self.can_add_pane() {
            return false;
        }
        self.workspaces.push(Workspace::new(name, id));
        self.active = self.workspaces.len() - 1;
        true
    }

    pub fn split_focused(&mut self, new_id: String, dir: SplitDir, split_key: String) -> bool {
        if !self.can_add_pane() {
            return false;
        }
        let Some(ws) = self.active_mut() else {
            return false;
        };
        let focus = ws.focused.clone();
        if ws.tree.split(&focus, new_id.clone(), dir, split_key) {
            ws.focused = new_id;
            true
        } else {
            false
        }
    }

    /// Close the focused pane. An empty workspace is dropped. Returns the id
    /// that closed. The session can be left with no workspaces; the window
    /// opens a fresh one (`add_workspace`) so a terminal is always on screen.
    pub fn close_focused(&mut self) -> Option<String> {
        let idx = self.active;
        let ws = self.workspaces.get_mut(idx)?;
        let closed = ws.focused.clone();
        let tree = std::mem::replace(&mut ws.tree, PaneTree::leaf("_"));
        let beside = sibling_id(&tree, &closed).map(str::to_string);
        match tree.close(&closed) {
            None => {
                self.workspaces.remove(idx);
                if self.workspaces.is_empty() {
                    self.active = 0;
                } else if self.active >= self.workspaces.len() {
                    self.active = self.workspaces.len() - 1;
                }
            }
            Some(tree) => {
                ws.focused = beside
                    .filter(|id| tree.contains(id))
                    .unwrap_or_else(|| tree.first_id().unwrap_or("").to_string());
                ws.tree = tree;
            }
        }
        Some(closed)
    }

    pub fn next_workspace(&mut self) {
        if self.workspaces.is_empty() {
            return;
        }
        self.active = (self.active + 1) % self.workspaces.len();
    }

    pub fn prev_workspace(&mut self) {
        if self.workspaces.is_empty() {
            return;
        }
        self.active = if self.active == 0 {
            self.workspaces.len() - 1
        } else {
            self.active - 1
        };
    }
}

/// The leaf that should take focus when `target` closes: the sibling beside
/// it, or the first leaf of that sibling when the closed pane was a branch.
fn sibling_id<'a>(tree: &'a PaneTree, target: &str) -> Option<&'a str> {
    match tree {
        PaneTree::Leaf { .. } => None,
        PaneTree::Split { a, b, .. } => {
            if a.contains(target) {
                if a.count() == 1 {
                    b.first_id()
                } else {
                    sibling_id(a, target)
                }
            } else if b.contains(target) {
                if b.count() == 1 {
                    a.first_id()
                } else {
                    sibling_id(b, target)
                }
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_count_respects_cap() {
        let mut s = Session::default();
        assert!(s.add_workspace("1".into(), "a".into()));
        for i in 1..MAX_PANES {
            assert!(
                s.split_focused(format!("p{i}"), SplitDir::Vertical, format!("sk{i}")),
                "split {i}"
            );
        }
        assert_eq!(s.pane_count(), MAX_PANES);
        assert!(!s.split_focused("overflow".into(), SplitDir::Horizontal, "skx".into()));
        assert!(!s.add_workspace("x".into(), "y".into()));
    }

    #[test]
    fn close_collapses_split() {
        let mut s = Session::default();
        s.add_workspace("1".into(), "a".into());
        s.split_focused("b".into(), SplitDir::Horizontal, "sk1".into());
        assert_eq!(s.pane_count(), 2);
        let closed = s.close_focused();
        assert_eq!(closed.as_deref(), Some("b"));
        assert_eq!(s.pane_count(), 1);
        assert!(s.active().unwrap().tree.contains("a"));
    }

    #[test]
    fn closing_last_pane_drops_workspace() {
        let mut s = Session::default();
        s.add_workspace("1".into(), "a".into());
        s.add_workspace("2".into(), "b".into());
        assert_eq!(s.workspaces.len(), 2);
        s.close_focused();
        assert_eq!(s.workspaces.len(), 1);
        assert_eq!(s.active().unwrap().focused, "a");
    }

    #[test]
    fn count_leaves_in_a_nested_tree() {
        let mut tree = PaneTree::leaf("a");
        assert_eq!(tree.count(), 1);
        assert!(tree.split("a", "b".into(), SplitDir::Vertical, "s1".into()));
        assert!(tree.split("b", "c".into(), SplitDir::Horizontal, "s2".into()));
        assert!(!tree.split("missing", "d".into(), SplitDir::Vertical, "s3".into()));
        assert_eq!(tree.count(), 3);
        assert_eq!(tree.ids().len(), 3);
        match &tree {
            PaneTree::Split { dir, ratio, .. } => {
                assert_eq!(*dir, SplitDir::Vertical);
                assert_eq!(*ratio, 0.5);
            }
            PaneTree::Leaf { .. } => panic!("expected a split"),
        }
    }

    #[test]
    fn closing_a_nested_pane_focuses_its_sibling() {
        let mut session = Session::default();
        session.add_workspace("1".into(), "a".into());
        assert!(session.split_focused("b".into(), SplitDir::Vertical, "s1".into()));
        assert!(session.split_focused("c".into(), SplitDir::Horizontal, "s2".into()));
        assert_eq!(session.pane_count(), 3);
        assert_eq!(session.close_focused().as_deref(), Some("c"));
        assert_eq!(session.active().unwrap().focused, "b");
        assert_eq!(session.pane_count(), 2);
        assert!(session.active().unwrap().tree.contains("a"));
        assert!(session.active().unwrap().tree.contains("b"));
    }

    #[test]
    fn the_cap_counts_every_workspace() {
        let mut session = Session::default();
        session.add_workspace("1".into(), "a".into());
        for i in 0..8 {
            assert!(session.split_focused(format!("p{i}"), SplitDir::Vertical, format!("s{i}")));
        }
        assert!(session.add_workspace("2".into(), "b".into()));
        let mut n = 0;
        while session.can_add_pane() {
            assert!(session.split_focused(format!("q{n}"), SplitDir::Horizontal, format!("t{n}")));
            n += 1;
            assert!(n < MAX_PANES, "cap did not stop the loop");
        }
        assert_eq!(session.pane_count(), MAX_PANES);
        assert!(!session.split_focused("no".into(), SplitDir::Vertical, "sx".into()));
        assert!(!session.add_workspace("3".into(), "z".into()));
    }

    #[test]
    fn workspace_cycle_wraps() {
        let mut session = Session::default();
        session.add_workspace("1".into(), "a".into());
        session.add_workspace("2".into(), "b".into());
        assert_eq!(session.active().unwrap().name, "2");
        session.next_workspace();
        assert_eq!(session.active().unwrap().name, "1");
        session.prev_workspace();
        assert_eq!(session.active().unwrap().name, "2");
        session.prev_workspace();
        assert_eq!(session.active().unwrap().name, "1");
    }

    #[test]
    fn closing_the_only_pane_leaves_room_for_a_fresh_workspace() {
        let mut session = Session::default();
        session.add_workspace("1".into(), "a".into());
        assert_eq!(session.close_focused().as_deref(), Some("a"));
        assert!(session.workspaces.is_empty());
        assert_eq!(session.pane_count(), 0);
        assert!(session.add_workspace("2".into(), "c".into()));
        assert_eq!(session.pane_count(), 1);
        assert_eq!(session.active().unwrap().focused, "c");
    }
}
