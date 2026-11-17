//! DiffHarbor の差分エンジン。画面に依存しない。
//!
//! - [`text`]: 文字コード、改行コード、BOM を保持したテキストの読み書き
//! - [`diff`]: 行差分、差分ブロック、行内のトークン差分
//! - [`merge`]: 差分ブロックのコピーと、元に戻す、やり直すの履歴
//! - [`folder`]: フォルダの再帰比較と、項目のコピー
//! - [`ini`]: INI ファイルのキー単位の比較とコピー

pub mod diff;
pub mod folder;
pub mod ini;
pub mod merge;
pub mod text;

pub use diff::{apply_hunk, diff_lines, diff_lines_deadline, token_spans, DiffResult, Hunk, Row, RowKind};
pub use folder::{
    ancestors, compare_dirs, compare_dirs_with, copy_entry, next_diff, recompare, side_path, tree_order, visible_rows,
    FolderEntry, Meta, Progress, ShowFilter, SortKey, Status, CANCELLED,
};
pub use ini::{compare_ini, copy_edits, is_ini_path, IniCell, IniDiff, IniRow, IniStatus};
pub use merge::{History, LineEdit, Side};
pub use encoding_rs::{Encoding, EUC_JP, SHIFT_JIS, UTF_8};
pub use text::{decode, load, save, Eol, TextFile};
