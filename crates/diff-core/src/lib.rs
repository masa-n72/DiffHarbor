//! DiffHarbor の差分エンジン。画面に依存しない。
//!
//! - [`text`]: 文字コード、改行コード、BOM を保持したテキストの読み書き
//! - [`diff`]: 行差分、差分ブロック、行内のトークン差分

pub mod diff;
pub mod text;

pub use diff::{apply_hunk, diff_lines, token_spans, DiffResult, Hunk, Row, RowKind};
pub use encoding_rs::{Encoding, EUC_JP, SHIFT_JIS, UTF_8};
pub use text::{decode, load, save, Eol, TextFile};
