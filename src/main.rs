#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod clean;
mod demo;
mod fmt;
mod history;
mod model;
mod prefs;
mod recognize;
mod scan;
mod theme;
mod treemap;
mod ui;
mod volume;

use gpui::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo");

    let stored = prefs::load();
    prefs::apply_language(stored.language);
    if let Ok(v) = std::env::var("SMD_LANG") {
        fmt::set_language(v.to_lowercase().starts_with("zh"));
    }

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("disk-cleaner".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|cx| ui::Root::new(demo, stored.appearance, stored.language, cx))
            },
        )
        .expect("could not open window");
        cx.activate(true);
    });
}
