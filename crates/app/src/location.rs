//! 位置ペインの計算。ファイル全体のどこに差分があるかを、縦長のバーに写す。

use diff_core::DiffResult;

use crate::theme::{DIFF, DIFF_SEL, FILLER, FILLER_SEL};

/// バーの中で 1 行に割り当てる高さの上限。短いファイルでバーが間延びしないようにする。
const MAX_ROW_PX: f32 = 4.0;
/// 差分 1 件の最小の高さ。大きなファイルでも見えるようにする。
const MIN_BAND_PX: f32 = 2.0;
/// 表示範囲の枠の最小の高さ。
const MIN_VIEW_PX: f32 = 4.0;

/// バーに描く帯。`top` と `height` はバーの上端からのピクセル。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub top: f32,
    pub height: f32,
    pub color: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocationLayout {
    /// バーの高さ。
    pub bar_h: f32,
    pub left: Vec<Band>,
    pub right: Vec<Band>,
    /// 表示範囲の枠。
    pub view_top: f32,
    pub view_h: f32,
}

/// 位置ペインの配置を計算する。
///
/// - `avail_h`: バーに使える高さ
/// - `top_row`: 一覧の上端にある表示行（小数可）
/// - `visible_rows`: 一覧に見えている行数
pub fn layout(diff: &DiffResult, current: Option<usize>, avail_h: f32, top_row: f32, visible_rows: f32) -> LocationLayout {
    let rows = diff.rows.len();
    if rows == 0 || avail_h <= 0.0 {
        return LocationLayout::default();
    }
    let bar_h = avail_h.min(rows as f32 * MAX_ROW_PX);
    let scale = bar_h / rows as f32;

    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut selected = None;
    for (i, h) in diff.hunks.iter().enumerate() {
        let len = h.left.len().max(h.right.len());
        let top = (h.first_row as f32 * scale).floor();
        let height = (len as f32 * scale).ceil().max(MIN_BAND_PX).min(bar_h);
        let top = top.min(bar_h - height);
        let is_selected = current == Some(i);
        // 行がない側は灰色、ある側は差分の色。
        let color = |empty: bool| match (empty, is_selected) {
            (false, false) => DIFF,
            (false, true) => DIFF_SEL,
            (true, false) => FILLER,
            (true, true) => FILLER_SEL,
        };
        let (l, r) = (
            Band { top, height, color: color(h.left.is_empty()) },
            Band { top, height, color: color(h.right.is_empty()) },
        );
        if is_selected {
            selected = Some((l, r));
        } else {
            push_band(&mut left, l);
            push_band(&mut right, r);
        }
    }
    // 選択中の差分は最後に描き、他の帯に隠れないようにする。
    if let Some((l, r)) = selected {
        left.push(l);
        right.push(r);
    }

    let view_h = (visible_rows * scale).max(MIN_VIEW_PX).min(bar_h);
    let view_top = (top_row * scale).clamp(0.0, bar_h - view_h);
    LocationLayout { bar_h, left, right, view_top, view_h }
}

/// 同じ色で重なるか接する帯は 1 つにまとめる。大きなファイルでも帯の数がバーの高さを超えない。
fn push_band(out: &mut Vec<Band>, band: Band) {
    if let Some(last) = out.last_mut() {
        if last.color == band.color && band.top <= last.top + last.height {
            last.height = (band.top + band.height - last.top).max(last.height);
            return;
        }
    }
    out.push(band);
}

/// バーの上端から `y` ピクセルの位置にある表示行。
pub fn row_at(y: f32, bar_h: f32, rows: usize) -> usize {
    if rows == 0 || bar_h <= 0.0 {
        return 0;
    }
    ((y / bar_h).clamp(0.0, 1.0) * rows as f32).min(rows as f32 - 1.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use diff_core::diff_lines;

    fn lines(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn short_files_do_not_stretch_the_bar() {
        let l = lines(10);
        let d = diff_lines(&l, &l);
        let lay = layout(&d, None, 500.0, 0.0, 30.0);
        assert_eq!(lay.bar_h, 40.0);
        assert!(lay.left.is_empty() && lay.right.is_empty());
        // 全体が見えているので、枠はバー全体になる。
        assert_eq!((lay.view_top, lay.view_h), (0.0, 40.0));
    }

    #[test]
    fn bands_follow_hunk_positions_and_sides() {
        let l = lines(100);
        let mut r = l.clone();
        r[50] = "changed".into(); // 変更
        r.insert(80, "added".into()); // 右のみ
        let d = diff_lines(&l, &r);
        assert_eq!(d.hunks.len(), 2);
        let lay = layout(&d, Some(0), 202.0, 0.0, 20.0);
        // 選択中の差分は最後に入る。
        let sel = lay.left.last().unwrap();
        assert_eq!(sel.color, DIFF_SEL);
        assert!((sel.top - 100.0).abs() <= 2.0, "{sel:?}");
        // 右のみの行は、左のバーでは灰色、右のバーでは差分の色。
        assert_eq!(lay.left[0].color, FILLER);
        assert_eq!(lay.right[0].color, DIFF);
        assert!((lay.left[0].top - 160.0).abs() <= 2.0);
    }

    #[test]
    fn many_hunks_merge_into_few_bands() {
        let l = lines(100_000);
        let r: Vec<String> = l.iter().enumerate().map(|(i, s)| if i % 2 == 0 { format!("{s}!") } else { s.clone() }).collect();
        let d = diff_lines(&l, &r);
        assert_eq!(d.hunks.len(), 50_000);
        let lay = layout(&d, None, 600.0, 0.0, 30.0);
        assert!(lay.left.len() <= 600, "{}", lay.left.len());
        let last = lay.left.last().unwrap();
        assert!(last.top + last.height <= 600.0 + 0.01);
    }

    #[test]
    fn view_rect_tracks_scroll_and_stays_inside() {
        let l = lines(1000);
        let d = diff_lines(&l, &l);
        let lay = layout(&d, None, 500.0, 500.0, 50.0);
        assert_eq!((lay.view_top, lay.view_h), (250.0, 25.0));
        let end = layout(&d, None, 500.0, 990.0, 50.0);
        assert_eq!(end.view_top + end.view_h, 500.0);
    }

    #[test]
    fn row_at_maps_pixels_back_to_rows() {
        assert_eq!(row_at(0.0, 500.0, 1000), 0);
        assert_eq!(row_at(250.0, 500.0, 1000), 500);
        assert_eq!(row_at(9999.0, 500.0, 1000), 999);
        assert_eq!(row_at(-5.0, 500.0, 1000), 0);
        assert_eq!(row_at(10.0, 0.0, 0), 0);
    }
}
