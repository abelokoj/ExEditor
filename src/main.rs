#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard_html;
mod color;
mod color_picker;
mod dialogs;
mod document;
mod editing;
mod emoji;
mod fonts;
mod formats;
mod instance;
mod keyboard;
mod language_tools;
mod names;
mod native;
mod palette;
mod preferences;
mod proofing;
mod services;
mod session;
mod settings;
mod syntax;
mod ui;

use gpui_kit::QuitMode;

fn main() {
    let paths: Vec<_> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect();
    let instance = match instance::start(&paths) {
        Ok(instance::Startup::Forwarded) => return,
        Ok(instance::Startup::Primary(instance)) => std::rc::Rc::new(instance),
        Err(error) => {
            app::show_error(&error);
            return;
        }
    };
    let receiver = instance.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            app::listen_for_files(receiver, cx);
            if let Ok(preferences) = preferences::Preferences::load() {
                ui::set_scale_percent(preferences.interface_scale);
            }
            if let Err(error) = fonts::register(cx) {
                app::show_error(&error);
            }
            cx.set_quit_mode(QuitMode::LastWindowClosed);
            let restored = app::restore_session(cx);
            if paths.is_empty() {
                if !restored {
                    app::open_document(None, cx);
                }
            } else {
                app::open_paths(paths, cx);
            }
            cx.activate(true);
        });
    drop(instance);
}
