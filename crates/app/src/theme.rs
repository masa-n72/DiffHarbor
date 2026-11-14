//! 配色。WinMerge の既定色に寄せた明るいテーマ。
//!
//! システムの外観への追従は段階 3 で行う。その際はここを構造体に置き換える。

/// 本文の背景。
pub const BG: u32 = 0xFFFFFF;
/// ツールバー、ステータスバー、中央の操作列の背景。
pub const PANEL: u32 = 0xF0F0F0;
pub const BORDER: u32 = 0xC4C4C4;
pub const TEXT: u32 = 0x000000;
/// 行番号や補助的な文字。
pub const DIM: u32 = 0x7A7A7A;
pub const ACCENT: u32 = 0x3B78D8;
/// ペイン上部のパスバー。
pub const PATH_BAR: u32 = 0xAFC3E0;
/// 行番号欄の背景。
pub const GUTTER: u32 = 0xF4F4F4;

/// 差分行。
pub const DIFF: u32 = 0xEFCB05;
/// 選択中の差分行。
pub const DIFF_SEL: u32 = 0xEF7774;
/// 相手側にしかない行を埋める空行。
pub const FILLER: u32 = 0xC0C0C0;
pub const FILLER_SEL: u32 = 0xF0C0C0;
/// 行内の変更箇所。
pub const WORD: u32 = 0xF1E2AD;
pub const WORD_SEL: u32 = 0xFFA0A0;
/// 横スクロールバーのつまみ。
pub const SCROLL_THUMB: u32 = 0xA8A8A8;

pub const BUTTON: u32 = 0xFFFFFF;
pub const BUTTON_HOVER: u32 = 0xDCE9FB;
pub const BUTTON_DISABLED_TEXT: u32 = 0xA8A8A8;

pub const ROW_SELECTED: u32 = 0xCCE0FF;
pub const ROW_HOVER: u32 = 0xEEF4FF;
pub const DIR_NAME: u32 = 0x1A3E8A;

pub const STATUS_IDENTICAL: u32 = 0x2E7D32;
pub const STATUS_DIFFERENT: u32 = 0xA06A00;
pub const STATUS_LEFT_ONLY: u32 = 0xC62828;
pub const STATUS_RIGHT_ONLY: u32 = 0x1565C0;
pub const STATUS_MISMATCH: u32 = 0x7B1FA2;

/// 本文に使う等幅フォント。日本語は macOS のフォールバックで表示される。
pub const MONO_FONT: &str = "Menlo";
