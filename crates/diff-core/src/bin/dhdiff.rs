//! 確認用の CLI。2 つのテキストファイルを比較して端末に表示する。
//!
//! 使い方: dhdiff LEFT RIGHT
//! 終了コード: 0 = 同一、1 = 差分あり、2 = エラー

use std::{path::Path, process::ExitCode};

use diff_core::{diff_lines, load, token_spans, RowKind};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [left, right] = args.as_slice() else {
        eprintln!("使い方: dhdiff LEFT RIGHT");
        return ExitCode::from(2);
    };
    match run(Path::new(left), Path::new(right)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("エラー: {e}");
            ExitCode::from(2)
        }
    }
}

/// 変更範囲を `open`..`close` で囲んだ文字列を作る。
fn mark(line: &str, spans: &[std::ops::Range<usize>], open: &str, close: &str) -> String {
    let mut out = String::new();
    let mut pos = 0;
    for s in spans {
        out.push_str(&line[pos..s.start]);
        out.push_str(open);
        out.push_str(&line[s.clone()]);
        out.push_str(close);
        pos = s.end;
    }
    out.push_str(&line[pos..]);
    out
}

fn run(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let (l, r) = (load(left, None)?, load(right, None)?);
    println!("左: {}  [{} / {}]", left.display(), l.encoding_label(), l.eol_label());
    println!("右: {}  [{} / {}]", right.display(), r.encoding_label(), r.eol_label());

    let d = diff_lines(&l.lines, &r.lines);
    for row in &d.rows {
        let num = |n: Option<usize>| n.map(|n| format!("{:>5}", n + 1)).unwrap_or_else(|| " ".repeat(5));
        match row.kind {
            RowKind::Equal => {
                println!("{} {}   {}", num(row.left), num(row.right), l.lines[row.left.unwrap()])
            }
            RowKind::LeftOnly => {
                println!("{} {} - {}", num(row.left), num(None), l.lines[row.left.unwrap()])
            }
            RowKind::RightOnly => {
                println!("{} {} + {}", num(None), num(row.right), r.lines[row.right.unwrap()])
            }
            RowKind::Changed => {
                let (lt, rt) = (&l.lines[row.left.unwrap()], &r.lines[row.right.unwrap()]);
                let (ls, rs) = token_spans(lt, rt);
                println!("{} {} - {}", num(row.left), num(None), mark(lt, &ls, "[-", "-]"));
                println!("{} {} + {}", num(None), num(row.right), mark(rt, &rs, "{+", "+}"));
            }
        }
    }
    println!("差分: {} 件", d.hunks.len());
    Ok(d.hunks.is_empty())
}
