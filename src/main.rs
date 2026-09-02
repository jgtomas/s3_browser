mod app;
mod aws;
mod models;
mod ui;

use gpui::{
    AppContext, Application, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowOptions, actions, px, size,
};
use gpui_component::Root;

use ui::MainWindow;

actions!(s3_downloader, [Quit]);

fn main() {
    Application::new().run(|cx| {
        gpui_component::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "S3 Downloader".into(),
            items: vec![MenuItem::action("Quit", Quit)],
        }]);

        let bounds = Bounds::centered(None, size(px(560.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("S3 Downloader".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(480.0), px(520.0))),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| MainWindow::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("failed to open S3 Downloader window");

        cx.activate(true);
    });
}
