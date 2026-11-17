//! フォルダの再帰比較と、項目のコピー。
//!
//! 名前の大文字小文字は区別しない（Windows と macOS のファイルシステムに合わせる）。
//! そのため、左右で綴りが違うことがあり、各項目は左右それぞれの相対パスを持つ。

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::SystemTime,
};

use anyhow::{Context, Result};

use crate::merge::Side;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Identical,
    Different,
    LeftOnly,
    RightOnly,
    /// 片側がファイルで、片側がフォルダ。
    TypeMismatch,
    /// 読めなかった。理由は `FolderEntry::error`。
    Error,
}

/// 片側の項目の情報。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Meta {
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub struct FolderEntry {
    /// 表示用の相対パス。両側にある場合は左の綴り。
    pub rel: PathBuf,
    /// 左での相対パス。左にない場合は None。
    pub left_rel: Option<PathBuf>,
    /// 右での相対パス。右にない場合は None。
    pub right_rel: Option<PathBuf>,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub status: Status,
    /// 親フォルダの項目の番号。最上位は None。
    pub parent: Option<usize>,
    pub left: Option<Meta>,
    pub right: Option<Meta>,
    /// `status` が `Error` の時の理由。
    pub error: Option<String>,
}

impl FolderEntry {
    pub fn rel_on(&self, side: Side) -> Option<&Path> {
        match side {
            Side::Left => self.left_rel.as_deref(),
            Side::Right => self.right_rel.as_deref(),
        }
    }
}

/// 比較の進み具合と中止の指示。比較するスレッドと画面のスレッドで共有する。
#[derive(Default, Debug)]
pub struct Progress {
    scanned: AtomicUsize,
    cancelled: AtomicBool,
}

impl Progress {
    /// これまでに調べた項目の数。
    pub fn scanned(&self) -> usize {
        self.scanned.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// 比較とコピーの対象から外す名前。
const IGNORED: &[&str] = &[".DS_Store", ".git", "Thumbs.db", "desktop.ini"];

/// 比較とコピーの対象から外すか。macOS が他のファイルシステムに作る `._*`（AppleDouble）も外す。
pub fn is_ignored(name: &str) -> bool {
    IGNORED.iter().any(|i| i.eq_ignore_ascii_case(name)) || name.starts_with("._")
}

/// 中止した時のエラーの文言。
pub const CANCELLED: &str = "中止しました";

/// 内容を比べる時に一度に読む大きさ。
const CHUNK: usize = 64 * 1024;

pub fn compare_dirs(left: &Path, right: &Path) -> Result<Vec<FolderEntry>> {
    compare_dirs_with(left, right, &Progress::default())
}

/// `progress` に進み具合を書きながら比較する。中止されたら `CANCELLED` のエラーを返す。
pub fn compare_dirs_with(left: &Path, right: &Path, progress: &Progress) -> Result<Vec<FolderEntry>> {
    // 起点のフォルダが開けない場合は、比較全体をエラーにする。
    let l = list(left)?;
    let r = list(right)?;
    let mut out = Vec::new();
    walk(left, right, l, r, Path::new(""), Path::new(""), 0, None, progress, &mut out)?;
    Ok(out)
}

/// フォルダ内の 1 項目。
struct Item {
    name: String,
    is_dir: bool,
    meta: Meta,
}

#[derive(Default)]
struct Pair {
    left: Option<Item>,
    right: Option<Item>,
}

fn meta_of(m: &fs::Metadata) -> Meta {
    Meta { size: if m.is_dir() { 0 } else { m.len() }, modified: m.modified().ok() }
}

fn list(dir: &Path) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for e in fs::read_dir(dir).with_context(|| format!("開けません: {}", dir.display()))? {
        let e = e.with_context(|| format!("読めません: {}", dir.display()))?;
        let name = e.file_name().to_string_lossy().into_owned();
        if is_ignored(&name) {
            continue;
        }
        // シンボリックリンクはフォルダとして扱わない（中に入らない）。大きさと日時はリンク先のもの。
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let meta = fs::metadata(e.path()).map(|m| meta_of(&m)).unwrap_or(Meta { size: 0, modified: None });
        items.push(Item { name, is_dir, meta });
    }
    Ok(items)
}

/// 左右の項目を、大文字小文字を区別しない名前で組にする。
/// 同じ側に大文字小文字だけが違う名前が複数あれば、綴りが同じものを先に組にし、残りは名前順に組にする。
fn pair_up(l: Vec<Item>, r: Vec<Item>) -> Vec<Pair> {
    let mut groups: BTreeMap<String, (Vec<Item>, Vec<Item>)> = BTreeMap::new();
    for i in l {
        groups.entry(i.name.to_lowercase()).or_default().0.push(i);
    }
    for i in r {
        groups.entry(i.name.to_lowercase()).or_default().1.push(i);
    }
    let mut pairs = Vec::new();
    for (_, (mut ls, mut rs)) in groups {
        ls.sort_by(|a, b| a.name.cmp(&b.name));
        rs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut rest_l = Vec::new();
        for li in ls {
            match rs.iter().position(|ri| ri.name == li.name) {
                Some(k) => pairs.push(Pair { left: Some(li), right: Some(rs.remove(k)) }),
                None => rest_l.push(li),
            }
        }
        let mut rest_r = rs.into_iter();
        for li in rest_l {
            pairs.push(Pair { left: Some(li), right: rest_r.next() });
        }
        for ri in rest_r {
            pairs.push(Pair { left: None, right: Some(ri) });
        }
    }
    let mut v = pairs;
    // フォルダが先、次に名前順（大文字小文字を区別しない）。
    v.sort_by_key(|p| {
        let is_dir = p.left.as_ref().is_some_and(|i| i.is_dir) || p.right.as_ref().is_some_and(|i| i.is_dir);
        let name = p.left.as_ref().or(p.right.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
        (!is_dir, name.to_lowercase(), name)
    });
    v
}

#[allow(clippy::too_many_arguments)]
fn walk(
    left: &Path,
    right: &Path,
    l: Vec<Item>,
    r: Vec<Item>,
    lrel: &Path,
    rrel: &Path,
    depth: usize,
    parent: Option<usize>,
    progress: &Progress,
    out: &mut Vec<FolderEntry>,
) -> Result<bool> {
    let mut all_identical = true;
    for pair in pair_up(l, r) {
        if progress.is_cancelled() {
            anyhow::bail!("{CANCELLED}");
        }
        let name = pair.left.as_ref().or(pair.right.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
        let left_rel = pair.left.as_ref().map(|i| lrel.join(&i.name));
        let right_rel = pair.right.as_ref().map(|i| rrel.join(&i.name));
        let rel = left_rel.clone().or_else(|| right_rel.clone()).unwrap_or_default();
        let is_dir = pair.left.as_ref().is_some_and(|i| i.is_dir) || pair.right.as_ref().is_some_and(|i| i.is_dir);
        let idx = out.len();
        let mut entry = FolderEntry {
            rel,
            left_rel,
            right_rel,
            name,
            depth,
            is_dir,
            status: Status::Identical,
            parent,
            left: pair.left.as_ref().map(|i| i.meta),
            right: pair.right.as_ref().map(|i| i.meta),
            error: None,
        };
        progress.scanned.fetch_add(1, Ordering::Relaxed);

        match (&pair.left, &pair.right) {
            (Some(li), Some(ri)) if li.is_dir && ri.is_dir => {
                let (lp, rp) = (left.join(entry.left_rel.as_ref().unwrap()), right.join(entry.right_rel.as_ref().unwrap()));
                match (list(&lp), list(&rp)) {
                    (Ok(lc), Ok(rc)) => {
                        let (lr, rr) = (entry.left_rel.clone().unwrap(), entry.right_rel.clone().unwrap());
                        out.push(entry);
                        let same = walk(left, right, lc, rc, &lr, &rr, depth + 1, Some(idx), progress, out)?;
                        out[idx].status = if same { Status::Identical } else { Status::Different };
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        entry.status = Status::Error;
                        entry.error = Some(format!("{e:#}"));
                        out.push(entry);
                    }
                }
            }
            (Some(li), Some(ri)) if li.is_dir != ri.is_dir => {
                entry.status = Status::TypeMismatch;
                out.push(entry);
            }
            (Some(_), Some(_)) => {
                let (lp, rp) = (left.join(entry.left_rel.as_ref().unwrap()), right.join(entry.right_rel.as_ref().unwrap()));
                set_file_status(&mut entry, &lp, &rp);
                out.push(entry);
            }
            (Some(_), None) | (None, Some(_)) => {
                let (side_only, root, side_rel) = if pair.left.is_some() {
                    (Status::LeftOnly, left, entry.left_rel.clone().unwrap())
                } else {
                    (Status::RightOnly, right, entry.right_rel.clone().unwrap())
                };
                entry.status = side_only;
                out.push(entry);
                if is_dir {
                    list_one_sided(root, &side_rel, side_only, depth + 1, idx, progress, out)?;
                }
            }
            (None, None) => unreachable!("組には少なくとも片側がある"),
        }
        if out[idx].status != Status::Identical {
            all_identical = false;
        }
    }
    Ok(all_identical)
}

/// 両側にあるファイルを比べて、状態を決める。
fn set_file_status(entry: &mut FolderEntry, lp: &Path, rp: &Path) {
    match files_equal(lp, rp) {
        Ok(true) => {
            entry.status = Status::Identical;
            entry.error = None;
        }
        Ok(false) => {
            entry.status = Status::Different;
            entry.error = None;
        }
        Err(e) => {
            entry.status = Status::Error;
            entry.error = Some(e);
        }
    }
}

fn list_one_sided(
    root: &Path,
    rel: &Path,
    status: Status,
    depth: usize,
    parent: usize,
    progress: &Progress,
    out: &mut Vec<FolderEntry>,
) -> Result<()> {
    let items = match list(&root.join(rel)) {
        Ok(items) => items,
        Err(e) => {
            out[parent].status = Status::Error;
            out[parent].error = Some(format!("{e:#}"));
            return Ok(());
        }
    };
    let empty = Vec::new();
    let pairs = if status == Status::LeftOnly { pair_up(items, empty) } else { pair_up(empty, items) };
    for pair in pairs {
        if progress.is_cancelled() {
            anyhow::bail!("{CANCELLED}");
        }
        let item = pair.left.or(pair.right).expect("片側の項目");
        let child_rel = rel.join(&item.name);
        let idx = out.len();
        let meta = Some(item.meta);
        let (left_rel, right_rel, left, right) = if status == Status::LeftOnly {
            (Some(child_rel.clone()), None, meta, None)
        } else {
            (None, Some(child_rel.clone()), None, meta)
        };
        out.push(FolderEntry {
            rel: child_rel.clone(),
            left_rel,
            right_rel,
            name: item.name,
            depth,
            is_dir: item.is_dir,
            status,
            parent: Some(parent),
            left,
            right,
            error: None,
        });
        progress.scanned.fetch_add(1, Ordering::Relaxed);
        if item.is_dir {
            list_one_sided(root, &child_rel, status, depth + 1, idx, progress, out)?;
        }
    }
    Ok(())
}

/// 2 つのファイルの内容が同じか。大きさが違えば読まない。読む時は一定の大きさずつ比べる。
/// 読めない場合は理由を返す。
fn files_equal(a: &Path, b: &Path) -> std::result::Result<bool, String> {
    let ma = fs::metadata(a).map_err(|e| format!("読めません: {}: {e}", a.display()))?;
    let mb = fs::metadata(b).map_err(|e| format!("読めません: {}: {e}", b.display()))?;
    if ma.len() != mb.len() {
        return Ok(false);
    }
    let mut fa = fs::File::open(a).map_err(|e| format!("開けません: {}: {e}", a.display()))?;
    let mut fb = fs::File::open(b).map_err(|e| format!("開けません: {}: {e}", b.display()))?;
    let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    loop {
        let na = read_full(&mut fa, &mut ba).map_err(|e| format!("読めません: {}: {e}", a.display()))?;
        let nb = read_full(&mut fb, &mut bb).map_err(|e| format!("読めません: {}: {e}", b.display()))?;
        if na != nb || ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// `buf` が埋まるか、ファイルの終わりまで読む。読んだバイト数を返す。
fn read_full(f: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// 項目 `ix` の、`side` の側でのパス。その側にない場合は、親フォルダのその側での綴りに名前をつなげる
/// （コピー先の計算に使う）。
pub fn side_path(entries: &[FolderEntry], ix: usize, side: Side, root: &Path) -> PathBuf {
    let e = &entries[ix];
    if let Some(rel) = e.rel_on(side) {
        return root.join(rel);
    }
    match e.parent {
        Some(p) => side_path(entries, p, side, root).join(&e.name),
        None => root.join(&e.name),
    }
}

/// 両側にあるファイル `ix` を比べ直し、状態、大きさ、更新日時を更新する。親フォルダの状態も更新する。
/// ファイル比較の画面で保存した後に使う。
pub fn recompare(entries: &mut [FolderEntry], ix: usize, left: &Path, right: &Path) {
    {
        let e = &entries[ix];
        if e.is_dir {
            return;
        }
        let (Some(lr), Some(rr)) = (e.left_rel.clone(), e.right_rel.clone()) else { return };
        let (lp, rp) = (left.join(lr), right.join(rr));
        let e = &mut entries[ix];
        e.left = fs::metadata(&lp).ok().map(|m| meta_of(&m));
        e.right = fs::metadata(&rp).ok().map(|m| meta_of(&m));
        set_file_status(e, &lp, &rp);
    }
    // 両側にある親フォルダの状態を、直下の項目から決め直す。
    let mut cur = entries[ix].parent;
    while let Some(p) = cur {
        if entries[p].left_rel.is_some() && entries[p].right_rel.is_some() && entries[p].error.is_none() {
            let same = entries.iter().filter(|c| c.parent == Some(p)).all(|c| c.status == Status::Identical);
            entries[p].status = if same { Status::Identical } else { Status::Different };
        }
        cur = entries[p].parent;
    }
}

/// `src` を `dst` へコピーする。ファイルは上書きし、フォルダは中身ごとコピーする。
pub fn copy_entry(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        copy_dir(src, dst)
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if dst.is_dir() {
            anyhow::bail!("コピー先に同名のフォルダがあります: {}", dst.display());
        }
        fs::copy(src, dst).with_context(|| format!("コピーできません: {}", src.display()))?;
        Ok(())
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() && !dst.is_dir() {
        anyhow::bail!("コピー先に同名のファイルがあります: {}", dst.display());
    }
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        if is_ignored(&e.file_name().to_string_lossy()) {
            continue;
        }
        let (s, d) = (e.path(), dst.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("diffharbor-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn find<'a>(e: &'a [FolderEntry], p: &str) -> &'a FolderEntry {
        e.iter().find(|x| x.rel == Path::new(p)).unwrap_or_else(|| panic!("{p} がない"))
    }

    #[test]
    fn classifies_entries() {
        let root = tmp("classify");
        let (l, r) = (root.join("l"), root.join("r"));
        for d in [&l, &r] {
            fs::create_dir_all(d.join("sub")).unwrap();
        }
        fs::write(l.join("same.txt"), "x").unwrap();
        fs::write(r.join("same.txt"), "x").unwrap();
        fs::write(l.join("diff.txt"), "a").unwrap();
        fs::write(r.join("diff.txt"), "b").unwrap();
        fs::write(l.join("only_l.txt"), "a").unwrap();
        fs::write(r.join("sub/only_r.txt"), "a").unwrap();

        let e = compare_dirs(&l, &r).unwrap();
        assert_eq!(find(&e, "same.txt").status, Status::Identical);
        assert_eq!(find(&e, "diff.txt").status, Status::Different);
        assert_eq!(find(&e, "only_l.txt").status, Status::LeftOnly);
        assert_eq!(find(&e, "sub/only_r.txt").status, Status::RightOnly);
        assert_eq!(find(&e, "sub").status, Status::Different);
        // フォルダが先。
        assert!(e[0].is_dir);
    }

    #[test]
    fn records_size_time_and_parent() {
        let root = tmp("meta");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("d")).unwrap();
        fs::create_dir_all(r.join("d")).unwrap();
        fs::write(l.join("d/f.txt"), "abc").unwrap();
        fs::write(r.join("d/f.txt"), "abcde").unwrap();
        fs::create_dir_all(l.join("lonly/inner")).unwrap();
        fs::write(l.join("lonly/inner/g.txt"), "g").unwrap();

        let e = compare_dirs(&l, &r).unwrap();
        let f = find(&e, "d/f.txt");
        assert_eq!(f.left.unwrap().size, 3);
        assert_eq!(f.right.unwrap().size, 5);
        assert!(f.left.unwrap().modified.is_some());
        assert_eq!(f.status, Status::Different);
        let d = e.iter().position(|x| x.rel == Path::new("d")).unwrap();
        assert_eq!(f.parent, Some(d));
        assert_eq!(e[d].parent, None);

        let g = find(&e, "lonly/inner/g.txt");
        assert_eq!(g.status, Status::LeftOnly);
        assert!(g.right.is_none() && g.right_rel.is_none());
        let inner = e.iter().position(|x| x.rel == Path::new("lonly/inner")).unwrap();
        assert_eq!(g.parent, Some(inner));
        assert_eq!(g.depth, 2);
    }

    #[test]
    fn compares_large_files_in_chunks() {
        let root = tmp("large");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(&r).unwrap();
        let mut a = vec![7u8; CHUNK * 3 + 123];
        fs::write(l.join("same.bin"), &a).unwrap();
        fs::write(r.join("same.bin"), &a).unwrap();
        fs::write(l.join("diff.bin"), &a).unwrap();
        // 最後のかたまりだけが違う。
        let last = a.len() - 1;
        a[last] = 8;
        fs::write(r.join("diff.bin"), &a).unwrap();

        let e = compare_dirs(&l, &r).unwrap();
        assert_eq!(find(&e, "same.bin").status, Status::Identical);
        assert_eq!(find(&e, "diff.bin").status, Status::Different);
    }

    #[test]
    fn matches_names_ignoring_case() {
        let root = tmp("case");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("Docs")).unwrap();
        fs::create_dir_all(r.join("docs")).unwrap();
        fs::write(l.join("Docs/Read.txt"), "x").unwrap();
        fs::write(r.join("docs/READ.txt"), "x").unwrap();

        let e = compare_dirs(&l, &r).unwrap();
        assert_eq!(e.len(), 2);
        let f = find(&e, "Docs/Read.txt");
        assert_eq!(f.status, Status::Identical);
        assert_eq!(f.left_rel.as_deref(), Some(Path::new("Docs/Read.txt")));
        assert_eq!(f.right_rel.as_deref(), Some(Path::new("docs/READ.txt")));
        assert_eq!(find(&e, "Docs").status, Status::Identical);
    }

    #[test]
    fn same_side_case_duplicates_stay_separate() {
        let root = tmp("casedup");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(&r).unwrap();
        fs::write(l.join("a.txt"), "x").unwrap();
        fs::write(l.join("A.txt"), "y").unwrap();
        // 大文字小文字を区別しないファイルシステムでは 2 つ目が作れない。その場合は確かめない。
        if fs::read_dir(&l).unwrap().count() < 2 {
            return;
        }
        fs::write(r.join("a.txt"), "x").unwrap();
        let e = compare_dirs(&l, &r).unwrap();
        assert_eq!(e.len(), 2);
        assert!(e.iter().any(|x| x.status == Status::LeftOnly));
        assert!(e.iter().any(|x| x.status == Status::Identical));
    }

    #[test]
    fn type_mismatch() {
        let root = tmp("type");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("x")).unwrap();
        fs::create_dir_all(&r).unwrap();
        fs::write(r.join("x"), "file").unwrap();
        let e = compare_dirs(&l, &r).unwrap();
        assert_eq!(find(&e, "x").status, Status::TypeMismatch);
    }

    #[test]
    fn missing_root_is_an_error() {
        let root = tmp("noroot");
        fs::create_dir_all(root.join("l")).unwrap();
        assert!(compare_dirs(&root.join("l"), &root.join("nothing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_subfolder_becomes_error_entry() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp("unreadable");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("locked")).unwrap();
        fs::create_dir_all(r.join("locked")).unwrap();
        fs::write(l.join("ok.txt"), "x").unwrap();
        fs::write(r.join("ok.txt"), "x").unwrap();
        fs::set_permissions(l.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        // root 権限では読めてしまうので確かめない。
        let readable = fs::read_dir(l.join("locked")).is_ok();
        let e = compare_dirs(&l, &r);
        fs::set_permissions(l.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }
        let e = e.unwrap();
        let locked = find(&e, "locked");
        assert_eq!(locked.status, Status::Error);
        assert!(locked.error.is_some());
        assert_eq!(find(&e, "ok.txt").status, Status::Identical);
    }

    #[test]
    fn reports_progress_and_can_be_cancelled() {
        let root = tmp("progress");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(&r).unwrap();
        for i in 0..5 {
            fs::write(l.join(format!("{i}.txt")), "x").unwrap();
        }
        let p = Progress::default();
        let e = compare_dirs_with(&l, &r, &p).unwrap();
        assert_eq!(e.len(), 5);
        assert_eq!(p.scanned(), 5);

        let p = Progress::default();
        p.cancel();
        let err = compare_dirs_with(&l, &r, &p).unwrap_err();
        assert_eq!(err.to_string(), CANCELLED);
    }

    #[test]
    fn recompare_updates_entry_and_parents() {
        let root = tmp("recompare");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("a/b")).unwrap();
        fs::create_dir_all(r.join("a/b")).unwrap();
        fs::write(l.join("a/b/f.txt"), "1").unwrap();
        fs::write(r.join("a/b/f.txt"), "22").unwrap();
        fs::write(l.join("a/same.txt"), "s").unwrap();
        fs::write(r.join("a/same.txt"), "s").unwrap();

        let mut e = compare_dirs(&l, &r).unwrap();
        assert_eq!(find(&e, "a").status, Status::Different);
        fs::write(r.join("a/b/f.txt"), "1").unwrap();
        let ix = e.iter().position(|x| x.rel == Path::new("a/b/f.txt")).unwrap();
        recompare(&mut e, ix, &l, &r);
        assert_eq!(find(&e, "a/b/f.txt").status, Status::Identical);
        assert_eq!(find(&e, "a/b/f.txt").right.unwrap().size, 1);
        assert_eq!(find(&e, "a/b").status, Status::Identical);
        assert_eq!(find(&e, "a").status, Status::Identical);
    }

    #[test]
    fn side_path_follows_parent_spelling() {
        let root = tmp("sidepath");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("Dir")).unwrap();
        fs::create_dir_all(r.join("dir")).unwrap();
        fs::write(l.join("Dir/new.txt"), "n").unwrap();
        fs::create_dir_all(l.join("Only/sub")).unwrap();

        let e = compare_dirs(&l, &r).unwrap();
        let ix = e.iter().position(|x| x.rel == Path::new("Dir/new.txt")).unwrap();
        assert_eq!(side_path(&e, ix, Side::Right, &r), r.join("dir").join("new.txt"));
        assert_eq!(side_path(&e, ix, Side::Left, &l), l.join("Dir/new.txt"));
        let ix = e.iter().position(|x| x.rel == Path::new("Only/sub")).unwrap();
        assert_eq!(side_path(&e, ix, Side::Right, &r), r.join("Only").join("sub"));

        let ix = e.iter().position(|x| x.rel == Path::new("Dir/new.txt")).unwrap();
        copy_entry(&side_path(&e, ix, Side::Left, &l), &side_path(&e, ix, Side::Right, &r)).unwrap();
        assert_eq!(fs::read_to_string(r.join("dir/new.txt")).unwrap(), "n");
    }

    #[test]
    fn ignores_os_metadata_files() {
        let root = tmp("ignore");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(&r).unwrap();
        fs::write(l.join("a.txt"), "x").unwrap();
        fs::write(r.join("a.txt"), "x").unwrap();
        for name in [".DS_Store", "._a.txt", "Thumbs.db", "desktop.ini"] {
            fs::write(l.join(name), "junk").unwrap();
        }
        fs::create_dir_all(r.join(".git")).unwrap();
        let e = compare_dirs(&l, &r).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].status, Status::Identical);

        // コピーでも外す。
        fs::create_dir_all(l.join("d")).unwrap();
        fs::write(l.join("d/._x"), "junk").unwrap();
        fs::write(l.join("d/x"), "x").unwrap();
        copy_entry(&l.join("d"), &r.join("d")).unwrap();
        assert!(r.join("d/x").exists());
        assert!(!r.join("d/._x").exists());
    }

    #[test]
    fn is_ignored_matches_names() {
        assert!(is_ignored(".DS_Store"));
        assert!(is_ignored("._main.rs"));
        assert!(is_ignored("thumbs.db"));
        assert!(!is_ignored("main.rs"));
        assert!(!is_ignored(".gitignore"));
    }

    #[test]
    fn copies_directories_recursively() {
        let root = tmp("copy");
        let (l, r) = (root.join("l"), root.join("r"));
        fs::create_dir_all(l.join("a/b")).unwrap();
        fs::create_dir_all(&r).unwrap();
        fs::write(l.join("a/b/f.txt"), "hi").unwrap();
        copy_entry(&l.join("a"), &r.join("a")).unwrap();
        assert_eq!(fs::read_to_string(r.join("a/b/f.txt")).unwrap(), "hi");
    }
}
