//! INI ファイルを [セクション] とキーで対応づけて比べる。キーごとのコピーの編集も作る。
//!
//! 行そのものは呼び出し側が持つ。ここでは行を読んで比べ、行の置き換え（`LineEdit`）を返すだけ。
//! セクション名とキー名は大文字小文字を区別しない。値は前後の空白と両端の引用符を除いて比べる。

use std::{collections::HashMap, ops::Range, path::Path};

use crate::merge::{LineEdit, Side};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IniStatus {
    Same,
    Changed,
    LeftOnly,
    RightOnly,
}

/// 片側の値。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniCell {
    /// キーの行、またはセクション見出しの行。（セクションなし）の行は None。
    pub line: Option<usize>,
    /// 表示する値（書かれたまま。前後の空白は除く）。セクションの行は空。
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniRow {
    /// セクション名（左の綴り）。（セクションなし）は空。
    pub section: String,
    /// キー名（左の綴り）。セクションの行は None。
    pub key: Option<String>,
    pub left: Option<IniCell>,
    pub right: Option<IniCell>,
    pub status: IniStatus,
}

impl IniRow {
    pub fn is_section(&self) -> bool {
        self.key.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IniDiff {
    pub rows: Vec<IniRow>,
}

impl IniDiff {
    /// 差異のある行の番号。
    pub fn diff_rows(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|&i| self.rows[i].status != IniStatus::Same).collect()
    }
}

/// 拡張子が .ini か（大文字小文字を区別しない）。
pub fn is_ini_path(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini"))
}

// ---- 読み取り ----

#[derive(Clone, Debug)]
struct Key {
    name: String,
    /// 大文字小文字をそろえた名前と、同じ名前の何番目か。対応づけに使う。
    id: (String, usize),
    line: usize,
    /// 行の中の値の範囲（前後の空白を除く）。
    value: Range<usize>,
    /// 比べるための値。
    norm: String,
}

#[derive(Clone, Debug)]
struct Section {
    name: String,
    folded: String,
    /// 見出しの行。（セクションなし）は空。
    headers: Vec<usize>,
    /// 見出しから次の見出しの前までの範囲。（セクションなし）は空。
    blocks: Vec<Range<usize>>,
    keys: Vec<Key>,
}

#[derive(Debug)]
struct Doc {
    sections: Vec<Section>,
}

impl Doc {
    fn find(&self, folded: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.folded == folded)
    }
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// 値を比べる形にする。前後の空白を除き、両端が同じ引用符なら外す。
fn normalize(v: &str) -> String {
    let v = v.trim();
    let b = v.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

enum LineKind {
    Section(String),
    Key { name: String, value: Range<usize> },
    Other,
}

fn classify(line: &str) -> LineKind {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
        return LineKind::Other;
    }
    if let Some(rest) = t.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return LineKind::Section(rest[..end].trim().to_string());
        }
    }
    if let Some(eq) = line.find('=') {
        let name = line[..eq].trim().to_string();
        let after = &line[eq + 1..];
        let lead = after.len() - after.trim_start().len();
        let start = eq + 1 + lead;
        let end = eq + 1 + after.trim_end().len();
        return LineKind::Key { name, value: start..end.max(start) };
    }
    LineKind::Other
}

fn parse(lines: &[String]) -> Doc {
    let mut sections = vec![Section {
        name: String::new(),
        folded: String::new(),
        headers: Vec::new(),
        blocks: Vec::new(),
        keys: Vec::new(),
    }];
    let mut cur = 0usize;
    // セクションごとの、キー名ごとの出現数。
    let mut counts: Vec<HashMap<String, usize>> = vec![HashMap::new()];
    for (i, line) in lines.iter().enumerate() {
        match classify(line) {
            LineKind::Section(name) => {
                // 前のセクションの範囲を閉じる。
                if let Some(b) = sections[cur].blocks.last_mut() {
                    if b.end == usize::MAX {
                        b.end = i;
                    }
                }
                let folded = fold(&name);
                cur = match sections.iter().position(|s| s.folded == folded && !s.headers.is_empty()) {
                    Some(k) => k,
                    None => {
                        sections.push(Section {
                            name,
                            folded,
                            headers: Vec::new(),
                            blocks: Vec::new(),
                            keys: Vec::new(),
                        });
                        counts.push(HashMap::new());
                        sections.len() - 1
                    }
                };
                sections[cur].headers.push(i);
                sections[cur].blocks.push(i..usize::MAX);
            }
            LineKind::Key { name, value } => {
                let f = fold(&name);
                let n = counts[cur].entry(f.clone()).or_insert(0);
                let id = (f, *n);
                *n += 1;
                let norm = normalize(&line[value.clone()]);
                sections[cur].keys.push(Key { name, id, line: i, value, norm });
            }
            LineKind::Other => {}
        }
    }
    if let Some(b) = sections[cur].blocks.last_mut() {
        if b.end == usize::MAX {
            b.end = lines.len();
        }
    }
    // （セクションなし）にキーがなければ外す。
    if sections[0].keys.is_empty() {
        sections.remove(0);
    }
    Doc { sections }
}

// ---- 比較 ----

/// 左の並びに、右だけにある項目を、右での直前の項目の後に入れる。
/// 戻り値は (左の番号, 右の番号) の組の列。
fn merge_order(left: &[String], right: &[String]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out: Vec<(Option<usize>, Option<usize>)> = left.iter().enumerate().map(|(i, _)| (Some(i), None)).collect();
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    for (i, id) in left.iter().enumerate() {
        by_id.entry(id.as_str()).or_insert(i);
    }
    // 直前に置いた右の項目の、out での位置。
    let mut after: Option<usize> = None;
    for (j, id) in right.iter().enumerate() {
        match by_id.get(id.as_str()).and_then(|&i| out.iter().position(|p| p.0 == Some(i) && p.1.is_none())) {
            Some(pos) => {
                out[pos].1 = Some(j);
                after = Some(pos);
            }
            None => {
                let pos = after.map_or(0, |a| a + 1);
                out.insert(pos, (None, Some(j)));
                after = Some(pos);
            }
        }
    }
    out
}

fn key_id(k: &Key) -> String {
    format!("{}\u{0}{}", k.id.0, k.id.1)
}

/// 行の組を比べる。
pub fn compare_ini(left: &[String], right: &[String]) -> IniDiff {
    let (l, r) = (parse(left), parse(right));
    compare_docs(&l, &r, left, right)
}

fn cell(lines: &[String], line: usize, value: &Range<usize>) -> IniCell {
    IniCell { line: Some(line), text: lines[line][value.clone()].to_string() }
}

fn compare_docs(l: &Doc, r: &Doc, left: &[String], right: &[String]) -> IniDiff {
    let lnames: Vec<String> = l.sections.iter().map(|s| s.folded.clone()).collect();
    let rnames: Vec<String> = r.sections.iter().map(|s| s.folded.clone()).collect();
    let mut rows = Vec::new();
    for (li, ri) in merge_order(&lnames, &rnames) {
        let ls = li.map(|i| &l.sections[i]);
        let rs = ri.map(|i| &r.sections[i]);
        let name = ls.or(rs).map(|s| s.name.clone()).unwrap_or_default();
        let head = |s: &Section| IniCell { line: s.headers.first().copied(), text: String::new() };
        let section_ix = rows.len();
        rows.push(IniRow {
            section: name.clone(),
            key: None,
            left: ls.map(head),
            right: rs.map(head),
            status: match (ls, rs) {
                (Some(_), None) => IniStatus::LeftOnly,
                (None, Some(_)) => IniStatus::RightOnly,
                _ => IniStatus::Same,
            },
        });
        let lkeys: Vec<String> = ls.map(|s| s.keys.iter().map(key_id).collect()).unwrap_or_default();
        let rkeys: Vec<String> = rs.map(|s| s.keys.iter().map(key_id).collect()).unwrap_or_default();
        let mut all_same = true;
        for (a, b) in merge_order(&lkeys, &rkeys) {
            let lk = a.map(|i| &ls.unwrap().keys[i]);
            let rk = b.map(|i| &rs.unwrap().keys[i]);
            let status = match (lk, rk) {
                (Some(x), Some(y)) if x.norm == y.norm => IniStatus::Same,
                (Some(_), Some(_)) => IniStatus::Changed,
                (Some(_), None) => IniStatus::LeftOnly,
                _ => IniStatus::RightOnly,
            };
            if status != IniStatus::Same {
                all_same = false;
            }
            rows.push(IniRow {
                section: name.clone(),
                key: lk.or(rk).map(|k| k.name.clone()),
                left: lk.map(|k| cell(left, k.line, &k.value)),
                right: rk.map(|k| cell(right, k.line, &k.value)),
                status,
            });
        }
        if ls.is_some() && rs.is_some() && !all_same {
            rows[section_ix].status = IniStatus::Changed;
        }
    }
    IniDiff { rows }
}

// ---- コピー ----

/// 1 つの操作の単位。
#[derive(Clone, PartialEq, Eq, Debug)]
enum Op {
    /// セクションを丸ごとコピー（コピー先になければ入れ、コピー元になければ消す）。
    Section(String),
    /// 両側にあるセクションの中のキー。
    Key { section: String, id: String },
}

/// `rows` の行（compare_ini の結果の番号）を `to` の側へコピーする編集を作る。
/// 編集は `to` の側の行に対するもの。差異のない行は無視する。
pub fn copy_edits(left: &[String], right: &[String], rows: &[usize], to: Side) -> Vec<LineEdit> {
    let (l, r) = (parse(left), parse(right));
    let diff = compare_docs(&l, &r, left, right);
    let (src, dst, src_lines, dst_lines) = match to {
        Side::Right => (&l, &r, left, right),
        Side::Left => (&r, &l, right, left),
    };

    // 行を操作に直す。片側だけのセクションの中のキーは、セクションごとの操作にする。
    let mut ops: Vec<Op> = Vec::new();
    let push = |ops: &mut Vec<Op>, op: Op| {
        if !ops.contains(&op) {
            ops.push(op);
        }
    };
    for &ri in rows {
        let Some(row) = diff.rows.get(ri) else { continue };
        if row.status == IniStatus::Same {
            continue;
        }
        let folded = fold(&row.section);
        let both = src.find(&folded).is_some() && dst.find(&folded).is_some();
        if !both {
            push(&mut ops, Op::Section(folded));
            continue;
        }
        if row.is_section() {
            // 両側にあるセクション: 差異のあるキーをすべて。
            for k in diff.rows.iter().filter(|x| !x.is_section() && fold(&x.section) == folded && x.status != IniStatus::Same) {
                if let Some(id) = row_key_id(&l, &r, k) {
                    push(&mut ops, Op::Key { section: folded.clone(), id });
                }
            }
        } else if let Some(id) = row_key_id(&l, &r, row) {
            push(&mut ops, Op::Key { section: folded, id });
        }
    }
    // セクションごと動かすなら、その中のキーの操作は要らない。
    let whole: Vec<String> = ops.iter().filter_map(|o| if let Op::Section(s) = o { Some(s.clone()) } else { None }).collect();
    ops.retain(|o| !matches!(o, Op::Key { section, .. } if whole.contains(section)));

    let mut edits: Vec<LineEdit> = Vec::new();
    // 末尾に足すセクション。最後にまとめて 1 つの挿入にする。
    let mut append: Vec<Vec<String>> = Vec::new();
    for op in &ops {
        match op {
            Op::Section(folded) => {
                match (src.find(folded), dst.find(folded)) {
                    (Some(si), None) => {
                        let s = &src.sections[si];
                        if s.headers.is_empty() {
                            // （セクションなし）のキーは先頭に入れる。
                            let new = s.keys.iter().map(|k| src_lines[k.line].clone()).collect();
                            edits.push(LineEdit { start: 0, end: 0, new });
                        } else {
                            for b in &s.blocks {
                                let mut block: Vec<String> = src_lines[b.clone()].to_vec();
                                while block.last().is_some_and(|x| x.trim().is_empty()) {
                                    block.pop();
                                }
                                append.push(block);
                            }
                        }
                    }
                    (None, Some(di)) => {
                        let d = &dst.sections[di];
                        if d.headers.is_empty() {
                            for k in &d.keys {
                                edits.push(LineEdit { start: k.line, end: k.line + 1, new: vec![] });
                            }
                        } else {
                            for b in &d.blocks {
                                edits.push(LineEdit { start: b.start, end: b.end, new: vec![] });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Op::Key { section, id } => {
                let (Some(si), Some(di)) = (src.find(section), dst.find(section)) else { continue };
                let (ss, ds) = (&src.sections[si], &dst.sections[di]);
                let sk = ss.keys.iter().find(|k| &key_id(k) == id);
                let dk = ds.keys.iter().find(|k| &key_id(k) == id);
                match (sk, dk) {
                    (Some(s), Some(d)) => {
                        let line = &dst_lines[d.line];
                        let new = format!("{}{}{}", &line[..d.value.start], &src_lines[s.line][s.value.clone()], &line[d.value.end..]);
                        edits.push(LineEdit { start: d.line, end: d.line + 1, new: vec![new] });
                    }
                    (Some(s), None) => {
                        let at = ds
                            .keys
                            .last()
                            .map(|k| k.line + 1)
                            .or_else(|| ds.headers.last().map(|h| h + 1))
                            .unwrap_or(0);
                        edits.push(LineEdit { start: at, end: at, new: vec![src_lines[s.line].clone()] });
                    }
                    (None, Some(d)) => edits.push(LineEdit { start: d.line, end: d.line + 1, new: vec![] }),
                    (None, None) => {}
                }
            }
        }
    }
    if !append.is_empty() {
        // 消した後に残る最後の行が空行でなければ、空行を 1 行あける。セクションの間も 1 行あける。
        let removed = |i: usize| edits.iter().any(|e| e.start <= i && i < e.end && e.new.is_empty());
        let last_kept = (0..dst_lines.len()).rev().find(|&i| !removed(i));
        let mut new = Vec::new();
        if last_kept.is_some_and(|i| !dst_lines[i].trim().is_empty()) {
            new.push(String::new());
        }
        for (k, block) in append.into_iter().enumerate() {
            if k > 0 {
                new.push(String::new());
            }
            new.extend(block);
        }
        let n = dst_lines.len();
        edits.push(LineEdit { start: n, end: n, new });
    }
    normalize_edits(edits)
}

/// 行のキーの対応づけの鍵。
fn row_key_id(l: &Doc, r: &Doc, row: &IniRow) -> Option<String> {
    let folded = fold(&row.section);
    let find = |doc: &Doc, cell: &Option<IniCell>| -> Option<String> {
        let line = cell.as_ref()?.line?;
        let s = &doc.sections[doc.find(&folded)?];
        s.keys.iter().find(|k| k.line == line).map(key_id)
    };
    find(l, &row.left).or_else(|| find(r, &row.right))
}

/// 編集を位置の順に並べ、同じ位置への挿入は 1 つにまとめる。重なる編集は後のものを捨てる。
fn normalize_edits(mut edits: Vec<LineEdit>) -> Vec<LineEdit> {
    // 同じ位置では、挿入（start == end）を置き換えや削除より先にする。安定ソートで、入れた順を保つ。
    edits.sort_by_key(|e| (e.start, e.end != e.start));
    let mut out: Vec<LineEdit> = Vec::new();
    for e in edits {
        if let Some(last) = out.last_mut() {
            if last.start == last.end && e.start == e.end && last.start == e.start {
                last.new.extend(e.new);
                continue;
            }
            if e.start < last.end || (e.start == last.start && e.start != e.end && last.start != last.end) {
                continue;
            }
        }
        out.push(e);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::History;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    fn row<'a>(d: &'a IniDiff, section: &str, key: Option<&str>) -> (usize, &'a IniRow) {
        d.rows
            .iter()
            .enumerate()
            .find(|(_, r)| fold(&r.section) == fold(section) && r.key.as_deref().map(fold) == key.map(fold))
            .unwrap_or_else(|| panic!("{section} {key:?} がない"))
    }

    /// `rows` を `to` へコピーした結果。
    fn copy(left: &str, right: &str, pick: &[(&str, Option<&str>)], to: Side) -> (Vec<String>, Vec<String>) {
        let (mut l, mut r) = (lines(left), lines(right));
        let d = compare_ini(&l, &r);
        let ix: Vec<usize> = pick.iter().map(|(s, k)| row(&d, s, *k).0).collect();
        let edits = copy_edits(&l, &r, &ix, to);
        let mut h = History::new();
        h.apply_edits(&mut l, &mut r, edits, to);
        (l, r)
    }

    #[test]
    fn detects_ini_paths() {
        assert!(is_ini_path(Path::new("a/setup.INI")));
        assert!(is_ini_path(Path::new("x.ini")));
        assert!(!is_ini_path(Path::new("x.ini.bak")));
        assert!(!is_ini_path(Path::new("ini")));
    }

    #[test]
    fn ignores_order_spacing_comments_case_and_quotes() {
        let l = lines("; comment\n[Main]\nName = Taro\nAge=20\n\n[Sub]\nPath=\"C:\\x\"\n");
        let r = lines("[sub]\n# other\npath = C:\\x\n[MAIN]\nage = 20\nname=Taro  \n");
        let d = compare_ini(&l, &r);
        assert!(d.rows.iter().all(|x| x.status == IniStatus::Same), "{d:#?}");
        // 並びは左の順、表示は左の綴り。
        assert_eq!(d.rows[0].section, "Main");
        assert_eq!(d.rows[1].key.as_deref(), Some("Name"));
        assert_eq!(d.rows[1].right.as_ref().unwrap().text, "Taro");
        assert_eq!(d.rows[1].right.as_ref().unwrap().line, Some(5));
    }

    #[test]
    fn classifies_rows() {
        let l = lines("top=1\n[A]\nk=1\nx=same\nonly_l=1\n[L]\na=1\n");
        let r = lines("top=1\n[A]\nk=2\nx=same\nonly_r=1\n[R]\nb=1\n");
        let d = compare_ini(&l, &r);
        assert_eq!(row(&d, "", None).1.status, IniStatus::Same);
        assert_eq!(row(&d, "A", None).1.status, IniStatus::Changed);
        assert_eq!(row(&d, "A", Some("k")).1.status, IniStatus::Changed);
        assert_eq!(row(&d, "A", Some("x")).1.status, IniStatus::Same);
        assert_eq!(row(&d, "A", Some("only_l")).1.status, IniStatus::LeftOnly);
        assert_eq!(row(&d, "A", Some("only_r")).1.status, IniStatus::RightOnly);
        assert_eq!(row(&d, "L", None).1.status, IniStatus::LeftOnly);
        assert_eq!(row(&d, "L", Some("a")).1.status, IniStatus::LeftOnly);
        assert_eq!(row(&d, "R", None).1.status, IniStatus::RightOnly);
        // 右だけの R は、右での直前の A の直後（左だけの L より前）。
        let names: Vec<String> = d.rows.iter().filter(|x| x.is_section()).map(|x| x.section.clone()).collect();
        assert_eq!(names, ["", "A", "R", "L"]);
        // only_r は右での直前の x の後。
        let keys: Vec<String> = d.rows.iter().filter(|x| x.section == "A").filter_map(|x| x.key.clone()).collect();
        assert_eq!(keys, ["k", "x", "only_r", "only_l"]);
        // A、k、only_r、only_l、L、L.a、R、R.b
        assert_eq!(d.diff_rows().len(), 8);
    }

    #[test]
    fn duplicate_keys_pair_in_order_and_duplicate_sections_merge() {
        let l = lines("[A]\nk=1\nk=2\n[B]\nb=1\n[a]\nz=9\n");
        let r = lines("[A]\nk=1\nk=3\nz=9\n[B]\nb=1\n");
        let d = compare_ini(&l, &r);
        let ks: Vec<(String, IniStatus)> =
            d.rows.iter().filter(|x| x.section == "A" && !x.is_section()).map(|x| (x.key.clone().unwrap(), x.status)).collect();
        assert_eq!(ks, [("k".into(), IniStatus::Same), ("k".into(), IniStatus::Changed), ("z".into(), IniStatus::Same)]);
        assert_eq!(d.rows.iter().filter(|x| x.is_section()).count(), 2);
    }

    #[test]
    fn copy_changed_value_keeps_target_spelling_and_spacing() {
        let (l, r) = copy("[A]\nkey=new value\n", "[a]\nKEY  =  old  \n; c\n", &[("A", Some("key"))], Side::Right);
        assert_eq!(r, lines("[a]\nKEY  =  new value  \n; c\n"));
        assert_eq!(l, lines("[A]\nkey=new value\n"));
    }

    #[test]
    fn copy_left_only_key_inserts_after_last_key() {
        let (_, r) = copy("[A]\nx=1\nnew=2\n[B]\n", "[A]\nx=1\n; tail\n\n[B]\n", &[("A", Some("new"))], Side::Right);
        assert_eq!(r, lines("[A]\nx=1\nnew=2\n; tail\n\n[B]\n"));
        // キーのないセクションへは見出しの次。
        let (_, r) = copy("[A]\nnew=2\n", "[A]\n; c\n", &[("A", Some("new"))], Side::Right);
        assert_eq!(r, lines("[A]\nnew=2\n; c\n"));
    }

    #[test]
    fn copy_right_only_key_to_right_removes_it() {
        let (_, r) = copy("[A]\nx=1\n", "[A]\nx=1\ngone=1\n", &[("A", Some("gone"))], Side::Right);
        assert_eq!(r, lines("[A]\nx=1\n"));
    }

    #[test]
    fn copy_sections() {
        // 左だけのセクションは末尾に、空行を 1 行あけて入れる。末尾の空行は持ってこない。
        let (_, r) = copy("[A]\nx=1\n[New]\n; about\nn=1\n\n\n[Z]\n", "[A]\nx=1\n", &[("New", None)], Side::Right);
        assert_eq!(r, lines("[A]\nx=1\n\n[New]\n; about\nn=1\n"));
        // 中のキーを選んでもセクションごと。
        let (_, r) = copy("[A]\nx=1\n[New]\nn=1\n", "[A]\nx=1\n", &[("New", Some("n"))], Side::Right);
        assert_eq!(r, lines("[A]\nx=1\n\n[New]\nn=1\n"));
        // 右だけのセクションは消す（見出しから次の見出しの前まで）。
        let (_, r) = copy("[A]\nx=1\n", "[Old]\no=1\n\n[A]\nx=1\n", &[("Old", None)], Side::Right);
        assert_eq!(r, lines("[A]\nx=1\n"));
        // 両側にあるセクションは、差異のあるキーをすべて。
        let (_, r) = copy("[A]\na=1\nb=2\nc=3\n", "[A]\na=9\nb=2\nd=4\n", &[("A", None)], Side::Right);
        assert_eq!(compare_ini(&lines("[A]\na=1\nb=2\nc=3\n"), &r).diff_rows().len(), 0, "{r:?}");
    }

    #[test]
    fn copy_all_to_left_makes_them_equal_and_is_one_undo() {
        let left = "g=1\n[A]\na=1\nonly_l=1\n[L]\nl=1\n";
        let right = "g=2\n[A]\na=2\nonly_r=1\n[R]\nr=1\n[S]\ns=1\n";
        let (mut l, mut r) = (lines(left), lines(right));
        let d = compare_ini(&l, &r);
        let edits = copy_edits(&l, &r, &d.diff_rows(), Side::Left);
        let mut h = History::new();
        assert!(h.apply_edits(&mut l, &mut r, edits, Side::Left));
        let after = compare_ini(&l, &r);
        assert!(after.diff_rows().is_empty(), "{l:#?}\n{after:#?}");
        assert_eq!(r, lines(right));
        h.undo(&mut l, &mut r);
        assert_eq!(l, lines(left));
    }

    #[test]
    fn appended_section_does_not_double_blank_lines() {
        // 右の末尾のセクションを消し、左のセクションを足す。消した後の最後の行は空行。
        let (_, r) = copy("[A]\nx=1\n\n[L]\nl=1\n", "[A]\nx=1\n\n[R]\nr=1\n", &[("L", None), ("R", None)], Side::Right);
        assert_eq!(r, lines("[A]\nx=1\n\n[L]\nl=1\n"));
        // 2 つ足す時は間を 1 行あける。
        let (_, r) = copy("[A]\n[B]\nb=1\n[C]\nc=1\n", "[A]", &[("B", None), ("C", None)], Side::Right);
        assert_eq!(r, lines("[A]\n\n[B]\nb=1\n\n[C]\nc=1\n"));
    }

    #[test]
    fn global_keys() {
        let (_, r) = copy("g=1\n[A]\n", "[A]\n", &[("", None)], Side::Right);
        assert_eq!(r, lines("g=1\n[A]\n"));
        let (_, r) = copy("[A]\n", "; head\ng=1\n[A]\n", &[("", Some("g"))], Side::Right);
        assert_eq!(r, lines("; head\n[A]\n"));
    }
}
