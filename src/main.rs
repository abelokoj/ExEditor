#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod color;
mod color_picker;
mod dialogs;
mod document;
mod fonts;
mod formats;
mod native;
mod preferences;
mod services;
mod session;
mod settings;
mod ui;

use gpui_kit::QuitMode;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            if let Err(error) = fonts::register(cx) {
                app::show_error(&error);
            }
            cx.set_quit_mode(QuitMode::LastWindowClosed);
            let paths: Vec<_> = std::env::args_os()
                .skip(1)
                .map(std::path::PathBuf::from)
                .collect();
            let restored = app::restore_session(cx);
            if paths.is_empty() {
                if !restored {
                    app::open_document(None, cx);
                }
            } else {
                for path in paths {
                    app::open_document(Some(path), cx);
                }
            }
            cx.activate(true);
        });
}
