//! 行を画面に出すための変換。タブの展開と表示幅の計算。

use std::ops::Range;

pub const TAB_WIDTH: usize = 4;

/// 全角として扱う文字か。等幅フォントで 2 桁ぶんの幅を取る。
pub fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F      // ハングル字母
        | 0x2E80..=0x303E    // CJK 部首、記号
        | 0x3041..=0x33FF    // ひらがな、カタカナ、CJK 記号
        | 0x3400..=0x4DBF    // CJK 拡張 A
        | 0x4E00..=0x9FFF    // CJK 統合漢字
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3    // ハングル音節
        | 0xF900..=0xFAFF    // CJK 互換漢字
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60    // 全角英数、記号
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF  // 絵文字
        | 0x20000..=0x3FFFD
    )
}

/// 表示幅。戻り値は (半角の桁数, 全角の文字数)。タブは桁位置に合わせて展開する。
pub fn display_width(line: &str) -> (usize, usize) {
    let (mut narrow, mut wide) = (0usize, 0usize);
    for c in line.chars() {
        if c == '\t' {
            let col = narrow + wide * 2;
            narrow += TAB_WIDTH - col % TAB_WIDTH;
        } else if is_wide(c) {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    (narrow, wide)
}

/// タブを空白に展開し、`spans`（元の行でのバイト範囲）を展開後の位置に直す。
pub fn expand_tabs(line: &str, spans: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    if !line.contains('\t') {
        return (line.to_string(), spans.to_vec());
    }
    let mut out = String::with_capacity(line.len() + 8);
    // 元のバイト位置から展開後のバイト位置への対応。
    let mut map = vec![0usize; line.len() + 1];
    let mut col = 0usize;
    for (i, c) in line.char_indices() {
        map[i] = out.len();
        if c == '\t' {
            let n = TAB_WIDTH - col % TAB_WIDTH;
            out.extend(std::iter::repeat(' ').take(n));
            col += n;
        } else {
            out.push(c);
            col += if is_wide(c) { 2 } else { 1 };
        }
    }
    map[line.len()] = out.len();
    let spans = spans.iter().map(|r| map[r.start]..map[r.end]).collect();
    (out, spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_expand_to_tab_stops() {
        assert_eq!(expand_tabs("\tx", &[]).0, "    x");
        assert_eq!(expand_tabs("ab\tx", &[]).0, "ab  x");
        assert_eq!(expand_tabs("abcd\tx", &[]).0, "abcd    x");
        // 全角 1 文字は 2 桁。
        assert_eq!(expand_tabs("あ\tx", &[]).0, "あ  x");
    }

    #[test]
    fn spans_follow_tab_expansion() {
        let line = "\tlet\tx = 1;";
        let start = line.find('x').unwrap();
        let (out, spans) = expand_tabs(line, &[start..start + 1]);
        assert_eq!(&out[spans[0].clone()], "x");
        // タブ自体を含む範囲も文字の境界に収まる。
        let (out, spans) = expand_tabs(line, &[0..4]);
        assert_eq!(&out[spans[0].clone()], "    let");
    }

    #[test]
    fn display_width_counts_wide_characters() {
        assert_eq!(display_width("abc"), (3, 0));
        assert_eq!(display_width("日本語abc"), (3, 3));
        assert_eq!(display_width("ﾊﾝｶｸ"), (4, 0));
        assert_eq!(display_width("\tx"), (5, 0));
    }
}
