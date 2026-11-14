mod app;
mod document;
mod folder;

use std::path::PathBuf;

use gpui::{prelude::*, px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    // Usage: diffharbor [LEFT RIGHT]  (two files or two folders)
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();

    Application::new().run(move |cx: &mut App| {
        app::init(cx);
        let bounds = Bounds::centered(None, size(px(1400.), px(860.)), cx);
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
        cx.activate(true);
    });
}
