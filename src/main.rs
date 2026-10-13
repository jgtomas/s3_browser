mod app;
mod aws;
mod models;
mod ui;

use gpui_kit::assets::Assets;
use gpui_kit::{
    AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds, WindowOptions,
    actions, px, size,
};

use ui::MainWindow;

actions!(
    s3_downloader,
    [Quit, NarrowSidebar, WidenSidebar, ResetSidebar]
);

fn main() {
    gpui_kit::application().with_assets(Assets).run(|cx| {
        gpui_kit::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-alt-left", NarrowSidebar, Some("MainWindow")),
            KeyBinding::new("cmd-alt-right", WidenSidebar, Some("MainWindow")),
            KeyBinding::new("cmd-alt-0", ResetSidebar, Some("MainWindow")),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "S3 Downloader".into(),
                disabled: false,
                items: vec![MenuItem::action("Quit", Quit)],
            },
            Menu {
                name: "View".into(),
                disabled: false,
                items: vec![
                    MenuItem::action("Narrow Sidebar", NarrowSidebar),
                    MenuItem::action("Widen Sidebar", WidenSidebar),
                    MenuItem::action("Reset Sidebar Width", ResetSidebar),
                ],
            },
        ]);

        let bounds = Bounds::centered(None, size(px(960.0), px(640.0)), cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("S3 Downloader".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(900.0), px(600.0))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("failed to open S3 Downloader window");

        cx.activate(true);
    });
}
