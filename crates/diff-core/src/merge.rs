//! マージ操作の履歴。差分ブロックのコピーを記録し、元に戻す、やり直すを行う。
//!
//! 行そのものは呼び出し側が持つ。ここでは編集内容と、各側の「版」を管理する。
//! 版は内容が変わるたびに新しい番号になり、元に戻すと前の番号に戻る。
//! 保存した時点の版と比べることで、未保存の変更があるかを正しく判定できる。

use crate::diff::Hunk;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// 1 か所の置き換え。`at` から `old` を取り除き `new` を入れる。
struct Edit {
    at: usize,
    old: Vec<String>,
    new: Vec<String>,
}

/// 1 回の操作。一括コピーでは複数の置き換えをまとめて持つ。
struct Step {
    side: Side,
    edits: Vec<Edit>,
    rev_before: u64,
    rev_after: u64,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    rev: [u64; 2],
    next_rev: u64,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// `side` の現在の版。
    pub fn rev(&self, side: Side) -> u64 {
        self.rev[side.index()]
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 差分ブロックを `to` の側へコピーする。複数渡した場合も 1 回の操作として記録する。
    /// `hunks` は現在の `left` と `right` に対する差分であること。変更がなければ false。
    pub fn copy_hunks(&mut self, left: &mut Vec<String>, right: &mut Vec<String>, hunks: &[Hunk], to: Side) -> bool {
        let mut edits = Vec::with_capacity(hunks.len());
        // 後ろのブロックから適用すると、前のブロックの行番号がずれない。
        let mut ordered: Vec<&Hunk> = hunks.iter().collect();
        ordered.sort_by_key(|h| std::cmp::Reverse(h.first_row));
        for h in ordered {
            let (src, src_range, dst, dst_range) = match to {
                Side::Right => (&*left, h.left.clone(), &mut *right, h.right.clone()),
                Side::Left => (&*right, h.right.clone(), &mut *left, h.left.clone()),
            };
            let new: Vec<String> = src[src_range].to_vec();
            let at = dst_range.start;
            let old: Vec<String> = dst.splice(dst_range, new.iter().cloned()).collect();
            edits.push(Edit { at, old, new });
        }
        if edits.is_empty() {
            return false;
        }
        self.next_rev += 1;
        let step = Step { side: to, edits, rev_before: self.rev(to), rev_after: self.next_rev };
        self.rev[to.index()] = step.rev_after;
        self.undo.push(step);
        self.redo.clear();
        true
    }

    /// 直前の操作を取り消す。変更した側を返す。
    pub fn undo(&mut self, left: &mut Vec<String>, right: &mut Vec<String>) -> Option<Side> {
        let step = self.undo.pop()?;
        let lines = match step.side {
            Side::Left => left,
            Side::Right => right,
        };
        // 適用した順の逆にたどる。
        for e in step.edits.iter().rev() {
            lines.splice(e.at..e.at + e.new.len(), e.old.iter().cloned());
        }
        self.rev[step.side.index()] = step.rev_before;
        let side = step.side;
        self.redo.push(step);
        Some(side)
    }

    /// 取り消した操作をやり直す。変更した側を返す。
    pub fn redo(&mut self, left: &mut Vec<String>, right: &mut Vec<String>) -> Option<Side> {
        let step = self.redo.pop()?;
        let lines = match step.side {
            Side::Left => left,
            Side::Right => right,
        };
        for e in &step.edits {
            lines.splice(e.at..e.at + e.old.len(), e.new.iter().cloned());
        }
        self.rev[step.side.index()] = step.rev_after;
        let side = step.side;
        self.undo.push(step);
        Some(side)
    }

    /// `side` の履歴を捨てる。ファイルを読み直した時に使う。
    /// もう一方の側の操作も行番号の前提が変わるため、すべての履歴を捨てて版を進める。
    pub fn reset(&mut self, side: Side) {
        self.undo.clear();
        self.redo.clear();
        self.next_rev += 1;
        self.rev[side.index()] = self.next_rev;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff_lines;

    fn v(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn copy_one_hunk_each_direction() {
        let (mut l, mut r) = (v("a b c d"), v("a X c Y Z"));
        let mut h = History::new();
        let d = diff_lines(&l, &r);
        assert_eq!(d.hunks.len(), 2);
        // 1 つ目を右へ。
        assert!(h.copy_hunks(&mut l, &mut r, &d.hunks[..1], Side::Right));
        assert_eq!(r, v("a b c Y Z"));
        // 残りを左へ。
        let d = diff_lines(&l, &r);
        assert!(h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Left));
        assert_eq!(l, v("a b c Y Z"));
        assert_eq!(l, r);
    }

    #[test]
    fn copy_all_is_one_undo_step() {
        let (mut l, mut r) = (v("1 2 3 4 5 6 7"), v("1 B 3 5 6 N 7 8"));
        let (l0, r0) = (l.clone(), r.clone());
        let mut h = History::new();
        let d = diff_lines(&l, &r);
        assert!(d.hunks.len() >= 3);
        h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Right);
        assert_eq!(l, r);
        assert_eq!(l, l0, "コピー元は変わらない");

        assert_eq!(h.undo(&mut l, &mut r), Some(Side::Right));
        assert_eq!((l.clone(), r.clone()), (l0.clone(), r0));
        assert!(!h.can_undo());

        assert_eq!(h.redo(&mut l, &mut r), Some(Side::Right));
        assert_eq!(r, l0);
        assert!(!h.can_redo());
    }

    #[test]
    fn insert_only_and_delete_only_hunks() {
        // 左にしかない行と、右にしかない行。
        let (mut l, mut r) = (v("a only_left b"), v("a b only_right"));
        let mut h = History::new();
        let d = diff_lines(&l, &r);
        h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Left);
        assert_eq!(l, v("a b only_right"));
        h.undo(&mut l, &mut r);
        assert_eq!(l, v("a only_left b"));
    }

    #[test]
    fn revision_tracks_saved_state_through_undo_and_redo() {
        let (mut l, mut r) = (v("a b"), v("a c"));
        let mut h = History::new();
        let saved = h.rev(Side::Right);
        let d = diff_lines(&l, &r);
        h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Right);
        assert_ne!(h.rev(Side::Right), saved, "編集後は未保存");
        assert_eq!(h.rev(Side::Left), 0, "もう一方は変わらない");

        // 編集後に保存したとする。
        let saved = h.rev(Side::Right);
        h.undo(&mut l, &mut r);
        assert_ne!(h.rev(Side::Right), saved, "保存後に元へ戻すと未保存になる");
        h.redo(&mut l, &mut r);
        assert_eq!(h.rev(Side::Right), saved, "やり直すと保存時の版に戻る");
    }

    #[test]
    fn new_edit_clears_redo() {
        let (mut l, mut r) = (v("a b c"), v("a X c"));
        let mut h = History::new();
        let d = diff_lines(&l, &r);
        h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Right);
        h.undo(&mut l, &mut r);
        assert!(h.can_redo());
        let d = diff_lines(&l, &r);
        h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Left);
        assert!(!h.can_redo());
        assert_eq!(l, v("a X c"));
    }

    #[test]
    fn empty_hunk_list_changes_nothing() {
        let (mut l, mut r) = (v("a"), v("a"));
        let mut h = History::new();
        assert!(!h.copy_hunks(&mut l, &mut r, &[], Side::Right));
        assert!(!h.can_undo());
        assert_eq!(h.undo(&mut l, &mut r), None);
    }

    #[test]
    fn reset_drops_history_and_bumps_revision() {
        let (mut l, mut r) = (v("a b"), v("a c"));
        let mut h = History::new();
        let d = diff_lines(&l, &r);
        h.copy_hunks(&mut l, &mut r, &d.hunks, Side::Right);
        let before = h.rev(Side::Left);
        h.reset(Side::Left);
        assert!(!h.can_undo() && !h.can_redo());
        assert_ne!(h.rev(Side::Left), before);
    }
}
