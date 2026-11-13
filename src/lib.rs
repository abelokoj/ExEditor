//! ExEditor native desktop application and document services.
mod accessories;
mod app;
mod clipboard_html;
mod color;
mod color_picker;
mod completion;
mod dialogs;
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
mod preferences;
mod proofing;
mod services;
mod session;
mod settings;
mod syntax;
mod ui;

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
    #[cfg(target_os = "linux")]
    use nsh_platform_linux as platform;
    #[cfg(target_os = "macos")]
    use nsh_platform_macos as platform;
    #[cfg(windows)]
    use nsh_platform_windows as platform;
    let result = platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<App>()),
        platform::AppOptions {
            title: "ExEditor".to_string(),
            app_id: "com.abelokoj.ExEditor".to_string(),
            size: (760, 620),
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
