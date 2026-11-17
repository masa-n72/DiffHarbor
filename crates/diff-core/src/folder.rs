//! フォルダの再帰比較と、項目のコピー。

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Identical,
    Different,
    LeftOnly,
    RightOnly,
    /// A file on one side and a directory on the other.
    TypeMismatch,
}

#[derive(Clone, Debug)]
pub struct FolderEntry {
    /// Path relative to both roots.
    pub rel: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub status: Status,
}

/// 比較とコピーの対象から外す名前。
const IGNORED: &[&str] = &[".DS_Store", ".git", "Thumbs.db", "desktop.ini"];

/// 比較とコピーの対象から外すか。macOS が他のファイルシステムに作る `._*`（AppleDouble）も外す。
pub fn is_ignored(name: &str) -> bool {
    IGNORED.iter().any(|i| i.eq_ignore_ascii_case(name)) || name.starts_with("._")
}

pub fn compare_dirs(left: &Path, right: &Path) -> Result<Vec<FolderEntry>> {
    let mut out = Vec::new();
    walk(left, right, Path::new(""), 0, &mut out)?;
    Ok(out)
}

#[derive(Default)]
struct Pair {
    left: Option<bool>,  // Some(is_dir)
    right: Option<bool>,
}

fn list(dir: &Path) -> Result<BTreeMap<String, bool>> {
    let mut map = BTreeMap::new();
    for e in fs::read_dir(dir).with_context(|| format!("開けません: {}", dir.display()))? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if is_ignored(&name) {
            continue;
        }
        // Symlinks are treated as files and never followed into.
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        map.insert(name, is_dir);
    }
    Ok(map)
}

fn walk(left: &Path, right: &Path, rel: &Path, depth: usize, out: &mut Vec<FolderEntry>) -> Result<bool> {
    let l = if left.is_dir() { list(left)? } else { BTreeMap::new() };
    let r = if right.is_dir() { list(right)? } else { BTreeMap::new() };

    let mut names: BTreeMap<String, Pair> = BTreeMap::new();
    for (n, d) in l {
        names.entry(n).or_default().left = Some(d);
    }
    for (n, d) in r {
        names.entry(n).or_default().right = Some(d);
    }
    // Directories first, then by name (case-insensitive).
    let mut sorted: Vec<(String, Pair)> = names.into_iter().collect();
    sorted.sort_by_key(|(n, p)| (!(p.left == Some(true) || p.right == Some(true)), n.to_lowercase()));

    let mut all_identical = true;
    for (name, pair) in sorted {
        let child_rel = rel.join(&name);
        let (lp, rp) = (left.join(&name), right.join(&name));
        let idx = out.len();
        let is_dir = pair.left == Some(true) || pair.right == Some(true);
        let status = match (pair.left, pair.right) {
            (Some(true), Some(true)) => Status::Identical, // fixed up after recursion
            (Some(false), Some(false)) => {
                if files_equal(&lp, &rp) { Status::Identical } else { Status::Different }
            }
            (Some(_), Some(_)) => Status::TypeMismatch,
            (Some(_), None) => Status::LeftOnly,
            _ => Status::RightOnly,
        };
        out.push(FolderEntry { rel: child_rel.clone(), name, depth, is_dir, status });

        if is_dir && pair.left.is_some() && pair.right.is_some() && pair.left == pair.right {
            let same = walk(&lp, &rp, &child_rel, depth + 1, out)?;
            out[idx].status = if same { Status::Identical } else { Status::Different };
        } else if is_dir {
            // One-sided directory: list its contents with the same one-sided status.
            let side_only = out[idx].status;
            let base = if lp.is_dir() { &lp } else { &rp };
            list_one_sided(base, &child_rel, depth + 1, side_only, out)?;
        }
        if out[idx].status != Status::Identical {
            all_identical = false;
        }
    }
    Ok(all_identical)
}

fn list_one_sided(dir: &Path, rel: &Path, depth: usize, status: Status, out: &mut Vec<FolderEntry>) -> Result<()> {
    let mut items: Vec<(String, bool)> = list(dir)?.into_iter().collect();
    items.sort_by_key(|(n, d)| (!*d, n.to_lowercase()));
    for (name, is_dir) in items {
        let child_rel = rel.join(&name);
        out.push(FolderEntry { rel: child_rel.clone(), name: name.clone(), depth, is_dir, status });
        if is_dir {
            list_one_sided(&dir.join(&name), &child_rel, depth + 1, status, out)?;
        }
    }
    Ok(())
}

fn files_equal(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else { return false };
    if ma.len() != mb.len() {
        return false;
    }
    matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// Copy `rel` from `src_root` into `dst_root`, overwriting existing files.
pub fn copy_entry(src_root: &Path, dst_root: &Path, rel: &Path) -> Result<()> {
    let (src, dst) = (src_root.join(rel), dst_root.join(rel));
    if src.is_dir() {
        copy_dir(&src, &dst)
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if dst.is_dir() {
            anyhow::bail!("コピー先に同名のフォルダがあります: {}", dst.display());
        }
        fs::copy(&src, &dst).with_context(|| format!("コピーできません: {}", src.display()))?;
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
        let find = |p: &str| e.iter().find(|x| x.rel == Path::new(p)).unwrap().status;
        assert_eq!(find("same.txt"), Status::Identical);
        assert_eq!(find("diff.txt"), Status::Different);
        assert_eq!(find("only_l.txt"), Status::LeftOnly);
        assert_eq!(find("sub/only_r.txt"), Status::RightOnly);
        assert_eq!(find("sub"), Status::Different);
        // Directories come first.
        assert!(e[0].is_dir);
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
        copy_entry(&l, &r, Path::new("d")).unwrap();
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
        copy_entry(&l, &r, Path::new("a")).unwrap();
        assert_eq!(fs::read_to_string(r.join("a/b/f.txt")).unwrap(), "hi");
    }
}
