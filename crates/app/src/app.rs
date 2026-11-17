//! The DiffHarbor main view: toolbar, folder comparison and side-by-side file diff.

use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gpui::{
    actions, canvas, div, fill, font, point, prelude::*, px, quad, rgb, rgba, size, uniform_list,
    AnyElement, App, BorderStyle, Bounds, ClickEvent, Context, Corners, Edges, FocusHandle,
    HighlightStyle, Hsla, KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent,
    PathPromptOptions, PromptLevel, Render, ScrollStrategy, ScrollWheelEvent, SharedString, Stateful,
    StyledText, UniformListScrollHandle, Window,
};

use diff_core::{
    diff_lines, diff_lines_deadline, token_spans, DiffResult, Encoding, History, RowKind, Side, EUC_JP,
    SHIFT_JIS, UTF_8,
};

use crate::{
    display::{display_width, expand_tabs},
    document::Document,
    theme::*,
    folder::{compare_dirs, copy_entry, FolderEntry, Status},
    location,
};

actions!(
    diffharbor,
    [
        Quit,
        OpenLeft,
        OpenRight,
        NextDiff,
        PrevDiff,
        FirstDiff,
        LastDiff,
        CopyToRight,
        CopyToLeft,
        CopyAllToRight,
        CopyAllToLeft,
        Save,
        SaveLeftAs,
        SaveRightAs,
        Undo,
        Redo,
        Refresh,
        Back,
        CloseWindow,
        ReopenLeftAuto,
        ReopenLeftUtf8,
        ReopenLeftShiftJis,
        ReopenLeftEucJp,
        ReopenRightAuto,
        ReopenRightUtf8,
        ReopenRightShiftJis,
        ReopenRightEucJp,
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenLeft, None),
        KeyBinding::new("cmd-shift-o", OpenRight, None),
        KeyBinding::new("alt-down", NextDiff, None),
        KeyBinding::new("alt-up", PrevDiff, None),
        KeyBinding::new("alt-home", FirstDiff, None),
        KeyBinding::new("alt-end", LastDiff, None),
        // Home / End キーがないキーボード向け。
        KeyBinding::new("alt-cmd-up", FirstDiff, None),
        KeyBinding::new("alt-cmd-down", LastDiff, None),
        KeyBinding::new("alt-right", CopyToRight, None),
        KeyBinding::new("alt-left", CopyToLeft, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-z", Undo, None),
        KeyBinding::new("cmd-shift-z", Redo, None),
        KeyBinding::new("cmd-r", Refresh, None),
        KeyBinding::new("cmd-[", Back, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(vec![
        Menu {
            name: "DiffHarbor".into(),
            items: vec![MenuItem::action("DiffHarbor を終了", Quit)],
        },
        Menu {
            name: "ファイル".into(),
            items: vec![
                MenuItem::action("左を開く…", OpenLeft),
                MenuItem::action("右を開く…", OpenRight),
                MenuItem::separator(),
                MenuItem::action("保存", Save),
                MenuItem::action("左を別名で保存…", SaveLeftAs),
                MenuItem::action("右を別名で保存…", SaveRightAs),
                MenuItem::separator(),
                MenuItem::action("フォルダを再比較", Refresh),
                MenuItem::action("フォルダ一覧へ戻る", Back),
                MenuItem::separator(),
                MenuItem::action("ウインドウを閉じる", CloseWindow),
            ],
        },
        Menu {
            name: "差分".into(),
            items: vec![
                MenuItem::action("次の差分", NextDiff),
                MenuItem::action("前の差分", PrevDiff),
                MenuItem::action("先頭の差分", FirstDiff),
                MenuItem::action("末尾の差分", LastDiff),
                MenuItem::separator(),
                MenuItem::action("右へコピー", CopyToRight),
                MenuItem::action("左へコピー", CopyToLeft),
                MenuItem::action("すべて右へコピー", CopyAllToRight),
                MenuItem::action("すべて左へコピー", CopyAllToLeft),
                MenuItem::separator(),
                MenuItem::action("元に戻す", Undo),
                MenuItem::action("やり直す", Redo),
            ],
        },
        Menu {
            name: "文字コード".into(),
            items: vec![
                MenuItem::action("左を自動判定で開き直す", ReopenLeftAuto),
                MenuItem::action("左を UTF-8 で開き直す", ReopenLeftUtf8),
                MenuItem::action("左を Shift_JIS で開き直す", ReopenLeftShiftJis),
                MenuItem::action("左を EUC-JP で開き直す", ReopenLeftEucJp),
                MenuItem::separator(),
                MenuItem::action("右を自動判定で開き直す", ReopenRightAuto),
                MenuItem::action("右を UTF-8 で開き直す", ReopenRightUtf8),
                MenuItem::action("右を Shift_JIS で開き直す", ReopenRightShiftJis),
                MenuItem::action("右を EUC-JP で開き直す", ReopenRightEucJp),
            ],
        },
    ]);
    cx.on_window_closed(|cx| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

const ROW_H: f32 = 20.0;
const FONT_SIZE: f32 = 12.0;
/// 行番号欄の幅。
const GUTTER_W: f32 = 48.0;
/// 中央の操作列の幅。
const MID_W: f32 = 44.0;
/// 左端の位置ペインの幅。
const LOC_W: f32 = 44.0;
/// 位置ペインの上下の余白。
const LOC_PAD: f32 = 6.0;
/// 左右の合計がこの行数以下なら、差分をその場で計算する。超える場合は別スレッドで計算する。
const SYNC_DIFF_LINES: usize = 20_000;
/// 差分計算の制限時間。超えた場合は最小ではない差分になる。
const DIFF_TIME_LIMIT: Duration = Duration::from_secs(5);
/// これより長い行は、行内の差分を取らずに行全体を強調する。
const TOKEN_DIFF_MAX_BYTES: usize = 2000;

type Spans = Rc<(Vec<Range<usize>>, Vec<Range<usize>>)>;

/// 開発時の確認用。デバッグビルドでは DIFFHARBOR_DEBUG_ACTIONS にカンマ区切りで操作を並べられる。
fn debug_actions() -> VecDeque<String> {
    if cfg!(debug_assertions) {
        std::env::var("DIFFHARBOR_DEBUG_ACTIONS")
            .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    } else {
        VecDeque::new()
    }
}

/// 開発時の確認用。デバッグビルドでは DIFFHARBOR_DEBUG_HUNK で最初に選択する差分（0 始まり）を指定できる。
fn debug_initial_hunk() -> usize {
    if cfg!(debug_assertions) {
        std::env::var("DIFFHARBOR_DEBUG_HUNK").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    } else {
        0
    }
}

/// 開発時の確認用。デバッグビルドでは DIFFHARBOR_DEBUG_HSCROLL で横スクロールの初期位置を指定できる。
fn debug_initial_h_scroll() -> f32 {
    if cfg!(debug_assertions) {
        std::env::var("DIFFHARBOR_DEBUG_HSCROLL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0)
    } else {
        0.0
    }
}

#[derive(Clone)]
struct FolderView {
    left: PathBuf,
    right: PathBuf,
    entries: Vec<FolderEntry>,
    visible: Vec<usize>,
    selected: Option<usize>,
    only_diffs: bool,
}

impl FolderView {
    fn new(left: PathBuf, right: PathBuf, only_diffs: bool) -> anyhow::Result<Self> {
        let entries = compare_dirs(&left, &right)?;
        let mut v = Self { left, right, entries, visible: Vec::new(), selected: None, only_diffs };
        v.update_visible();
        Ok(v)
    }

    fn update_visible(&mut self) {
        let keep = self.selected.and_then(|s| self.visible.get(s).copied());
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !self.only_diffs || e.status != Status::Identical)
            .map(|(i, _)| i)
            .collect();
        self.selected = keep.and_then(|k| self.visible.iter().position(|&i| i == k));
    }

    fn selected_entry(&self) -> Option<&FolderEntry> {
        self.selected.and_then(|s| self.visible.get(s)).map(|&i| &self.entries[i])
    }
}

/// 未保存の変更を確認した後に実行する操作。
enum Pending {
    CloseWindow,
    Quit,
    /// `side` に別のファイルやフォルダを開く。
    Open(Side, PathBuf),
    Back,
    /// `side` を指定の文字コード（None は自動判定）で開き直す。
    Reopen(Side, Option<&'static Encoding>),
}

struct FileView {
    left: Document,
    right: Document,
    /// 位置ペインの描画と共有するので Rc で持つ。
    diff: Rc<DiffResult>,
    current: Option<usize>,
    /// コピー操作の履歴。元に戻す、やり直すに使う。
    history: History,
    back: Option<Box<FolderView>>,
    /// 差分を別スレッドで計算中か。計算中は行の一覧を出さない。
    computing: bool,
    /// 差分計算の世代。古い計算結果を捨てるために使う。
    generation: u64,
    /// 横スクロール量（ピクセル）。左右のペインで共有する。
    h_scroll: f32,
    /// 最長行の表示幅（ピクセル）。差分の更新時に破棄し、描画時に計算し直す。
    max_line_px: Option<f32>,
    /// 変更行ごとの行内差分。キーは表示行の番号。
    span_cache: HashMap<usize, Spans>,
}

impl FileView {
    fn new(left: &Path, right: &Path, back: Option<Box<FolderView>>) -> anyhow::Result<Self> {
        let (left, right) = (Document::load(left, None)?, Document::load(right, None)?);
        Ok(Self {
            left,
            right,
            diff: Rc::new(DiffResult::default()),
            current: None,
            history: History::new(),
            back,
            computing: false,
            generation: 0,
            h_scroll: debug_initial_h_scroll(),
            max_line_px: None,
            span_cache: HashMap::new(),
        })
    }

    fn set_diff(&mut self, diff: DiffResult) {
        self.diff = Rc::new(diff);
        self.computing = false;
        self.span_cache.clear();
        self.max_line_px = None;
        self.current = match (self.current, self.diff.hunks.len()) {
            (_, 0) => None,
            (Some(c), n) => Some(c.min(n - 1)),
            (None, n) => Some(debug_initial_hunk().min(n - 1)),
        };
    }

    fn doc(&self, side: Side) -> &Document {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn doc_mut(&mut self, side: Side) -> &mut Document {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// 履歴の版と保存時の版を比べて、未保存フラグを更新する。
    fn sync_dirty(&mut self) {
        self.left.dirty = self.history.rev(Side::Left) != self.left.saved_rev;
        self.right.dirty = self.history.rev(Side::Right) != self.right.saved_rev;
    }

    fn mark_saved(&mut self, side: Side) {
        let rev = self.history.rev(side);
        self.doc_mut(side).saved_rev = rev;
        self.sync_dirty();
    }

    fn has_unsaved(&self) -> bool {
        self.left.dirty || self.right.dirty
    }

    /// 表示行 `ix` が変更行なら、行内の差分を計算して覚えておく。
    fn ensure_spans(&mut self, ix: usize) {
        let Some(row) = self.diff.rows.get(ix) else { return };
        if row.kind != RowKind::Changed || self.span_cache.contains_key(&ix) {
            return;
        }
        let (Some(l), Some(r)) = (row.left, row.right) else { return };
        let (lt, rt) = (&self.left.lines[l], &self.right.lines[r]);
        let spans = if lt.len() > TOKEN_DIFF_MAX_BYTES || rt.len() > TOKEN_DIFF_MAX_BYTES {
            (vec![0..lt.len()], vec![0..rt.len()])
        } else {
            token_spans(lt, rt)
        };
        self.span_cache.insert(ix, Rc::new(spans));
    }
}

enum View {
    Empty,
    Folder(FolderView),
    File(FileView),
}

pub struct DiffHarbor {
    focus: FocusHandle,
    left_path: Option<PathBuf>,
    right_path: Option<PathBuf>,
    view: View,
    status: SharedString,
    diff_scroll: UniformListScrollHandle,
    folder_scroll: UniformListScrollHandle,
    only_diffs: bool,
    title: String,
    /// 等幅フォントの半角 1 文字の幅。最初の描画で測る。
    char_w: Option<f32>,
    /// 位置ペインのバーの (上端の y 座標, 高さ)。描画時に記録し、クリック位置の計算に使う。
    loc_geom: Rc<Cell<Option<(f32, f32)>>>,
    /// 未保存の確認を済ませて、ウィンドウを閉じてよい状態か。
    allow_close: bool,
    /// 開発時の確認用。差分の計算が終わるたびに 1 つずつ実行する操作。
    debug_actions: VecDeque<String>,
}

impl DiffHarbor {
    pub fn new(args: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut this = Self {
            focus,
            left_path: None,
            right_path: None,
            view: View::Empty,
            status: "左右のファイルまたはフォルダを開いてください (⌘O / ⇧⌘O)".into(),
            diff_scroll: UniformListScrollHandle::new(),
            folder_scroll: UniformListScrollHandle::new(),
            only_diffs: false,
            title: String::new(),
            char_w: None,
            loc_geom: Rc::new(Cell::new(None)),
            allow_close: false,
            debug_actions: debug_actions(),
        };
        // 閉じるボタンで閉じる時も、未保存の変更を確認する。
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                if this.allow_close || !this.has_unsaved() {
                    true
                } else {
                    this.guard(Pending::CloseWindow, window, cx);
                    false
                }
            })
            .unwrap_or(true)
        });
        if let [l, r, ..] = args.as_slice() {
            this.left_path = Some(l.clone());
            this.right_path = Some(r.clone());
            this.compare(cx);
        }
        this
    }

    fn set_status(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = msg.into();
        cx.notify();
    }

    fn has_unsaved(&self) -> bool {
        matches!(&self.view, View::File(f) if f.has_unsaved())
    }

    /// 未保存の変更があれば確認してから `pending` を実行する。なければすぐ実行する。
    fn guard(&mut self, pending: Pending, window: &mut Window, cx: &mut Context<Self>) {
        let unsaved = match (&pending, &self.view) {
            // 開き直しで失われるのは、その側の変更だけ。
            (Pending::Reopen(side, _), View::File(f)) => f.doc(*side).dirty,
            _ => self.has_unsaved(),
        };
        if !unsaved {
            self.run(pending, window, cx);
            return;
        }
        let rx = window.prompt(
            PromptLevel::Warning,
            "変更が保存されていません",
            Some("保存しない場合、マージした内容は失われます。"),
            &["保存", "保存しない", "キャンセル"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = rx.await else { return };
            this.update_in(cx, |this, window, cx| match answer {
                0 => match this.save_all() {
                    Ok(_) => this.run(pending, window, cx),
                    Err(e) => this.set_status(e, cx),
                },
                1 => this.run(pending, window, cx),
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    fn run(&mut self, pending: Pending, window: &mut Window, cx: &mut Context<Self>) {
        match pending {
            Pending::CloseWindow => {
                self.allow_close = true;
                window.remove_window();
            }
            Pending::Quit => cx.quit(),
            Pending::Open(side, path) => {
                match side {
                    Side::Left => self.left_path = Some(path),
                    Side::Right => self.right_path = Some(path),
                }
                self.compare(cx);
            }
            Pending::Back => self.back_now(cx),
            Pending::Reopen(side, enc) => self.reopen(side, enc, cx),
        }
    }

    fn pick(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(
                match side {
                    Side::Left => "左側を選択",
                    Side::Right => "右側を選択",
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                    // 開く対象が決まってから、未保存の変更を確認する。
                    this.update_in(cx, |this, window, cx| this.guard(Pending::Open(side, path), window, cx)).ok();
                }
            }
        })
        .detach();
    }

    /// Compare the two chosen paths (file/file or folder/folder).
    fn compare(&mut self, cx: &mut Context<Self>) {
        let (Some(l), Some(r)) = (self.left_path.clone(), self.right_path.clone()) else {
            cx.notify();
            return;
        };
        let result: anyhow::Result<View> = if l.is_dir() && r.is_dir() {
            FolderView::new(l, r, self.only_diffs).map(View::Folder)
        } else if l.is_file() && r.is_file() {
            FileView::new(&l, &r, None).map(View::File)
        } else {
            Err(anyhow::anyhow!("ファイル同士、またはフォルダ同士を選択してください"))
        };
        match result {
            Ok(view) => {
                self.view = view;
                match &self.view {
                    View::File(_) => self.start_diff(cx),
                    View::Folder(f) => {
                        let n = f.entries.iter().filter(|e| !e.is_dir && e.status != Status::Identical).count();
                        self.status = format!("{n} 件のファイルに差分があります").into();
                    }
                    View::Empty => {}
                }
            }
            Err(e) => self.status = e.to_string().into(),
        }
        cx.notify();
    }

    /// 現在のファイル比較の差分を計算し直す。大きなファイルは別スレッドで計算する。
    fn start_diff(&mut self, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        f.generation += 1;
        if f.left.lines.len() + f.right.lines.len() <= SYNC_DIFF_LINES {
            let diff = diff_lines(&f.left.lines, &f.right.lines);
            f.set_diff(diff);
            self.after_diff(cx);
            return;
        }
        f.computing = true;
        let generation = f.generation;
        let (l, r) = (f.left.lines.clone(), f.right.lines.clone());
        let task = cx
            .background_executor()
            .spawn(async move { diff_lines_deadline(&l, &r, Some(DIFF_TIME_LIMIT)) });
        self.status = "比較中…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let diff = task.await;
            this.update(cx, |this, cx| {
                if let View::File(f) = &mut this.view {
                    // 計算中に別の比較が始まっていたら、この結果は捨てる。
                    if f.generation == generation {
                        f.set_diff(diff);
                        this.after_diff(cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn after_diff(&mut self, cx: &mut Context<Self>) {
        if let View::File(f) = &self.view {
            let n = f.diff.hunks.len();
            self.status = if n == 0 { "差分はありません".into() } else { format!("{n} 件の差分").into() };
        }
        self.scroll_to_current();
        cx.notify();
        self.run_debug_action(cx);
    }

    // ---- folder actions ----

    fn select_entry(&mut self, vis_ix: usize, cx: &mut Context<Self>) {
        if let View::Folder(f) = &mut self.view {
            f.selected = Some(vis_ix);
            cx.notify();
        }
    }

    fn open_entry(&mut self, vis_ix: usize, cx: &mut Context<Self>) {
        let View::Folder(f) = &self.view else { return };
        let Some(&ei) = f.visible.get(vis_ix) else { return };
        let entry = f.entries[ei].clone();
        if entry.is_dir {
            return;
        }
        if !matches!(entry.status, Status::Identical | Status::Different) {
            self.set_status("両側に存在するファイルのみ比較できます", cx);
            return;
        }
        let (l, r) = (f.left.join(&entry.rel), f.right.join(&entry.rel));
        let View::Folder(mut folder) = std::mem::replace(&mut self.view, View::Empty) else { return };
        folder.selected = Some(vis_ix);
        match FileView::new(&l, &r, Some(Box::new(folder.clone()))) {
            Ok(fv) => {
                self.view = View::File(fv);
                self.start_diff(cx);
            }
            Err(e) => {
                self.view = View::Folder(folder);
                self.set_status(e.to_string(), cx);
            }
        }
    }

    fn copy_selected(&mut self, to: Side, cx: &mut Context<Self>) {
        let View::Folder(f) = &self.view else { return };
        let Some(entry) = f.selected_entry() else {
            self.set_status("コピーする項目を選択してください", cx);
            return;
        };
        let (src, dst) = match to {
            Side::Right => (&f.left, &f.right),
            Side::Left => (&f.right, &f.left),
        };
        let exists_in_src = src.join(&entry.rel).symlink_metadata().is_ok();
        if !exists_in_src {
            self.set_status("コピー元に存在しません", cx);
            return;
        }
        let name = entry.rel.display().to_string();
        match copy_entry(src, dst, &entry.rel) {
            Ok(()) => {
                self.refresh(cx);
                self.set_status(format!("コピーしました: {name}"), cx);
            }
            Err(e) => self.set_status(e.to_string(), cx),
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let View::Folder(f) = &self.view {
            match FolderView::new(f.left.clone(), f.right.clone(), f.only_diffs) {
                Ok(mut nf) => {
                    nf.selected = f.selected.filter(|&s| s < nf.visible.len());
                    self.view = View::Folder(nf);
                }
                Err(e) => self.status = e.to_string().into(),
            }
            cx.notify();
        }
    }

    fn toggle_only_diffs(&mut self, cx: &mut Context<Self>) {
        self.only_diffs = !self.only_diffs;
        if let View::Folder(f) = &mut self.view {
            f.only_diffs = self.only_diffs;
            f.update_visible();
        }
        cx.notify();
    }

    // ---- file actions ----

    fn scroll_to_current(&self) {
        if let View::File(f) = &self.view {
            if let Some(h) = f.current.and_then(|c| f.diff.hunks.get(c)) {
                self.diff_scroll.scroll_to_item(h.first_row, ScrollStrategy::Center);
            }
        }
    }

    fn select_hunk(&mut self, hunk: usize, cx: &mut Context<Self>) {
        if let View::File(f) = &mut self.view {
            if !f.computing && hunk < f.diff.hunks.len() && f.current != Some(hunk) {
                f.current = Some(hunk);
                cx.notify();
            }
        }
    }

    fn step_hunk(&mut self, delta: isize, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        let n = f.diff.hunks.len();
        if n == 0 || f.computing {
            return;
        }
        let next = match f.current {
            None => 0,
            Some(c) => (c as isize + delta).clamp(0, n as isize - 1) as usize,
        };
        // 端で止まった時は、移動せずにその旨を知らせる。
        if f.current == Some(next) {
            self.status = if delta > 0 { "最後の差分です".into() } else { "最初の差分です".into() };
        } else {
            f.current = Some(next);
            self.status = format!("{n} 件の差分").into();
        }
        self.scroll_to_current();
        cx.notify();
    }

    /// 先頭（`last` が false）または末尾の差分へ移動する。
    fn goto_edge_hunk(&mut self, last: bool, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        let n = f.diff.hunks.len();
        if n == 0 || f.computing {
            return;
        }
        f.current = Some(if last { n - 1 } else { 0 });
        self.status = format!("{n} 件の差分").into();
        self.scroll_to_current();
        cx.notify();
    }

    /// 位置ペイン上の y 座標に対応する行へスクロールする。
    fn jump_to_location(&mut self, y: f32, cx: &mut Context<Self>) {
        let View::File(f) = &self.view else { return };
        let Some((top, bar_h)) = self.loc_geom.get() else { return };
        if f.computing || f.diff.rows.is_empty() {
            return;
        }
        let row = location::row_at(y - top, bar_h, f.diff.rows.len());
        self.diff_scroll.scroll_to_item(row, ScrollStrategy::Center);
        cx.notify();
    }

    fn copy_hunk(&mut self, hunk: usize, to: Side, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        if f.computing {
            return;
        }
        let Some(h) = f.diff.hunks.get(hunk).cloned() else { return };
        f.history.copy_hunks(&mut f.left.file.lines, &mut f.right.file.lines, &[h], to);
        f.sync_dirty();
        // 同じ番号を選んでおくと、再計算後は次の差分が選択される。
        f.current = Some(hunk);
        self.start_diff(cx);
    }

    fn copy_current(&mut self, to: Side, cx: &mut Context<Self>) {
        match &self.view {
            View::File(f) => {
                if let Some(c) = f.current {
                    self.copy_hunk(c, to, cx);
                }
            }
            View::Folder(_) => self.copy_selected(to, cx),
            View::Empty => {}
        }
    }

    /// すべての差分を `to` の側へコピーする。1 回の操作として元に戻せる。
    fn copy_all(&mut self, to: Side, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        if f.computing || f.diff.hunks.is_empty() {
            return;
        }
        let diff = f.diff.clone();
        f.history.copy_hunks(&mut f.left.file.lines, &mut f.right.file.lines, &diff.hunks, to);
        f.sync_dirty();
        self.start_diff(cx);
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        if f.computing {
            return;
        }
        if f.history.undo(&mut f.left.file.lines, &mut f.right.file.lines).is_none() {
            self.set_status("元に戻す操作がありません", cx);
            return;
        }
        f.sync_dirty();
        self.start_diff(cx);
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        if f.computing {
            return;
        }
        if f.history.redo(&mut f.left.file.lines, &mut f.right.file.lines).is_none() {
            self.set_status("やり直す操作がありません", cx);
            return;
        }
        f.sync_dirty();
        self.start_diff(cx);
    }

    /// 未保存の側をすべて保存する。保存したファイル名を返す。
    fn save_all(&mut self) -> Result<Vec<String>, String> {
        let View::File(f) = &mut self.view else { return Ok(Vec::new()) };
        let mut saved = Vec::new();
        for side in [Side::Left, Side::Right] {
            if !f.doc(side).dirty {
                continue;
            }
            f.doc(side).write().map_err(|e| e.to_string())?;
            f.mark_saved(side);
            saved.push(f.doc(side).file_name());
        }
        Ok(saved)
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let msg = match self.save_all() {
            Err(e) => e,
            Ok(saved) if saved.is_empty() => "変更はありません".to_string(),
            Ok(saved) => format!("保存しました: {}", saved.join(", ")),
        };
        self.set_status(msg, cx);
    }

    /// `side` の内容を別のファイルへ保存する。以後はそのファイルを比較対象にする。
    fn save_as(&mut self, side: Side, cx: &mut Context<Self>) {
        let View::File(f) = &self.view else { return };
        let doc = f.doc(side);
        let dir = doc.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let rx = cx.prompt_for_new_path(&dir, Some(&doc.file_name()));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                this.update(cx, |this, cx| this.finish_save_as(side, path, cx)).ok();
            }
        })
        .detach();
    }

    fn finish_save_as(&mut self, side: Side, path: PathBuf, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        if let Err(e) = diff_core::save(&path, &f.doc(side).file) {
            self.set_status(e.to_string(), cx);
            return;
        }
        f.doc_mut(side).path = path.clone();
        f.mark_saved(side);
        let name = f.doc(side).file_name();
        match side {
            Side::Left => self.left_path = Some(path),
            Side::Right => self.right_path = Some(path),
        }
        self.set_status(format!("保存しました: {name}"), cx);
    }

    /// `side` を読み直す。`enc` が None なら文字コードを自動判定する。その側の履歴は捨てる。
    fn reopen(&mut self, side: Side, enc: Option<&'static Encoding>, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        if f.computing {
            return;
        }
        let path = f.doc(side).path.clone();
        match Document::load(&path, enc) {
            Ok(doc) => {
                *f.doc_mut(side) = doc;
                f.history.reset(side);
                f.mark_saved(side);
                self.start_diff(cx);
            }
            Err(e) => self.set_status(e.to_string(), cx),
        }
    }

    /// フォルダ一覧へ戻る。未保存の確認は済んでいること。
    fn back_now(&mut self, cx: &mut Context<Self>) {
        if let View::File(f) = &mut self.view {
            if let Some(folder) = f.back.take() {
                self.view = View::Folder(*folder);
                self.refresh(cx);
            }
        }
    }

    /// 開発時の確認用。DIFFHARBOR_DEBUG_ACTIONS に並べた操作を、差分の計算が終わるたびに 1 つ実行する。
    fn run_debug_action(&mut self, cx: &mut Context<Self>) {
        while let Some(action) = self.debug_actions.pop_front() {
            match action.as_str() {
                "copy_right" => return self.copy_current(Side::Right, cx),
                "copy_left" => return self.copy_current(Side::Left, cx),
                "copy_all_right" => return self.copy_all(Side::Right, cx),
                "copy_all_left" => return self.copy_all(Side::Left, cx),
                "undo" => return self.undo(cx),
                "redo" => return self.redo(cx),
                "reopen_right_sjis" => return self.reopen(Side::Right, Some(SHIFT_JIS), cx),
                "reopen_right_utf8" => return self.reopen(Side::Right, Some(UTF_8), cx),
                // 差分の再計算を伴わない操作は、続けて次を実行する。
                "save" => self.save(cx),
                "next" => self.step_hunk(1, cx),
                other => eprintln!("unknown debug action: {other}"),
            }
        }
    }

    // ---- rendering ----

    fn button(id: &'static str, label: &str, enabled: bool) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded_md()
            .bg(rgb(if enabled { BUTTON } else { PANEL }))
            .border_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(if enabled { TEXT } else { BUTTON_DISABLED_TEXT }))
            .text_size(px(12.))
            .when(enabled, |b| b.cursor_pointer().hover(|s| s.bg(rgb(BUTTON_HOVER))))
            .child(label.to_string())
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let in_file = matches!(self.view, View::File(_));
        let in_folder = matches!(self.view, View::Folder(_));
        let has_back = matches!(&self.view, View::File(f) if f.back.is_some());
        let has_hunks = matches!(&self.view, View::File(f) if !f.diff.hunks.is_empty());
        let can_copy = has_hunks || matches!(&self.view, View::Folder(f) if f.selected.is_some());
        let can_undo = matches!(&self.view, View::File(f) if f.history.can_undo());
        let can_redo = matches!(&self.view, View::File(f) if f.history.can_redo());
        let can_save = matches!(&self.view, View::File(f) if f.left.dirty || f.right.dirty);

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(Self::button("open-left", "左を開く…", true).on_click(cx.listener(|t, _: &ClickEvent, window, cx| t.pick(Side::Left, window, cx))))
            .child(Self::button("open-right", "右を開く…", true).on_click(cx.listener(|t, _: &ClickEvent, window, cx| t.pick(Side::Right, window, cx))))
            .child(div().w(px(1.)).h(px(18.)).bg(rgb(BORDER)))
            .when(has_back, |d| {
                d.child(Self::button("back", "← フォルダへ", true).on_click(cx.listener(|t, _: &ClickEvent, window, cx| t.guard(Pending::Back, window, cx))))
            })
            .when(in_file, |d| {
                d.child(Self::button("first", "⤒ 先頭", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.goto_edge_hunk(false, cx)))))
                    .child(Self::button("prev", "↑ 前の差分", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.step_hunk(-1, cx)))))
                    .child(Self::button("next", "↓ 次の差分", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.step_hunk(1, cx)))))
                    .child(Self::button("last", "⤓ 末尾", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.goto_edge_hunk(true, cx)))))
            })
            .when(in_file || in_folder, |d| {
                d.child(Self::button("copy-r", "左→右", can_copy).when(can_copy, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.copy_current(Side::Right, cx)))))
                    .child(Self::button("copy-l", "右→左", can_copy).when(can_copy, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.copy_current(Side::Left, cx)))))
                    .when(in_file, |d| {
                        d.child(Self::button("copy-all-r", "すべて右へ", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.copy_all(Side::Right, cx)))))
                            .child(Self::button("copy-all-l", "すべて左へ", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.copy_all(Side::Left, cx)))))
                    })
            })
            .when(in_file, |d| {
                d.child(Self::button("undo", "元に戻す", can_undo).when(can_undo, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.undo(cx)))))
                    .child(Self::button("redo", "やり直す", can_redo).when(can_redo, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.redo(cx)))))
                    .child(Self::button("save", "保存", can_save).when(can_save, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.save(cx)))))
            })
            .when(in_folder, |d| {
                d.child(Self::button("refresh", "再比較", true).on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.refresh(cx))))
                    .child(Self::button("only-diffs", if self.only_diffs { "✓ 差分のみ" } else { "差分のみ" }, true).on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.toggle_only_diffs(cx))))
            })
            .into_any_element()
    }

    fn path_header(left: String, right: String) -> AnyElement {
        let cell = |s: String| {
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .px_2()
                .py_1()
                .text_size(px(12.))
                .text_color(rgb(TEXT))
                .child(s)
        };
        div()
            .flex()
            .flex_row()
            .bg(rgb(PATH_BAR))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(cell(left))
            .child(div().w(px(44.)).flex_none())
            .child(cell(right))
            .into_any_element()
    }

    /// 片側のペインで本文に使える幅（行番号欄を除く）。
    fn pane_text_width(window: &Window) -> f32 {
        ((f32::from(window.viewport_size().width) - LOC_W - MID_W) / 2.0 - GUTTER_W).max(1.0)
    }

    /// 横方向にスクロールする。`dx` はホイールの移動量で、正なら左へ戻る。
    fn scroll_horizontally(&mut self, dx: f32, window: &Window, cx: &mut Context<Self>) {
        let char_w = self.char_w.unwrap_or(FONT_SIZE * 0.6);
        let View::File(f) = &mut self.view else { return };
        let content = f.max_line_px.unwrap_or(0.0) + char_w * 2.0;
        let max = (content - Self::pane_text_width(window)).max(0.0);
        let next = (f.h_scroll - dx).clamp(0.0, max);
        if next != f.h_scroll {
            f.h_scroll = next;
            cx.notify();
        }
    }

    fn render_file(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // 半角 1 文字の幅を一度だけ測る。横スクロールの範囲の計算に使う。
        let char_w = *self.char_w.get_or_insert_with(|| {
            let ts = window.text_system();
            let id = ts.resolve_font(&font(MONO_FONT));
            ts.em_advance(id, px(FONT_SIZE)).map(f32::from).unwrap_or(FONT_SIZE * 0.6)
        });
        let pane_w = Self::pane_text_width(window);
        let View::File(f) = &mut self.view else { return div().into_any_element() };

        let content_w = *f.max_line_px.get_or_insert_with(|| {
            f.left
                .lines
                .iter()
                .chain(f.right.lines.iter())
                .map(|l| {
                    let (narrow, wide) = display_width(l);
                    narrow as f32 * char_w + wide as f32 * FONT_SIZE
                })
                .fold(0.0, f32::max)
        }) + char_w * 2.0;
        // ウィンドウを広げた時に、スクロール位置が範囲外に残らないようにする。
        f.h_scroll = f.h_scroll.min((content_w - pane_w).max(0.0));

        // 未保存の印は先頭に付ける。長いパスが切れても見えるようにする。
        let mark = |d: &Document| if d.dirty { "● " } else { "" };
        let header = Self::path_header(
            format!("{}{}", mark(&f.left), f.left.path.display()),
            format!("{}{}", mark(&f.right), f.right.path.display()),
        );

        let body = if f.computing {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(DIM))
                .child("比較中…")
                .into_any_element()
        } else {
            uniform_list(
                "diff-rows",
                f.diff.rows.len(),
                cx.processor(|this, range: Range<usize>, _w, cx| this.render_rows(range, cx)),
            )
            .track_scroll(self.diff_scroll.clone())
            .flex_1()
            .w_full()
            .into_any_element()
        };

        // 横スクロールの位置を示すバー。内容がペインに収まる時はつまみを出さない。
        let h_bar = |h_scroll: f32| {
            let track = div().flex_1().min_w_0().h_full().overflow_hidden();
            let track = if content_w > pane_w {
                let thumb_w = (pane_w * pane_w / content_w).max(24.0);
                let offset = h_scroll / (content_w - pane_w) * (pane_w - thumb_w);
                track.child(div().ml(px(offset)).w(px(thumb_w)).h_full().rounded_sm().bg(rgb(SCROLL_THUMB)))
            } else {
                track
            };
            div().flex().flex_row().flex_1().min_w_0().child(div().w(px(GUTTER_W)).flex_none()).child(track)
        };
        let h_scroll = f.h_scroll;
        let scroll_bar = div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(6.))
            .bg(rgb(PANEL))
            .child(h_bar(h_scroll))
            .child(div().w(px(MID_W)).flex_none())
            .child(h_bar(h_scroll));

        let pane_status = |d: &Document| {
            let mut text = format!("{}　　{}　　{} 行", d.encoding_label(), d.eol_label(), d.lines.len());
            if d.lossy {
                text.push_str("　　変換できない文字があります（保存できません）");
            }
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .px_2()
                .py_1()
                .text_size(px(12.))
                .text_color(rgb(if d.lossy { STATUS_LEFT_ONLY } else { TEXT }))
                .child(text)
        };
        let status = div()
            .flex()
            .flex_row()
            .flex_none()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(pane_status(&f.left))
            .child(div().w(px(MID_W)).flex_none())
            .child(pane_status(&f.right));

        let panes = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| {
                let d = ev.delta.pixel_delta(px(ROW_H));
                let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                if dx != 0.0 && dx.abs() > dy.abs() {
                    this.scroll_horizontally(dx, window, cx);
                }
            }))
            .child(header)
            .child(body)
            .child(scroll_bar)
            .child(status);

        div()
            .flex()
            .flex_row()
            .size_full()
            .child(self.render_location(cx))
            .child(panes)
            .into_any_element()
    }

    /// 左端の位置ペイン。左右のファイルを 1 本ずつのバーで表し、差分の位置と表示範囲を示す。
    fn render_location(&self, cx: &mut Context<Self>) -> AnyElement {
        let View::File(f) = &self.view else { return div().into_any_element() };
        let (diff, current, computing) = (f.diff.clone(), f.current, f.computing);
        // 一覧のスクロール位置。前のフレームで確定した値を使う。
        let (top_row, visible_rows) = {
            let state = self.diff_scroll.0.borrow();
            let offset_y = -f32::from(state.base_handle.offset().y);
            let view_h = f32::from(state.base_handle.bounds().size.height);
            (offset_y / ROW_H, view_h / ROW_H)
        };
        let geom = self.loc_geom.clone();

        let bars = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let avail = f32::from(bounds.size.height) - LOC_PAD * 2.0;
                let lay = if computing {
                    location::LocationLayout::default()
                } else {
                    location::layout(&diff, current, avail, top_row, visible_rows)
                };
                let y0 = bounds.origin.y + px(LOC_PAD);
                geom.set((lay.bar_h > 0.0).then_some((f32::from(y0), lay.bar_h)));
                if lay.bar_h <= 0.0 {
                    return;
                }
                for (i, bands) in [&lay.left, &lay.right].into_iter().enumerate() {
                    let x = bounds.origin.x + px(8.0 + i as f32 * 17.0);
                    window.paint_quad(quad(
                        Bounds::new(point(x - px(1.), y0 - px(1.)), size(px(13.), px(lay.bar_h + 2.0))),
                        Corners::all(px(0.)),
                        rgb(BG),
                        Edges::all(px(1.)),
                        rgb(BORDER),
                        BorderStyle::default(),
                    ));
                    for b in bands {
                        window.paint_quad(fill(
                            Bounds::new(point(x, y0 + px(b.top)), size(px(11.), px(b.height))),
                            rgb(b.color),
                        ));
                    }
                }
                // 表示範囲の枠。
                window.paint_quad(quad(
                    Bounds::new(point(bounds.origin.x + px(3.), y0 + px(lay.view_top)), size(px(LOC_W - 7.0), px(lay.view_h))),
                    Corners::all(px(1.)),
                    Hsla::from(rgba(0x3B78D826)),
                    Edges::all(px(1.)),
                    rgb(ACCENT),
                    BorderStyle::default(),
                ));
            },
        )
        .size_full();

        div()
            .id("location-pane")
            .w(px(LOC_W))
            .flex_none()
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| this.jump_to_location(f32::from(ev.position.y), cx)),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                // 押したまま動かすと、位置に追従してスクロールする。
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.jump_to_location(f32::from(ev.position.y), cx);
                }
            }))
            .child(bars)
            .into_any_element()
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if let View::File(f) = &mut self.view {
            for ix in range.clone() {
                f.ensure_spans(ix);
            }
        }
        let View::File(f) = &self.view else { return Vec::new() };
        let h_scroll = f.h_scroll;
        range
            .filter_map(|ix| {
                let row = *f.diff.rows.get(ix)?;
                let selected = row.hunk.is_some() && row.hunk == f.current;
                let (lbg, rbg) = match (row.kind, selected) {
                    (RowKind::Equal, _) => (None, None),
                    (RowKind::Changed, false) => (Some(DIFF), Some(DIFF)),
                    (RowKind::Changed, true) => (Some(DIFF_SEL), Some(DIFF_SEL)),
                    (RowKind::LeftOnly, false) => (Some(DIFF), Some(FILLER)),
                    (RowKind::LeftOnly, true) => (Some(DIFF_SEL), Some(FILLER_SEL)),
                    (RowKind::RightOnly, false) => (Some(FILLER), Some(DIFF)),
                    (RowKind::RightOnly, true) => (Some(FILLER_SEL), Some(DIFF_SEL)),
                };
                let word_bg = if selected { WORD_SEL } else { WORD };
                let spans = f.span_cache.get(&ix).cloned();

                let side = |doc: &Document, line: Option<usize>, bg: Option<u32>, spans: &[Range<usize>]| {
                    let (num, text, highlights) = match line {
                        Some(i) => {
                            let (text, spans) = expand_tabs(&doc.lines[i], spans);
                            ((i + 1).to_string(), text, spans)
                        }
                        None => (String::new(), String::new(), Vec::new()),
                    };
                    let style = HighlightStyle { background_color: Some(rgb(word_bg).into()), ..Default::default() };
                    let text = StyledText::new(text).with_highlights(highlights.into_iter().map(|r| (r, style)));
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_hidden()
                        .when_some(bg, |d, c| d.bg(rgb(c)))
                        .child(
                            div()
                                .w(px(GUTTER_W))
                                .flex_none()
                                .pr_2()
                                .text_right()
                                .h_full()
                                .bg(rgb(GUTTER))
                                .text_color(rgb(DIM))
                                .child(num),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_hidden()
                                .child(div().ml(px(4.0 - h_scroll)).whitespace_nowrap().child(text)),
                        )
                };
                let (ls, rs): (&[Range<usize>], &[Range<usize>]) = match &spans {
                    Some(s) => (&s.0, &s.1),
                    None => (&[], &[]),
                };

                let first_of_hunk = row.hunk.filter(|&h| f.diff.hunks[h].first_row == ix);
                let mut mid = div()
                    .w(px(MID_W))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .bg(rgb(PANEL));
                if let Some(h) = first_of_hunk {
                    let small = |id: (&'static str, usize), label: &'static str| {
                        div()
                            .id(id)
                            .w(px(16.))
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .bg(rgb(BUTTON))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .hover(|s| s.bg(rgb(BUTTON_HOVER)).border_color(rgb(ACCENT)))
                            .child(label)
                    };
                    mid = mid
                        .child(small(("to-right", ix), "›").on_click(cx.listener(move |t, _: &ClickEvent, _, cx| t.copy_hunk(h, Side::Right, cx))))
                        .child(small(("to-left", ix), "‹").on_click(cx.listener(move |t, _: &ClickEvent, _, cx| t.copy_hunk(h, Side::Left, cx))));
                }

                Some(
                    div()
                        .id(("row", ix))
                        .flex()
                        .flex_row()
                        .w_full()
                        .h(px(ROW_H))
                        .font_family(MONO_FONT)
                        .text_size(px(FONT_SIZE))
                        .line_height(px(ROW_H))
                        .text_color(rgb(TEXT))
                        // 差分行をクリックすると、その差分を選択する。
                        .when_some(row.hunk, |d, h| {
                            d.on_click(cx.listener(move |t, _: &ClickEvent, _, cx| t.select_hunk(h, cx)))
                        })
                        .child(side(&f.left, row.left, lbg, ls))
                        .child(mid)
                        .child(side(&f.right, row.right, rbg, rs))
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_folder(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let View::Folder(f) = &self.view else { return div().into_any_element() };
        let header = Self::path_header(f.left.display().to_string(), f.right.display().to_string());
        let count = f.visible.len();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(
                uniform_list(
                    "folder-rows",
                    count,
                    cx.processor(|this, range: Range<usize>, _w, cx| this.render_folder_rows(range, cx)),
                )
                .track_scroll(self.folder_scroll.clone())
                .flex_1()
                .w_full(),
            )
            .into_any_element()
    }

    fn render_folder_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let View::Folder(f) = &self.view else { return Vec::new() };
        range
            .filter_map(|ix| {
                let entry = f.entries.get(*f.visible.get(ix)?)?;
                let selected = f.selected == Some(ix);
                let (label, color) = match entry.status {
                    Status::Identical => ("同一", STATUS_IDENTICAL),
                    Status::Different => ("差異あり", STATUS_DIFFERENT),
                    Status::LeftOnly => ("左のみ", STATUS_LEFT_ONLY),
                    Status::RightOnly => ("右のみ", STATUS_RIGHT_ONLY),
                    Status::TypeMismatch => ("種類が異なる", STATUS_MISMATCH),
                };
                let name = format!("{}{}", if entry.is_dir { "▾ " } else { "   " }, entry.name);
                Some(
                    div()
                        .id(("entry", ix))
                        .flex()
                        .flex_row()
                        .items_center()
                        .w_full()
                        .h(px(ROW_H + 4.))
                        .text_size(px(13.))
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .when(selected, |d| d.bg(rgb(ROW_SELECTED)))
                        .when(!selected, |d| d.hover(|s| s.bg(rgb(ROW_HOVER))))
                        .on_click(cx.listener(move |t, e: &ClickEvent, _, cx| {
                            t.select_entry(ix, cx);
                            if e.click_count() >= 2 {
                                t.open_entry(ix, cx);
                            }
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .pl(px(10. + entry.depth as f32 * 18.))
                                .when(entry.is_dir, |d| d.text_color(rgb(DIR_NAME)))
                                .child(name),
                        )
                        .child(div().w(px(120.)).flex_none().text_color(rgb(color)).child(label))
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl Render for DiffHarbor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match (&self.left_path, &self.right_path) {
            (Some(l), Some(r)) => format!(
                "DiffHarbor — {} ↔ {}",
                l.file_name().unwrap_or_default().to_string_lossy(),
                r.file_name().unwrap_or_default().to_string_lossy()
            ),
            _ => "DiffHarbor".to_string(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }

        // 右下に出す「差分 n / m」。
        let counter = match &self.view {
            View::File(f) if !f.computing => match (f.current, f.diff.hunks.len()) {
                (_, 0) => String::new(),
                (Some(c), n) => format!("差分 {} / {}", c + 1, n),
                (None, n) => format!("差分 {n} 件"),
            },
            _ => String::new(),
        };

        let body = match &self.view {
            View::Empty => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(DIM))
                .child("ファイルまたはフォルダを左右に開いて比較します")
                .into_any_element(),
            View::Folder(_) => self.render_folder(cx),
            View::File(_) => self.render_file(window, cx),
        };

        div()
            .key_context("DiffHarbor")
            .track_focus(&self.focus)
            .on_action(cx.listener(|t, _: &OpenLeft, window, cx| t.pick(Side::Left, window, cx)))
            .on_action(cx.listener(|t, _: &OpenRight, window, cx| t.pick(Side::Right, window, cx)))
            .on_action(cx.listener(|t, _: &Quit, window, cx| t.guard(Pending::Quit, window, cx)))
            .on_action(cx.listener(|t, _: &CopyAllToRight, _, cx| t.copy_all(Side::Right, cx)))
            .on_action(cx.listener(|t, _: &CopyAllToLeft, _, cx| t.copy_all(Side::Left, cx)))
            .on_action(cx.listener(|t, _: &Redo, _, cx| t.redo(cx)))
            .on_action(cx.listener(|t, _: &SaveLeftAs, _, cx| t.save_as(Side::Left, cx)))
            .on_action(cx.listener(|t, _: &SaveRightAs, _, cx| t.save_as(Side::Right, cx)))
            .on_action(cx.listener(|t, _: &ReopenLeftAuto, w, cx| t.guard(Pending::Reopen(Side::Left, None), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenLeftUtf8, w, cx| t.guard(Pending::Reopen(Side::Left, Some(UTF_8)), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenLeftShiftJis, w, cx| t.guard(Pending::Reopen(Side::Left, Some(SHIFT_JIS)), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenLeftEucJp, w, cx| t.guard(Pending::Reopen(Side::Left, Some(EUC_JP)), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenRightAuto, w, cx| t.guard(Pending::Reopen(Side::Right, None), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenRightUtf8, w, cx| t.guard(Pending::Reopen(Side::Right, Some(UTF_8)), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenRightShiftJis, w, cx| t.guard(Pending::Reopen(Side::Right, Some(SHIFT_JIS)), w, cx)))
            .on_action(cx.listener(|t, _: &ReopenRightEucJp, w, cx| t.guard(Pending::Reopen(Side::Right, Some(EUC_JP)), w, cx)))
            .on_action(cx.listener(|t, _: &NextDiff, _, cx| t.step_hunk(1, cx)))
            .on_action(cx.listener(|t, _: &PrevDiff, _, cx| t.step_hunk(-1, cx)))
            .on_action(cx.listener(|t, _: &FirstDiff, _, cx| t.goto_edge_hunk(false, cx)))
            .on_action(cx.listener(|t, _: &LastDiff, _, cx| t.goto_edge_hunk(true, cx)))
            .on_action(cx.listener(|t, _: &CopyToRight, _, cx| t.copy_current(Side::Right, cx)))
            .on_action(cx.listener(|t, _: &CopyToLeft, _, cx| t.copy_current(Side::Left, cx)))
            .on_action(cx.listener(|t, _: &Save, _, cx| t.save(cx)))
            .on_action(cx.listener(|t, _: &Undo, _, cx| t.undo(cx)))
            .on_action(cx.listener(|t, _: &Refresh, _, cx| t.refresh(cx)))
            .on_action(cx.listener(|t, _: &Back, window, cx| t.guard(Pending::Back, window, cx)))
            .on_action(cx.listener(|t, _: &CloseWindow, window, cx| t.guard(Pending::CloseWindow, window, cx)))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.render_toolbar(cx))
            .child(div().flex_1().min_h_0().child(body))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .px_3()
                    .py_1()
                    .bg(rgb(PANEL))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(12.))
                    .text_color(rgb(DIM))
                    .child(self.status.clone())
                    .child(div().flex_none().text_color(rgb(TEXT)).child(counter)),
            )
    }
}
