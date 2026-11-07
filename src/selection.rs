//! Selection of table rows (#17): one cursor row plus a set of selected rows, all identified by
//! [`ConnKey`], so the selection survives refreshes and re-sorting (the position of a row in the
//! table changes, its key does not).
//!
//! The functions get the keys of the rows shown, in table order, and are pure – tested without
//! a UI. Clicks behave like the lists of Windows Explorer: click = only this row, Ctrl = add /
//! remove the row, Shift = range from the anchor, Ctrl+Shift = add the range.

use crate::net::ConnKey;
use std::collections::HashSet;

#[derive(Default, Debug, Clone)]
pub struct Selection {
    /// The row moved by the arrow keys (the last clicked one).
    cursor: Option<ConnKey>,
    /// Where a Shift range starts.
    anchor: Option<ConnKey>,
    set: HashSet<ConnKey>,
}

impl Selection {
    #[cfg(test)]
    pub fn cursor(&self) -> Option<ConnKey> {
        self.cursor
    }

    pub fn contains(&self, key: &ConnKey) -> bool {
        self.set.contains(key)
    }

    pub fn count(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Only this row.
    pub fn select_only(&mut self, key: ConnKey) {
        self.set.clear();
        self.set.insert(key);
        self.cursor = Some(key);
        self.anchor = Some(key);
    }

    /// Ctrl + click: adds or removes the row; it becomes the cursor and the new anchor.
    pub fn toggle(&mut self, key: ConnKey) {
        if !self.set.remove(&key) {
            self.set.insert(key);
        }
        self.cursor = Some(key);
        self.anchor = Some(key);
        if self.set.is_empty() {
            self.clear();
        }
    }

    /// Shift + click / Shift + arrows: the rows between the anchor and position `pos` of `keys`
    /// (the rows shown, in table order); `add` keeps the rows selected before (Ctrl+Shift).
    /// An anchor that is no longer shown starts the range at `pos`.
    pub fn extend_to(&mut self, pos: usize, keys: &[ConnKey], add: bool) {
        let Some(&key) = keys.get(pos) else { return };
        let anchor_pos = self
            .anchor
            .and_then(|a| keys.iter().position(|k| *k == a))
            .unwrap_or(pos);
        if !add {
            self.set.clear();
        }
        let (a, b) = (anchor_pos.min(pos), anchor_pos.max(pos));
        self.set.extend(keys[a..=b].iter().copied());
        if self.anchor.is_none() || anchor_pos == pos {
            self.anchor = Some(keys[anchor_pos]);
        }
        self.cursor = Some(key);
    }

    /// A click on the row at `pos` of `keys` with the modifiers held.
    pub fn click(&mut self, pos: usize, keys: &[ConnKey], ctrl: bool, shift: bool) {
        let Some(&key) = keys.get(pos) else { return };
        if shift {
            self.extend_to(pos, keys, ctrl);
        } else if ctrl {
            self.toggle(key);
        } else {
            self.select_only(key);
        }
    }

    /// Ctrl+A: every row shown; the cursor stays when it is shown, else the first row.
    pub fn select_all(&mut self, keys: &[ConnKey]) {
        self.set = keys.iter().copied().collect();
        if self.cursor.is_none_or(|c| !self.set.contains(&c)) {
            self.cursor = keys.first().copied();
            self.anchor = self.cursor;
        }
    }

    /// Position of the cursor in `keys`, if it is shown.
    pub fn cursor_pos(&self, keys: &[ConnKey]) -> Option<usize> {
        let c = self.cursor?;
        keys.iter().position(|k| *k == c)
    }

    /// Positions of the selected rows that are shown, in table order.
    pub fn positions(&self, keys: &[ConnKey]) -> Vec<usize> {
        keys.iter()
            .enumerate()
            .filter(|(_, k)| self.set.contains(k))
            .map(|(i, _)| i)
            .collect()
    }

    /// Forgets rows that no longer exist (closed rows removed from the tracker).
    pub fn retain(&mut self, exists: impl Fn(&ConnKey) -> bool) {
        self.set.retain(|k| exists(k));
        if self.cursor.is_some_and(|c| !exists(&c)) {
            self.cursor = None;
        }
        if self.anchor.is_some_and(|a| !exists(&a)) {
            self.anchor = None;
        }
        if self.set.is_empty() {
            self.clear();
        }
    }
}

/// Where a navigation key moves the cursor: `n` rows shown, `cur` = the cursor position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

/// Rows moved by PgUp / PgDn.
pub const PAGE: usize = 20;

impl Nav {
    /// The target position, or `None` when no rows are shown. Without a cursor, Up / PgUp / End
    /// start at the end and the other keys at the start.
    pub fn target(self, cur: Option<usize>, n: usize) -> Option<usize> {
        if n == 0 {
            return None;
        }
        let last = n - 1;
        Some(match (self, cur) {
            (Nav::Home, _) => 0,
            (Nav::End, _) => last,
            (Nav::Up, Some(p)) => p.saturating_sub(1),
            (Nav::Down, Some(p)) => (p + 1).min(last),
            (Nav::PageUp, Some(p)) => p.saturating_sub(PAGE),
            (Nav::PageDown, Some(p)) => (p + PAGE).min(last),
            (Nav::Up | Nav::PageUp, None) => last,
            (Nav::Down | Nav::PageDown, None) => 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Protocol;

    fn keys(n: u16) -> Vec<ConnKey> {
        (0..n)
            .map(|i| ConnKey {
                proto: Protocol::Tcp,
                local: format!("10.0.0.1:{}", 1000 + i).parse().unwrap(),
                remote: None,
                pid: 1,
            })
            .collect()
    }

    #[test]
    fn click_ctrl_shift() {
        let k = keys(10);
        let mut s = Selection::default();
        s.click(2, &k, false, false);
        assert_eq!(s.positions(&k), vec![2]);
        s.click(5, &k, false, true);
        assert_eq!(s.positions(&k), vec![2, 3, 4, 5]);
        // Shift again from the same anchor shrinks the range.
        s.click(3, &k, false, true);
        assert_eq!(s.positions(&k), vec![2, 3]);
        // Ctrl adds a row and moves the anchor.
        s.click(8, &k, true, false);
        assert_eq!(s.positions(&k), vec![2, 3, 8]);
        // Ctrl+Shift adds the range from the new anchor.
        s.click(6, &k, true, true);
        assert_eq!(s.positions(&k), vec![2, 3, 6, 7, 8]);
        // Ctrl removes.
        s.click(7, &k, true, false);
        assert_eq!(s.positions(&k), vec![2, 3, 6, 8]);
        assert_eq!(s.cursor(), Some(k[7]));
        // A plain click selects only one row.
        s.click(0, &k, false, false);
        assert_eq!(s.positions(&k), vec![0]);
    }

    #[test]
    fn ctrl_click_on_the_only_row_clears() {
        let k = keys(3);
        let mut s = Selection::default();
        s.click(1, &k, false, false);
        s.click(1, &k, true, false);
        assert!(s.is_empty());
        assert_eq!(s.cursor(), None);
    }

    #[test]
    fn survives_resorting_and_refresh() {
        let k = keys(6);
        let mut s = Selection::default();
        s.click(1, &k, false, false);
        s.click(3, &k, false, true);
        // The table is sorted the other way round: the same rows stay selected.
        let rev: Vec<_> = k.iter().rev().copied().collect();
        assert_eq!(s.positions(&rev), vec![2, 3, 4]);
        assert_eq!(s.cursor_pos(&rev), Some(2));
        // Row 2 was closed and removed from the tracker.
        s.retain(|key| *key != k[2]);
        assert_eq!(s.count(), 2);
        s.retain(|_| false);
        assert!(s.is_empty() && s.cursor().is_none());
    }

    #[test]
    fn select_all_and_hidden_anchor() {
        let k = keys(5);
        let mut s = Selection::default();
        s.select_all(&k);
        assert_eq!(s.count(), 5);
        assert_eq!(s.cursor(), Some(k[0]));
        // The anchor row is filtered out: Shift starts a new range at the clicked row.
        let mut s = Selection::default();
        s.click(0, &k, false, false);
        s.click(3, &k[1..], false, true);
        assert_eq!(s.positions(&k), vec![4]);
    }

    #[test]
    fn navigation_targets() {
        assert_eq!(Nav::Down.target(None, 0), None);
        assert_eq!(Nav::Down.target(None, 5), Some(0));
        assert_eq!(Nav::Up.target(None, 5), Some(4));
        assert_eq!(Nav::Down.target(Some(4), 5), Some(4));
        assert_eq!(Nav::Up.target(Some(0), 5), Some(0));
        assert_eq!(Nav::PageDown.target(Some(1), 100), Some(1 + PAGE));
        assert_eq!(Nav::PageDown.target(Some(90), 100), Some(99));
        assert_eq!(Nav::PageUp.target(Some(5), 100), Some(0));
        assert_eq!(Nav::Home.target(Some(50), 100), Some(0));
        assert_eq!(Nav::End.target(None, 100), Some(99));
    }
}
