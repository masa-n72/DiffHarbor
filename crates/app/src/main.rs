mod app;
mod display;
mod document;
mod folder;
mod location;
mod theme;

use std::path::PathBuf;

use gpui::{point, prelude::*, px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    // Usage: diffharbor [LEFT RIGHT]  (two files or two folders)
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();

    Application::new().run(move |cx: &mut App| {
        app::init(cx);
        // 開発時の確認用。DIFFHARBOR_BOUNDS="x,y,幅,高さ" でウィンドウの位置と大きさを指定できる。
        let bounds = std::env::var("DIFFHARBOR_BOUNDS")
            .ok()
            .and_then(|v| {
                let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                match n.as_slice() {
                    [x, y, w, h] => Some(Bounds::new(point(px(*x), px(*y)), size(px(*w), px(*h)))),
                    _ => None,
                }
            })
            .unwrap_or_else(|| Bounds::centered(None, size(px(1400.), px(860.)), cx));
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("DiffHarbor".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| app::DiffHarbor::new(args, window, cx)),
        )
        .unwrap();
        // 開発時の確認用。DIFFHARBOR_NO_ACTIVATE を設定すると、起動時に前面へ出ない。
        if std::env::var_os("DIFFHARBOR_NO_ACTIVATE").is_none() {
            cx.activate(true);
        }
    });
}
