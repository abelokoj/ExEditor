//! ExEditor native desktop application and document services.
mod accessories;
mod app;
mod clipboard_html;
mod color;
mod color_picker;
mod completion;
mod dialogs;
mod dictionaries;
mod document;
mod editing;
mod editor;
mod fonts;
mod formats;
mod instance;
mod keyboard;
mod language_tools;
mod names;
mod palette;
mod panes;
mod preferences;
mod proofing;
mod services;
mod session;
mod settings;
mod syntax;
mod ui;
mod window_style;

pub use app::App;
use nsh_core::prelude::*;

#[native_module("exeditor/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// Starts the native platform without a browser or JavaScript VM.
pub fn run() -> std::process::ExitCode {
    let paths: Vec<_> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect();
    let instance = match instance::start(&paths) {
        Ok(instance::Startup::Forwarded) => return std::process::ExitCode::SUCCESS,
        Ok(instance::Startup::Primary(instance)) => std::sync::Arc::new(instance),
        Err(error) => {
            app::show_error(&error);
            return std::process::ExitCode::FAILURE;
        }
    };
    app::configure(paths, instance);
    ui::configure_scrollbars();
    #[cfg(target_os = "linux")]
    use nsh_platform_linux as platform;
    #[cfg(target_os = "macos")]
    use nsh_platform_macos as platform;
    #[cfg(windows)]
    use nsh_platform_windows as platform;
    let result = platform::run_app(
        platform::AppSource::Rust(fonts::configure_root(platform::Root::of::<App>())),
        platform::AppOptions {
            title: "ExEditor".to_string(),
            app_id: "com.abelokoj.ExEditor".to_string(),
            size: (760, 620),
            data_dir: document::data_dir().ok(),
            appearance: match preferences::Preferences::load()
                .unwrap_or_default()
                .appearance
            {
                preferences::Appearance::Light => nsh_core::platform::Appearance::Light,
                preferences::Appearance::Dark => nsh_core::platform::Appearance::Dark,
                preferences::Appearance::System => nsh_core::platform::Appearance::System,
            },
            ..Default::default()
        },
    );
    #[cfg(target_os = "linux")]
    {
        result.into()
    }
    #[cfg(not(target_os = "linux"))]
    {
        result
    }
}
