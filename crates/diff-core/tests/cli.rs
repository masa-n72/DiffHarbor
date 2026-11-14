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
