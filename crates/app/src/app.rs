//! The DiffHarbor main view: toolbar, folder comparison and side-by-side file diff.

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use gpui::{
    actions, div, prelude::*, px, rgb, uniform_list, AnyElement, App, ClickEvent, Context,
    FocusHandle, KeyBinding, Menu, MenuItem, PathPromptOptions, Render, ScrollStrategy,
    SharedString, Stateful, UniformListScrollHandle, Window,
};

use diff_core::{apply_hunk, diff_lines, DiffResult, RowKind};

use crate::{
    document::Document,
    theme::*,
    folder::{compare_dirs, copy_entry, FolderEntry, Status},
};

actions!(
    diffharbor,
    [
        Quit,
        OpenLeft,
        OpenRight,
        NextDiff,
        PrevDiff,
        CopyToRight,
        CopyToLeft,
        Save,
        Undo,
        Refresh,
        Back,
        CloseWindow,
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenLeft, None),
        KeyBinding::new("cmd-shift-o", OpenRight, None),
        KeyBinding::new("alt-down", NextDiff, None),
        KeyBinding::new("alt-up", PrevDiff, None),
        KeyBinding::new("alt-right", CopyToRight, None),
        KeyBinding::new("alt-left", CopyToLeft, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-z", Undo, None),
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
                MenuItem::separator(),
                MenuItem::action("右へコピー", CopyToRight),
                MenuItem::action("左へコピー", CopyToLeft),
                MenuItem::separator(),
                MenuItem::action("元に戻す", Undo),
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
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

struct Snapshot {
    left: Vec<String>,
    right: Vec<String>,
    left_dirty: bool,
    right_dirty: bool,
}

struct FileView {
    left: Document,
    right: Document,
    diff: DiffResult,
    current: Option<usize>,
    undo: Vec<Snapshot>,
    back: Option<Box<FolderView>>,
}

impl FileView {
    fn new(left: &Path, right: &Path, back: Option<Box<FolderView>>) -> anyhow::Result<Self> {
        let (left, right) = (Document::load(left)?, Document::load(right)?);
        let diff = diff_lines(&left.lines, &right.lines);
        let current = (!diff.hunks.is_empty()).then_some(0);
        Ok(Self { left, right, diff, current, undo: Vec::new(), back })
    }

    fn rediff(&mut self) {
        self.diff = diff_lines(&self.left.lines, &self.right.lines);
        self.current = match (self.current, self.diff.hunks.len()) {
            (_, 0) => None,
            (Some(c), n) => Some(c.min(n - 1)),
            (None, _) => Some(0),
        };
    }

    fn snapshot(&mut self) {
        self.undo.push(Snapshot {
            left: self.left.lines.clone(),
            right: self.right.lines.clone(),
            left_dirty: self.left.dirty,
            right_dirty: self.right.dirty,
        });
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
        };
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

    fn pick(&mut self, side: Side, cx: &mut Context<Self>) {
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
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        match side {
                            Side::Left => this.left_path = Some(path),
                            Side::Right => this.right_path = Some(path),
                        }
                        this.compare(cx);
                    })
                    .ok();
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
                    View::File(f) => {
                        let n = f.diff.hunks.len();
                        self.status = if n == 0 {
                            "ファイルは同一です".into()
                        } else {
                            format!("{n} 件の差分").into()
                        };
                        self.scroll_to_current();
                    }
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
                self.scroll_to_current();
                cx.notify();
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

    fn step_hunk(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let View::File(f) = &mut self.view {
            let n = f.diff.hunks.len();
            if n == 0 {
                return;
            }
            let next = match f.current {
                None => 0,
                Some(c) => (c as isize + delta).clamp(0, n as isize - 1) as usize,
            };
            f.current = Some(next);
            self.scroll_to_current();
            cx.notify();
        }
    }

    fn copy_hunk(&mut self, hunk: usize, to: Side, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        let Some(h) = f.diff.hunks.get(hunk).cloned() else { return };
        f.snapshot();
        match to {
            Side::Right => {
                apply_hunk(&mut f.right.lines, h.right, &f.left.lines, h.left);
                f.right.dirty = true;
            }
            Side::Left => {
                apply_hunk(&mut f.left.lines, h.left, &f.right.lines, h.right);
                f.left.dirty = true;
            }
        }
        f.current = Some(hunk);
        f.rediff();
        let remaining = f.diff.hunks.len();
        self.scroll_to_current();
        self.set_status(
            if remaining == 0 { "差分はありません".to_string() } else { format!("残り {remaining} 件の差分") },
            cx,
        );
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

    fn undo(&mut self, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        let Some(s) = f.undo.pop() else {
            self.set_status("元に戻す操作がありません", cx);
            return;
        };
        f.left.lines = s.left;
        f.right.lines = s.right;
        f.left.dirty = s.left_dirty;
        f.right.dirty = s.right_dirty;
        f.rediff();
        self.scroll_to_current();
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let View::File(f) = &mut self.view else { return };
        let mut saved = Vec::new();
        let mut error = None;
        for doc in [&mut f.left, &mut f.right] {
            if !doc.dirty {
                continue;
            }
            match doc.save() {
                Ok(()) => saved.push(
                    doc.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                ),
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            }
        }
        let msg = match error {
            Some(e) => e,
            None if saved.is_empty() => "変更はありません".to_string(),
            None => format!("保存しました: {}", saved.join(", ")),
        };
        self.set_status(msg, cx);
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if let View::File(f) = &mut self.view {
            if f.left.dirty || f.right.dirty {
                self.set_status("未保存の変更があります。⌘S で保存してください", cx);
                return;
            }
            if let Some(folder) = f.back.take() {
                self.view = View::Folder(*folder);
                self.refresh(cx);
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
        let can_undo = matches!(&self.view, View::File(f) if !f.undo.is_empty());
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
            .child(Self::button("open-left", "左を開く…", true).on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.pick(Side::Left, cx))))
            .child(Self::button("open-right", "右を開く…", true).on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.pick(Side::Right, cx))))
            .child(div().w(px(1.)).h(px(18.)).bg(rgb(BORDER)))
            .when(has_back, |d| {
                d.child(Self::button("back", "← フォルダへ", true).on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.back(cx))))
            })
            .when(in_file, |d| {
                d.child(Self::button("prev", "↑ 前の差分", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.step_hunk(-1, cx)))))
                    .child(Self::button("next", "↓ 次の差分", has_hunks).when(has_hunks, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.step_hunk(1, cx)))))
            })
            .when(in_file || in_folder, |d| {
                d.child(Self::button("copy-r", "左→右", can_copy).when(can_copy, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.copy_current(Side::Right, cx)))))
                    .child(Self::button("copy-l", "右→左", can_copy).when(can_copy, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.copy_current(Side::Left, cx)))))
            })
            .when(in_file, |d| {
                d.child(Self::button("undo", "元に戻す", can_undo).when(can_undo, |b| b.on_click(cx.listener(|t, _: &ClickEvent, _, cx| t.undo(cx)))))
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

    fn render_file(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let View::File(f) = &self.view else { return div().into_any_element() };
        let mark = |d: &Document| if d.dirty { " ●" } else { "" };
        let header = Self::path_header(
            format!("{}{}", f.left.path.display(), mark(&f.left)),
            format!("{}{}", f.right.path.display(), mark(&f.right)),
        );
        let count = f.diff.rows.len();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(
                uniform_list(
                    "diff-rows",
                    count,
                    cx.processor(|this, range: Range<usize>, _w, cx| this.render_rows(range, cx)),
                )
                .track_scroll(self.diff_scroll.clone())
                .flex_1()
                .w_full(),
            )
            .into_any_element()
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let View::File(f) = &self.view else { return Vec::new() };
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
                let side = |doc: &Document, line: Option<usize>, bg: Option<u32>| {
                    let (num, text) = match line {
                        Some(i) => ((i + 1).to_string(), doc.lines[i].replace('\t', "    ")),
                        None => (String::new(), String::new()),
                    };
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
                                .w(px(48.))
                                .flex_none()
                                .pr_2()
                                .text_right()
                                .h_full()
                                .bg(rgb(GUTTER))
                                .text_color(rgb(DIM))
                                .child(num),
                        )
                        .child(div().flex_1().pl_1().whitespace_nowrap().child(text))
                };

                let first_of_hunk = row.hunk.filter(|&h| f.diff.hunks[h].first_row == ix);
                let mut mid = div()
                    .w(px(44.))
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
                        .text_size(px(12.))
                        .line_height(px(ROW_H))
                        .text_color(rgb(TEXT))
                        .child(side(&f.left, row.left, lbg))
                        .child(mid)
                        .child(side(&f.right, row.right, rbg))
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
            View::File(_) => self.render_file(cx),
        };

        div()
            .key_context("DiffHarbor")
            .track_focus(&self.focus)
            .on_action(cx.listener(|t, _: &OpenLeft, _, cx| t.pick(Side::Left, cx)))
            .on_action(cx.listener(|t, _: &OpenRight, _, cx| t.pick(Side::Right, cx)))
            .on_action(cx.listener(|t, _: &NextDiff, _, cx| t.step_hunk(1, cx)))
            .on_action(cx.listener(|t, _: &PrevDiff, _, cx| t.step_hunk(-1, cx)))
            .on_action(cx.listener(|t, _: &CopyToRight, _, cx| t.copy_current(Side::Right, cx)))
            .on_action(cx.listener(|t, _: &CopyToLeft, _, cx| t.copy_current(Side::Left, cx)))
            .on_action(cx.listener(|t, _: &Save, _, cx| t.save(cx)))
            .on_action(cx.listener(|t, _: &Undo, _, cx| t.undo(cx)))
            .on_action(cx.listener(|t, _: &Refresh, _, cx| t.refresh(cx)))
            .on_action(cx.listener(|t, _: &Back, _, cx| t.back(cx)))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.render_toolbar(cx))
            .child(div().flex_1().min_h_0().child(body))
            .child(
                div()
                    .px_3()
                    .py_1()
                    .bg(rgb(PANEL))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(12.))
                    .text_color(rgb(DIM))
                    .child(self.status.clone()),
            )
    }
}
