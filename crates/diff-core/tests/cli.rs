//! dhdiff を実際に起動して確認する。

use std::{fs, path::PathBuf, process::Command};

use encoding_rs::SHIFT_JIS;

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("dhdiff-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn dhdiff(args: &[&PathBuf]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_dhdiff")).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

#[test]
fn compares_utf8_with_shift_jis_as_characters() {
    let dir = tmp_dir("sjis");
    let (l, r) = (dir.join("utf8.txt"), dir.join("sjis.txt"));
    let left = "設定ファイルの説明です。\n今日は晴れです。\n文字コードが違っても比較できます。\n";
    let right = "設定ファイルの説明です。\r\n今日は雨です。\r\n文字コードが違っても比較できます。\r\n追加の行です。\r\n";
    fs::write(&l, left).unwrap();
    fs::write(&r, SHIFT_JIS.encode(right).0).unwrap();

    let (code, out) = dhdiff(&[&l, &r]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[UTF-8 / LF]"), "{out}");
    assert!(out.contains("[Shift_JIS (推定) / CRLF]"), "{out}");
    assert!(out.contains("- 今日は[-晴れ-]です。"), "{out}");
    assert!(out.contains("+ 今日は{+雨+}です。"), "{out}");
    assert!(out.contains("+ 追加の行です。"), "{out}");
    assert!(out.contains("差分: 2 件"), "{out}");
}

#[test]
fn identical_content_in_different_encodings_exits_zero() {
    let dir = tmp_dir("same");
    let (l, r) = (dir.join("a.txt"), dir.join("b.txt"));
    let text = "同じ内容のファイルです。\n文字コードと改行コードだけが違います。\n";
    fs::write(&l, text).unwrap();
    fs::write(&r, SHIFT_JIS.encode(&text.replace('\n', "\r\n")).0).unwrap();
    let (code, out) = dhdiff(&[&l, &r]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("差分: 0 件"), "{out}");
}

#[test]
fn binary_file_exits_two() {
    let dir = tmp_dir("bin");
    let (l, r) = (dir.join("a.bin"), dir.join("b.txt"));
    fs::write(&l, [1u8, 0, 2, 3]).unwrap();
    fs::write(&r, "x\n").unwrap();
    assert_eq!(dhdiff(&[&l, &r]).0, 2);
}

/// 段階 1 の完了条件。Shift_JIS のファイルをマージして保存しても、
/// 文字コード、改行コード、BOM が変わらない。
#[test]
fn merge_and_save_keeps_encoding_eol_and_bom() {
    use diff_core::{diff_lines, load, save, History, Side, SHIFT_JIS};

    let dir = tmp_dir("merge");
    let (lp, rp) = (dir.join("utf8_bom.txt"), dir.join("sjis.txt"));
    let left = "設定ファイルの説明です。\n今日は晴れです。\n文字コードが違っても比較できます。\n左だけにある行です。\n";
    let right = "設定ファイルの説明です。\r\n今日は雨です。\r\n文字コードが違っても比較できます。\r\n";
    let mut left_bytes = vec![0xEF, 0xBB, 0xBF];
    left_bytes.extend_from_slice(left.as_bytes());
    fs::write(&lp, &left_bytes).unwrap();
    fs::write(&rp, SHIFT_JIS.encode(right).0).unwrap();

    let (mut l, mut r) = (load(&lp, None).unwrap(), load(&rp, None).unwrap());
    let mut history = History::new();
    let d = diff_lines(&l.lines, &r.lines);
    assert_eq!(d.hunks.len(), 2);
    // すべて右へコピーして保存する。
    history.copy_hunks(&mut l.lines, &mut r.lines, &d.hunks, Side::Right);
    save(&rp, &r).unwrap();

    // 右は Shift_JIS、CRLF のまま、内容は左と同じになる。
    let expected = SHIFT_JIS.encode(&left.replace('\n', "\r\n")).0.into_owned();
    assert_eq!(fs::read(&rp).unwrap(), expected);
    // 左は触っていないので、BOM 付き UTF-8、LF のまま。
    save(&lp, &l).unwrap();
    assert_eq!(fs::read(&lp).unwrap(), left_bytes);

    let (code, out) = dhdiff(&[&lp, &rp]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("[UTF-8 (BOM) / LF]"), "{out}");
    assert!(out.contains("[Shift_JIS (推定) / CRLF]"), "{out}");
}
