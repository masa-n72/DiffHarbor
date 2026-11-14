//! 行差分、差分ブロック、行内のトークン差分。

use std::{
    ops::Range,
    time::{Duration, Instant},
};

use similar::{capture_diff_slices, capture_diff_slices_deadline, Algorithm, DiffOp};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Equal,
    Changed,
    LeftOnly,
    RightOnly,
}

/// 整列した表示上の 1 行。`left` と `right` は各側の行番号（0 始まり）。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub kind: RowKind,
    pub hunk: Option<usize>,
}

/// 連続した差分のまとまり。
#[derive(Clone, Debug)]
pub struct Hunk {
    pub left: Range<usize>,
    pub right: Range<usize>,
    pub first_row: usize,
}

#[derive(Default, Clone, Debug)]
pub struct DiffResult {
    pub rows: Vec<Row>,
    pub hunks: Vec<Hunk>,
}

pub fn diff_lines(left: &[String], right: &[String]) -> DiffResult {
    diff_lines_deadline(left, right, None)
}

/// 制限時間つきの行差分。時間を超えた場合は、最小ではないが正しい差分を返す。
pub fn diff_lines_deadline(left: &[String], right: &[String], limit: Option<Duration>) -> DiffResult {
    let deadline = limit.map(|d| Instant::now() + d);
    let ops = capture_diff_slices_deadline(Algorithm::Myers, left, right, deadline);
    let mut out = DiffResult::default();
    // 未確定の差分ブロック: (左の開始, 左の行数, 右の開始, 右の行数)。
    let mut pending: Option<(usize, usize, usize, usize)> = None;

    fn flush(out: &mut DiffResult, pending: &mut Option<(usize, usize, usize, usize)>) {
        let Some((ls, ll, rs, rl)) = pending.take() else { return };
        let hunk = out.hunks.len();
        out.hunks.push(Hunk { left: ls..ls + ll, right: rs..rs + rl, first_row: out.rows.len() });
        for i in 0..ll.max(rl) {
            let l = (i < ll).then_some(ls + i);
            let r = (i < rl).then_some(rs + i);
            let kind = match (l, r) {
                (Some(_), Some(_)) => RowKind::Changed,
                (Some(_), None) => RowKind::LeftOnly,
                _ => RowKind::RightOnly,
            };
            out.rows.push(Row { left: l, right: r, kind, hunk: Some(hunk) });
        }
    }

    for op in ops {
        match op {
            DiffOp::Equal { old_index, new_index, len } => {
                flush(&mut out, &mut pending);
                for i in 0..len {
                    out.rows.push(Row {
                        left: Some(old_index + i),
                        right: Some(new_index + i),
                        kind: RowKind::Equal,
                        hunk: None,
                    });
                }
            }
            DiffOp::Delete { old_index, old_len, new_index } => {
                extend(&mut pending, old_index, old_len, new_index, 0)
            }
            DiffOp::Insert { old_index, new_index, new_len } => {
                extend(&mut pending, old_index, 0, new_index, new_len)
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                extend(&mut pending, old_index, old_len, new_index, new_len)
            }
        }
    }
    flush(&mut out, &mut pending);
    out
}

fn extend(
    pending: &mut Option<(usize, usize, usize, usize)>,
    ls: usize,
    ll: usize,
    rs: usize,
    rl: usize,
) {
    match pending {
        Some((_, pll, _, prl)) => {
            *pll += ll;
            *prl += rl;
        }
        None => *pending = Some((ls, ll, rs, rl)),
    }
}

/// `dst[dst_range]` を `src[src_range]` で置き換える。
pub fn apply_hunk(dst: &mut Vec<String>, dst_range: Range<usize>, src: &[String], src_range: Range<usize>) {
    dst.splice(dst_range, src[src_range].iter().cloned());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Word,
    Space,
    Other,
}

/// 行をトークンに分ける。英数字と `_` の連続は 1 語、空白の連続も 1 つ、
/// それ以外（日本語や記号）は 1 文字ずつ。戻り値はバイト範囲。
fn tokenize(s: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut prev = TokenClass::Other;
    for (i, c) in s.char_indices() {
        let class = if c.is_ascii_alphanumeric() || c == '_' {
            TokenClass::Word
        } else if c.is_whitespace() {
            TokenClass::Space
        } else {
            TokenClass::Other
        };
        let end = i + c.len_utf8();
        match out.last_mut() {
            Some(last) if class == prev && class != TokenClass::Other => last.end = end,
            _ => out.push(i..end),
        }
        prev = class;
    }
    out
}

/// 変更行の中で実際に変わった範囲を返す。戻り値は (左, 右) のバイト範囲で、
/// 文字の境界に揃っている。隣り合う範囲は 1 つにまとめる。
pub fn token_spans(left: &str, right: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let (lt, rt) = (tokenize(left), tokenize(right));
    let ls: Vec<&str> = lt.iter().map(|r| &left[r.clone()]).collect();
    let rs: Vec<&str> = rt.iter().map(|r| &right[r.clone()]).collect();

    fn push(out: &mut Vec<Range<usize>>, tokens: &[Range<usize>], start: usize, len: usize) {
        if len == 0 {
            return;
        }
        let range = tokens[start].start..tokens[start + len - 1].end;
        match out.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            _ => out.push(range),
        }
    }

    let (mut lo, mut ro) = (Vec::new(), Vec::new());
    for op in capture_diff_slices(Algorithm::Myers, &ls, &rs) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_index, old_len, .. } => push(&mut lo, &lt, old_index, old_len),
            DiffOp::Insert { new_index, new_len, .. } => push(&mut ro, &rt, new_index, new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                push(&mut lo, &lt, old_index, old_len);
                push(&mut ro, &rt, new_index, new_len);
            }
        }
    }
    (lo, ro)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn identical_has_no_hunks() {
        let d = diff_lines(&v("a\nb\nc"), &v("a\nb\nc"));
        assert!(d.hunks.is_empty());
        assert_eq!(d.rows.len(), 3);
    }

    #[test]
    fn replace_insert_delete_are_aligned() {
        let l = v("a\nb\nc\nd");
        let r = v("a\nX\nY\nc");
        let d = diff_lines(&l, &r);
        assert_eq!(d.hunks.len(), 2);
        assert_eq!(d.hunks[0].left, 1..2);
        assert_eq!(d.hunks[0].right, 1..3);
        assert_eq!(d.rows.iter().filter(|r| r.kind == RowKind::RightOnly).count(), 1);
        assert_eq!(d.rows.iter().filter(|r| r.kind == RowKind::LeftOnly).count(), 1);
    }

    #[test]
    fn merging_every_hunk_makes_sides_equal() {
        let l = v("1\n2\n3\n4\n5");
        let mut r = v("1\nB\n3\n5\n6\n7");
        loop {
            let d = diff_lines(&l, &r);
            let Some(h) = d.hunks.first().cloned() else { break };
            apply_hunk(&mut r, h.right, &l, h.left);
        }
        assert_eq!(l, r);
    }

    #[test]
    fn deadline_still_yields_a_valid_diff() {
        // 全行が異なる大きな入力。制限時間を 0 にしても、マージすれば一致する差分が返る。
        let l: Vec<String> = (0..4000).map(|i| format!("left {i}")).collect();
        let mut r: Vec<String> = (0..4000).map(|i| format!("right {i}")).collect();
        let d = diff_lines_deadline(&l, &r, Some(Duration::ZERO));
        for h in d.hunks.iter().rev() {
            apply_hunk(&mut r, h.right.clone(), &l, h.left.clone());
        }
        assert_eq!(l, r);
    }

    #[test]
    fn rows_cover_every_line_once_in_order() {
        let l = v("a\nb\nc\nd\ne");
        let r = v("b\nc\nX\ne\nf\ng");
        let d = diff_lines(&l, &r);
        let ls: Vec<usize> = d.rows.iter().filter_map(|r| r.left).collect();
        let rs: Vec<usize> = d.rows.iter().filter_map(|r| r.right).collect();
        assert_eq!(ls, (0..l.len()).collect::<Vec<_>>());
        assert_eq!(rs, (0..r.len()).collect::<Vec<_>>());
        for (i, h) in d.hunks.iter().enumerate() {
            assert_eq!(d.rows[h.first_row].hunk, Some(i));
        }
    }

    fn spans<'a>(s: &'a str, r: &[Range<usize>]) -> Vec<&'a str> {
        r.iter().map(|r| &s[r.clone()]).collect()
    }

    #[test]
    fn token_spans_mark_changed_words() {
        let (l, r) = ("let count = 1;", "let total = 1;");
        let (ls, rs) = token_spans(l, r);
        assert_eq!(spans(l, &ls), ["count"]);
        assert_eq!(spans(r, &rs), ["total"]);
    }

    #[test]
    fn token_spans_handle_insertions_and_identical_lines() {
        let (l, r) = ("foo(a)", "foo(a, b)");
        let (ls, rs) = token_spans(l, r);
        assert!(ls.is_empty());
        assert_eq!(spans(r, &rs), [", b"]);
        assert_eq!(token_spans("same", "same"), (vec![], vec![]));
    }

    #[test]
    fn token_spans_split_japanese_per_character() {
        let (l, r) = ("今日は晴れです", "今日は雨です");
        let (ls, rs) = token_spans(l, r);
        assert_eq!(spans(l, &ls), ["晴れ"]);
        assert_eq!(spans(r, &rs), ["雨"]);
    }
}
