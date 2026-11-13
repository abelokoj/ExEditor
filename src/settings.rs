//! Persisted TextEdit defaults and explicitly requested optional font downloads.
use crate::{
    app::Command,
    dialogs::{Commands, FormKind},
    document::TextEncoding,
    preferences::{Appearance, Preferences},
    ui::{self, Palette, px},
};
use nsh_core::{
    desktop::WindowKind,
    element::{SignalEquals, signal_with},
    prelude::*,
};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    NewDocument,
    OpenAndSave,
    Accessories,
}
#[derive(Clone, PartialEq)]
enum DownloadStatus {
    Missing,
    Downloading,
    Available,
    Failed(String),
}
struct Settings {
    draft: Signal<Preferences>,
    prefix: Signal<String>,
    rich_font: Signal<String>,
    rich_size: Signal<String>,
    plain_font: Signal<String>,
    plain_size: Signal<String>,
    recovery: Signal<String>,
    tab: Signal<Tab>,
    accessories: Signal<Vec<DownloadStatus>>,
    error: Signal<String>,
    commands: Commands,
    colors: Palette,
}

pub fn open(preferences: Preferences, commands: Commands, colors: Palette) {
    ui::show_panel(
        "settings",
        "ExEditor Settings",
        620.,
        610.,
        WindowKind::Dialog,
        move || {
            let state = Rc::new(Settings {
                prefix: signal(preferences.untitled_prefix.clone()),
                rich_font: signal(preferences.rich_font.clone()),
                rich_size: signal(preferences.rich_size.to_string()),
                plain_font: signal(preferences.plain_font.clone()),
                plain_size: signal(preferences.plain_size.to_string()),
                recovery: signal(preferences.autosave_seconds.to_string()),
                draft: signal_with(preferences.clone(), SignalEquals::Never),
                tab: signal(Tab::NewDocument),
                accessories: signal(
                    crate::accessories::manifest()
                        .packs
                        .iter()
                        .map(|pack| {
                            if crate::fonts::accessory_available(&pack.id) {
                                DownloadStatus::Available
                            } else {
                                DownloadStatus::Missing
                            }
                        })
                        .collect(),
                ),
                error: signal(String::new()),
                commands: commands.clone(),
                colors,
            });
            let submit = state.clone();
            on_key("mod+enter", move |_| submit.submit(true));
            let content = state.clone();
            let restore = state.clone();
            let apply = state.clone();
            let save = state.clone();
            ui::surface(colors)
                .child(ui::heading("Settings"))
                .child(
                    ui::radio_group("Settings sections")
                        .flex_row()
                        .gap(px(12.))
                        .children(
                            [
                                (Tab::NewDocument, "New Document"),
                                (Tab::OpenAndSave, "Open and Save"),
                                (Tab::Accessories, "Accessories"),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(index, (tab, label))| {
                                let selected = state.tab;
                                ui::radio(
                                    format!("settings-tab-{index}"),
                                    label,
                                    move || selected.get() == tab,
                                    colors,
                                    move |_| selected.set(tab),
                                )
                            }),
                        ),
                )
                .child(scroll().flex_1().min_h(px(0.)).child(dynamic(
                    move || match content.tab.get() {
                        Tab::NewDocument => content.new_document(),
                        Tab::OpenAndSave => content.open_and_save(),
                        Tab::Accessories => content.accessories(),
                    },
                )))
                .child(ui::error(state.error, colors))
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            ui::button("restore-settings", "Restore All Defaults", 12.)
                                .on_press(move || restore.restore()),
                        )
                        .child(
                            view()
                                .flex_row()
                                .gap(px(8.))
                                .child(
                                    ui::control("cancel-settings", "Cancel", 66., false, colors)
                                        .on_press(|| ui::close_panel("settings")),
                                )
                                .child(
                                    ui::control("apply-settings", "Apply", 66., false, colors)
                                        .on_press(move || apply.submit(false)),
                                )
                                .child(
                                    ui::control("save-settings", "OK", 66., true, colors)
                                        .on_press(move || save.submit(true)),
                                ),
                        ),
                )
                .into_element()
        },
    );
}

impl Settings {
    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        read: fn(&Preferences) -> bool,
        update: fn(&mut Preferences, bool),
    ) -> impl IntoElement {
        let draft = self.draft;
        let error = self.error;
        ui::checkbox(
            id,
            label,
            move || draft.with(read),
            self.colors,
            move |checked| {
                draft.update(|preferences| update(preferences, checked));
                error.set(String::new());
            },
        )
    }
    fn new_document(self: &Rc<Self>) -> AnyElement {
        let rich = self.draft;
        let plain = self.draft;
        let prefix = self.prefix;
        let queue = self.commands.clone();
        view()
            .gap(px(14.))
            .child(ui::heading("Document Name"))
            .child(ui::row(
                "New document name prefix:",
                ui::input("New document name prefix", self.prefix, self.colors),
            ))
            .child(ui::hint(move || name_examples(&prefix.get()), self.colors))
            .child(ui::heading("Format"))
            .child(
                ui::radio_group("Default document format")
                    .flex_row()
                    .gap(px(24.))
                    .child(ui::radio(
                        "default-rich-text",
                        "Rich text",
                        move || rich.with(|p| p.rich_text),
                        self.colors,
                        move |_| rich.update(|p| p.rich_text = true),
                    ))
                    .child(ui::radio(
                        "default-plain-text",
                        "Plain text",
                        move || plain.with(|p| !p.rich_text),
                        self.colors,
                        move |_| plain.update(|p| p.rich_text = false),
                    )),
            )
            .child(self.checkbox(
                "default-wrap",
                "Wrap to page",
                |p| p.wrap_to_page,
                |p, v| p.wrap_to_page = v,
            ))
            .child(ui::heading("Font"))
            .child(ui::row(
                "Plain text font:",
                ui::input("Plain text font", self.plain_font, self.colors),
            ))
            .child(ui::row(
                "Plain text size (pt):",
                ui::input("Plain text font size", self.plain_size, self.colors),
            ))
            .child(ui::row(
                "Rich text font:",
                ui::input("Rich text font", self.rich_font, self.colors),
            ))
            .child(ui::row(
                "Rich text size (pt):",
                ui::input("Rich text font size", self.rich_size, self.colors),
            ))
            .child(ui::heading("Options"))
            .child(self.checkbox(
                "default-spelling",
                "Check spelling and grammar",
                |p| p.spellcheck,
                |p, v| p.spellcheck = v,
            ))
            .child(
                ui::button("language-tools-settings", "Language Tools…", 12.)
                    .on_press(move || queue.borrow_mut().push_back(Command::LanguageTools)),
            )
            .child(self.checkbox(
                "default-ruler",
                "Show ruler",
                |p| p.show_ruler,
                |p, v| p.show_ruler = v,
            ))
            .child(self.checkbox(
                "word-suggestions",
                "Suggest words while typing",
                |p| p.autocomplete,
                |p, v| p.autocomplete = v,
            ))
            .child(self.checkbox(
                "default-quotes",
                "Smart quotes",
                |p| p.smart_quotes,
                |p, v| p.smart_quotes = v,
            ))
            .child(self.checkbox(
                "default-dashes",
                "Smart dashes",
                |p| p.smart_dashes,
                |p, v| p.smart_dashes = v,
            ))
            .child(self.checkbox(
                "default-links",
                "Smart links",
                |p| p.automatic_links,
                |p, v| p.automatic_links = v,
            ))
            .child(ui::hint(
                "Defaults apply to new documents. Existing documents keep their formatting.",
                self.colors,
            ))
            .into_element()
    }
    fn open_and_save(self: &Rc<Self>) -> AnyElement {
        let draft = self.draft;
        let appearance = self.draft;
        view().gap(px(14.))
            .child(ui::heading("Plain Text File Encoding"))
            .child(ui::hint("Use this encoding when saving new plain-text documents:", self.colors))
            .child(ui::radio_group("Default plain text encoding")
                .children([TextEncoding::Utf8, TextEncoding::Utf8Bom, TextEncoding::Utf16Le,
                    TextEncoding::Utf16Be, TextEncoding::Windows1252].into_iter().enumerate().map(|(index, encoding)| {
                    ui::radio(format!("default-encoding-{index}"), encoding.label(),
                        move || draft.with(|p| p.default_encoding == encoding), self.colors,
                        move |_| draft.update(|p| p.default_encoding = encoding))
                })))
            .child(ui::heading("Document Recovery"))
            .child(ui::row("Checkpoint interval (seconds):", ui::input("Recovery interval in seconds", self.recovery, self.colors)))
            .child(ui::hint("Unsaved buffers are protected as you edit and restored at startup. Periodic checkpoints never change saved files.", self.colors))
            .child(ui::heading("Appearance"))
            .child(ui::radio_group("Application appearance").flex_row().gap(px(20.))
                .children([(Appearance::System,"System"), (Appearance::Light,"Light"), (Appearance::Dark,"Dark")]
                    .into_iter().enumerate().map(|(index,(value,label))| {
                        ui::radio(format!("default-appearance-{index}"), label,
                            move || appearance.with(|p| p.appearance == value), self.colors,
                            move |_| appearance.update(|p| p.appearance = value))
                    })))
            .into_element()
    }
    fn accessories(self: &Rc<Self>) -> AnyElement {
        let mut panel = view()
            .gap(px(16.))
            .child(ui::heading("Font Accessories"))
            .child(ui::hint(
                "Download the font families you want. Inter is included with ExEditor.",
                self.colors,
            ));
        for (index, pack) in crate::accessories::manifest().packs.iter().enumerate() {
            let state = self.clone();
            let status = self.accessories;
            let colors = self.colors;
            panel = panel.child(
                view()
                    .gap(px(6.))
                    .child(ui::heading(pack.name.clone()))
                    .child(ui::hint(pack.description.clone(), colors))
                    .child(ui::hint(
                        format!("{:.1} MB", pack.download_bytes() as f64 / 1_000_000.),
                        colors,
                    ))
                    .child(dynamic(move || {
                        let current = status.with(|states| states[index].clone());
                        let available = current == DownloadStatus::Available
                            || crate::fonts::accessory_available(&pack.id);
                        let downloading = current == DownloadStatus::Downloading;
                        let label = if available {
                            "Available"
                        } else if downloading {
                            "Downloading…"
                        } else if matches!(current, DownloadStatus::Failed(_)) {
                            "Retry"
                        } else {
                            "Download"
                        };
                        let state = state.clone();
                        view()
                            .gap(px(5.))
                            .child(
                                ui::button(format!("download-{}", pack.id), label, 12.)
                                    .disabled(available || downloading)
                                    .on_press(move || state.download(index)),
                            )
                            .child(ui::hint(
                                match current {
                                    DownloadStatus::Available => {
                                        "Ready to use, including offline.".to_owned()
                                    }
                                    DownloadStatus::Failed(error) => error,
                                    _ => String::new(),
                                },
                                colors,
                            ))
                    })),
            );
        }
        panel
            .child(ui::hint(
                "Choose downloaded fonts in Format > Font or New Document settings.",
                self.colors,
            ))
            .into_element()
    }
    fn download(&self, index: usize) {
        if self.accessories.with_untracked(|states| {
            matches!(
                states[index],
                DownloadStatus::Downloading | DownloadStatus::Available
            )
        }) {
            return;
        }
        let status = self.accessories;
        status.update(|states| states[index] = DownloadStatus::Downloading);
        let pack = &crate::accessories::manifest().packs[index];
        spawn(async move {
            let result = match smol::unblock(move || crate::accessories::download(pack)).await {
                Ok(files) => crate::fonts::register_accessory(pack, files).await,
                Err(error) => Err(error),
            };
            status.update(|states| {
                states[index] = match result {
                    Ok(()) => DownloadStatus::Available,
                    Err(error) => DownloadStatus::Failed(error.to_string()),
                }
            });
        });
    }
    fn restore(&self) {
        let defaults = Preferences::default();
        self.prefix.set(defaults.untitled_prefix.clone());
        self.rich_font.set(defaults.rich_font.clone());
        self.rich_size.set(defaults.rich_size.to_string());
        self.plain_font.set(defaults.plain_font.clone());
        self.plain_size.set(defaults.plain_size.to_string());
        self.recovery.set(defaults.autosave_seconds.to_string());
        self.draft.set(defaults);
        self.error.set(String::new());
    }
    fn values(&self) -> Result<Vec<String>, String> {
        settings_values(
            &self.draft.get_untracked(),
            &self.prefix.get_untracked(),
            &self.rich_font.get_untracked(),
            &self.rich_size.get_untracked(),
            &self.plain_font.get_untracked(),
            &self.plain_size.get_untracked(),
            &self.recovery.get_untracked(),
        )
    }
    fn submit(&self, close: bool) {
        match self.values() {
            Ok(values) => {
                self.commands
                    .borrow_mut()
                    .push_back(Command::Submit(FormKind::Preferences, values));
                self.error.set(String::new());
                if close {
                    ui::close_panel("settings");
                }
            }
            Err(error) => self.error.set(error),
        }
    }
}

fn settings_values(
    draft: &Preferences,
    prefix: &str,
    rich_font: &str,
    rich_size: &str,
    plain_font: &str,
    plain_size: &str,
    recovery: &str,
) -> Result<Vec<String>, String> {
    let prefix = validated_prefix(prefix)?;
    for font in [rich_font, plain_font] {
        if font.trim().is_empty() || font.contains('\0') || font.encode_utf16().count() >= 32 {
            return Err("Choose a font name containing 1–31 characters.".into());
        }
    }
    let rich_size = font_size(rich_size)?;
    let plain_size = font_size(plain_size)?;
    let recovery = recovery
        .trim()
        .parse::<u64>()
        .map_err(|_| "Enter a whole number of seconds for document recovery.".to_owned())?;
    if !(5..=3600).contains(&recovery) {
        return Err("The recovery interval must be between 5 and 3600 seconds.".into());
    }
    let appearance = match draft.appearance {
        Appearance::System => "system",
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    };
    let encoding = match draft.default_encoding {
        TextEncoding::Utf8 => "Utf8",
        TextEncoding::Utf8Bom => "Utf8Bom",
        TextEncoding::Utf16Le => "Utf16Le",
        TextEncoding::Utf16Be => "Utf16Be",
        TextEncoding::Windows1252 => "Windows1252",
    };
    Ok(vec![
        draft.rich_text.to_string(),
        rich_font.trim().into(),
        rich_size.to_string(),
        plain_font.trim().into(),
        plain_size.to_string(),
        draft.wrap_to_page.to_string(),
        draft.show_ruler.to_string(),
        draft.spellcheck.to_string(),
        draft.smart_quotes.to_string(),
        draft.smart_dashes.to_string(),
        draft.automatic_links.to_string(),
        recovery.to_string(),
        appearance.into(),
        encoding.into(),
        prefix,
        draft.autocomplete.to_string(),
    ])
}
fn validated_prefix(value: &str) -> Result<String, String> {
    crate::names::validate_prefix(value).map_err(|error| error.to_string())?;
    Ok(value.trim().to_owned())
}

fn name_examples(value: &str) -> String {
    let prefix = value.trim();
    let prefix = if prefix.is_empty() {
        crate::names::DEFAULT_PREFIX
    } else {
        prefix
    };
    format!("Examples: {prefix} 1, {prefix} 2")
}

fn font_size(value: &str) -> Result<f32, String> {
    let size = value
        .trim()
        .parse::<f32>()
        .map_err(|_| "Enter a numeric font size.".to_string())?;
    if !size.is_finite() || !(1.0..=1638.0).contains(&size) {
        return Err("Font sizes must be between 1 and 1638 points.".into());
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::{font_size, name_examples, validated_prefix};
    #[test]
    fn font_size_validation_accepts_boundaries_and_rejects_non_finite_input() {
        assert_eq!(font_size("1"), Ok(1.));
        assert_eq!(font_size("1638"), Ok(1638.));
        for invalid in ["NaN", "inf", "0", "1639", "twelve"] {
            assert!(font_size(invalid).is_err(), "invalid size {invalid}");
        }
    }
    #[test]
    fn document_name_prefix_is_validated_and_examples_use_the_current_value() {
        assert_eq!(validated_prefix(" Notes "), Ok("Notes".into()));
        assert!(validated_prefix("../Notes").is_err());
        assert_eq!(name_examples(" Notes "), "Examples: Notes 1, Notes 2");
        assert_eq!(name_examples(""), "Examples: Untitled 1, Untitled 2");
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;
    #[test]
    fn native_preferences_keep_the_existing_submit_field_order() {
        let preferences = Preferences::default();
        let values = settings_values(
            &preferences,
            " Notes ",
            "Inter",
            "12",
            "Consolas",
            "11",
            "30",
        )
        .expect("valid settings");
        assert_eq!(values.len(), 16);
        assert_eq!(&values[1..5], ["Inter", "12", "Consolas", "11"]);
        assert_eq!(values[11], "30");
        assert_eq!(values[14], "Notes");
        assert_eq!(values[15], preferences.autocomplete.to_string());
    }
}
