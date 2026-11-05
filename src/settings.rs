//! TextEdit's two-tab Settings window, backed by real persisted preferences.
use crate::{
    app::{Command, show_error},
    dialogs::{Commands, FormKind},
    document::TextEncoding,
    preferences::{Appearance, Preferences},
    ui::{Palette, px},
};
use gpui_kit::component::{
    Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    checkbox::Checkbox,
    input::{Input, InputState},
    radio::Radio,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    NewDocument,
    OpenAndSave,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FontEditor {
    Plain,
    Rich,
}
struct Settings {
    draft: Preferences,
    rich_font: Entity<InputState>,
    rich_size: Entity<InputState>,
    plain_font: Entity<InputState>,
    plain_size: Entity<InputState>,
    recovery: Entity<InputState>,
    tab: Tab,
    font_editor: Option<FontEditor>,
    error: Option<String>,
    commands: Commands,
    colors: Palette,
}

pub fn open(preferences: Preferences, commands: Commands, colors: Palette, cx: &mut App) {
    let options = WindowOptions {
        kind: WindowKind::Dialog,
        window_bounds: Some(WindowBounds::centered(size(px(590.), px(570.)), cx)),
        titlebar: None,
        app_owns_titlebar_drag: true,
        window_decorations: Some(WindowDecorations::Client),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        window.set_window_title("ExEditor Settings");
        cx.new(|cx| Settings {
            rich_font: input("Rich text font", &preferences.rich_font, window, cx),
            rich_size: input(
                "Rich text font size",
                &preferences.rich_size.to_string(),
                window,
                cx,
            ),
            plain_font: input("Plain text font", &preferences.plain_font, window, cx),
            plain_size: input(
                "Plain text font size",
                &preferences.plain_size.to_string(),
                window,
                cx,
            ),
            recovery: input(
                "Additional recovery checkpoint interval in seconds",
                &preferences.autosave_seconds.to_string(),
                window,
                cx,
            ),
            draft: preferences,
            tab: Tab::NewDocument,
            font_editor: None,
            error: None,
            commands,
            colors,
        })
    });
    if let Err(error) = result {
        show_error(&error);
    }
}

fn input(
    label: &'static str,
    value: &str,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(label)
            .default_value(value.to_owned())
    })
}

impl Settings {
    fn button(&self, id: &'static str, label: &'static str, cx: &App) -> Button {
        let c = self.colors;
        Button::new(id)
            .label(label)
            .with_size(Size::Small)
            .rounded(px(4.))
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(rgb(c.field).into())
                    .foreground(rgb(c.ink).into())
                    .hover(rgb(c.toolbar).into())
                    .shadow(false),
            )
            .h(px(24.))
            .text_size(px(12.))
    }
    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        update: fn(&mut Preferences, bool),
        cx: &Context<Self>,
    ) -> Checkbox {
        Checkbox::new(id)
            .label(label)
            .checked(checked)
            .with_size(Size::Small)
            .text_size(px(12.))
            .text_color(rgb(self.colors.ink))
            .on_change(cx.listener(move |this, checked, _, cx| {
                update(&mut this.draft, *checked);
                this.error = None;
                cx.notify();
            }))
    }
    fn heading(&self, text: &'static str) -> Div {
        div()
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text)
    }
    fn font_row(&self, kind: FontEditor, cx: &mut Context<Self>) -> Div {
        let (label, id, font, size) = match kind {
            FontEditor::Plain => (
                "Plain text font:",
                "change-plain-font",
                &self.plain_font,
                &self.plain_size,
            ),
            FontEditor::Rich => (
                "Rich text font:",
                "change-rich-font",
                &self.rich_font,
                &self.rich_size,
            ),
        };
        let summary = format!("{} {}", font.read(cx).value(), size.read(cx).value());
        let expanded = self.font_editor == Some(kind);
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_h(px(27.))
                    .child(div().w(px(108.)).child(label))
                    .child(
                        self.button(id, if expanded { "Done" } else { "Change…" }, cx)
                            .w(px(79.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.font_editor = if this.font_editor == Some(kind) {
                                    None
                                } else {
                                    Some(kind)
                                };
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1().child(summary)),
            )
            .when(expanded, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .pl(px(118.))
                        .child(
                            div()
                                .w(px(185.))
                                .child(Input::new(font).with_size(Size::Small)),
                        )
                        .child(
                            div()
                                .w(px(58.))
                                .child(Input::new(size).with_size(Size::Small)),
                        )
                        .child(div().text_color(rgb(self.colors.muted)).child("pt")),
                )
            })
    }
    fn new_document(&self, cx: &mut Context<Self>) -> Div {
        let c = self.colors;
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(self.heading("Format"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .child("Use the Format menu to choose settings for an open document."),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(54.))
                            .mt(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(9.))
                                    .w(px(150.))
                                    .child(
                                        Radio::new("default-rich-text")
                                            .label("Rich text")
                                            .checked(self.draft.rich_text)
                                            .with_size(Size::Small)
                                            .text_size(px(12.))
                                            .on_change(cx.listener(|this, _, _, cx| {
                                                this.draft.rich_text = true;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Radio::new("default-plain-text")
                                            .label("Plain text")
                                            .checked(!self.draft.rich_text)
                                            .with_size(Size::Small)
                                            .text_size(px(12.))
                                            .on_change(cx.listener(|this, _, _, cx| {
                                                this.draft.rich_text = false;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(self.checkbox(
                                "default-wrap",
                                "Wrap to page",
                                self.draft.wrap_to_page,
                                |p, value| p.wrap_to_page = value,
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(self.heading("Font"))
                    .child(self.font_row(FontEditor::Plain, cx))
                    .child(self.font_row(FontEditor::Rich, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(11.))
                    .child(self.heading("Options"))
                    .child(
                        div()
                            .flex()
                            .gap(px(22.))
                            .child(
                                div()
                                    .w(px(250.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(self.checkbox(
                                        "default-spelling",
                                        "Check spelling as you type",
                                        self.draft.spellcheck,
                                        |p, value| p.spellcheck = value,
                                        cx,
                                    ))
                                    .child(self.checkbox(
                                        "default-ruler",
                                        "Show ruler",
                                        self.draft.show_ruler,
                                        |p, value| p.show_ruler = value,
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(self.checkbox(
                                        "default-quotes",
                                        "Smart quotes",
                                        self.draft.smart_quotes,
                                        |p, value| p.smart_quotes = value,
                                        cx,
                                    ))
                                    .child(self.checkbox(
                                        "default-dashes",
                                        "Smart dashes",
                                        self.draft.smart_dashes,
                                        |p, value| p.smart_dashes = value,
                                        cx,
                                    ))
                                    .child(self.checkbox(
                                        "default-links",
                                        "Smart links",
                                        self.draft.automatic_links,
                                        |p, value| p.automatic_links = value,
                                        cx,
                                    )),
                            ),
                    ),
            )
            .child(div().text_size(px(11.)).text_color(rgb(c.muted)).child(
                "Defaults apply to new documents. Existing documents keep their formatting.",
            ))
    }
    fn open_and_save(&self, cx: &mut Context<Self>) -> Div {
        let c = self.colors;
        let encodings = [
            TextEncoding::Utf8,
            TextEncoding::Utf8Bom,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
            TextEncoding::Windows1252,
        ];
        let appearances = [
            (Appearance::System, "System"),
            (Appearance::Light, "Light"),
            (Appearance::Dark, "Dark"),
        ];
        div().flex().flex_col().gap(px(22.))
            .child(div().flex().flex_col().gap(px(10.))
                .child(self.heading("Plain Text File Encoding"))
                .child(div().text_size(px(12.)).child("Use this encoding when saving new plain-text documents:"))
                .children(encodings.into_iter().enumerate().map(|(index, encoding)| {
                    Radio::new(("default-encoding", index)).label(encoding.label())
                        .checked(self.draft.default_encoding == encoding).with_size(Size::Small).text_size(px(12.))
                        .on_change(cx.listener(move |this, _, _, cx| {
                            this.draft.default_encoding = encoding; cx.notify();
                        }))
                })))
            .child(div().flex().flex_col().gap(px(9.))
                .child(self.heading("Document Recovery"))
                .child(div().flex().items_center().gap(px(10.))
                    .child("Additional checkpoint every")
                    .child(div().w(px(64.)).child(Input::new(&self.recovery).with_size(Size::Small)))
                    .child("seconds"))
                .child(div().text_size(px(11.)).text_color(rgb(c.muted))
                    .child("Unsaved buffers are protected as you edit and restored automatically at startup. This interval adds periodic checkpoints; it never changes your saved files.")))
            .child(div().flex().flex_col().gap(px(10.))
                .child(self.heading("Appearance"))
                .child(div().flex().gap(px(22.)).children(appearances.into_iter().enumerate().map(|(index, (appearance, label))| {
                    Radio::new(("default-appearance", index)).label(label)
                        .checked(self.draft.appearance == appearance).with_size(Size::Small).text_size(px(12.))
                        .on_change(cx.listener(move |this, _, _, cx| { this.draft.appearance = appearance; cx.notify(); }))
                }))))
    }
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let defaults = Preferences::default();
        let fields = [
            (&self.rich_font, defaults.rich_font.clone()),
            (&self.rich_size, defaults.rich_size.to_string()),
            (&self.plain_font, defaults.plain_font.clone()),
            (&self.plain_size, defaults.plain_size.to_string()),
            (&self.recovery, defaults.autosave_seconds.to_string()),
        ];
        for (field, value) in fields {
            field.update(cx, |state, cx| state.set_value(value, window, cx));
        }
        self.draft = defaults;
        self.font_editor = None;
        self.error = None;
        cx.notify();
    }
    fn values(&self, cx: &App) -> Result<Vec<String>, String> {
        let rich_font = self.rich_font.read(cx).value().to_string();
        let plain_font = self.plain_font.read(cx).value().to_string();
        for font in [&rich_font, &plain_font] {
            if font.trim().is_empty() || font.contains('\0') || font.encode_utf16().count() >= 32 {
                return Err("Choose a font name containing 1–31 characters.".into());
            }
        }
        let rich_size = font_size(self.rich_size.read(cx).value().as_ref())?;
        let plain_size = font_size(self.plain_size.read(cx).value().as_ref())?;
        let recovery = self
            .recovery
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .map_err(|_| "Enter a whole number of seconds for document recovery.".to_string())?;
        if !(5..=3600).contains(&recovery) {
            return Err("The recovery interval must be between 5 and 3600 seconds.".into());
        }
        let appearance = match self.draft.appearance {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        };
        let encoding = match self.draft.default_encoding {
            TextEncoding::Utf8 => "Utf8",
            TextEncoding::Utf8Bom => "Utf8Bom",
            TextEncoding::Utf16Le => "Utf16Le",
            TextEncoding::Utf16Be => "Utf16Be",
            TextEncoding::Windows1252 => "Windows1252",
        };
        Ok(vec![
            self.draft.rich_text.to_string(),
            rich_font.trim().to_owned(),
            rich_size.to_string(),
            plain_font.trim().to_owned(),
            plain_size.to_string(),
            self.draft.wrap_to_page.to_string(),
            self.draft.show_ruler.to_string(),
            self.draft.spellcheck.to_string(),
            self.draft.smart_quotes.to_string(),
            self.draft.smart_dashes.to_string(),
            self.draft.automatic_links.to_string(),
            recovery.to_string(),
            appearance.into(),
            encoding.into(),
        ])
    }
    fn submit(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        match self.values(cx) {
            Ok(values) => {
                self.commands
                    .borrow_mut()
                    .push_back(Command::Submit(FormKind::Preferences, values));
                self.error = None;
                if close {
                    window.remove_window();
                } else {
                    cx.notify();
                }
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui-kit components size text in rem; scale it with View > Interface Size.
        window.set_rem_size(crate::ui::rem_size());
        let c = self.colors;
        let tab = self.tab;
        let tabs = [
            (Tab::NewDocument, "New Document", "settings-new-document"),
            (Tab::OpenAndSave, "Open and Save", "settings-open-save"),
        ];
        let panel = match tab {
            Tab::NewDocument => self.new_document(cx),
            Tab::OpenAndSave => self.open_and_save(cx),
        };
        div()
            .id("exeditor-settings")
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(c.chrome))
            .rounded(px(8.))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(c.border))
            .text_color(rgb(c.ink))
            .text_size(px(12.))
            .font_family("Segoe UI")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                } else if event.keystroke.key == "enter" && event.keystroke.modifiers.control {
                    this.submit(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .h(px(34.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px(px(12.))
                    .border_b_1()
                    .border_color(rgb(c.border))
                    .child(
                        div()
                            .w(px(70.))
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(
                                Button::new("close-settings")
                                    .accessibility_label("Close Settings")
                                    .tooltip("Close Settings")
                                    .w(px(14.))
                                    .h(px(14.))
                                    .p_0()
                                    .rounded(px(7.))
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(rgb(0xff5f57).into())
                                            .foreground(rgb(0x772018).into())
                                            .hover(rgb(0xf3544c).into())
                                            .shadow(false),
                                    )
                                    .on_click(|_, window, _| window.remove_window()),
                            )
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded(px(7.))
                                    .bg(rgb(c.muted))
                                    .opacity(0.5),
                            )
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded(px(7.))
                                    .bg(rgb(c.muted))
                                    .opacity(0.5),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(13.))
                            .child("Settings")
                            .window_control_area(WindowControlArea::Drag),
                    )
                    .child(div().w(px(70.)).h_full()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .relative()
                    .px(px(24.))
                    .pt(px(33.))
                    .child(
                        div()
                            .id("settings-panel")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .border_1()
                            .border_color(rgb(c.border))
                            .rounded(px(6.))
                            .bg(rgb(c.toolbar))
                            .px(px(25.))
                            .pt(px(29.))
                            .pb(px(19.))
                            .child(panel),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(20.))
                            .left(px(0.))
                            .right(px(0.))
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .flex()
                                    .p(px(1.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(rgb(c.border))
                                    .bg(rgb(c.toolbar))
                                    .children(tabs.into_iter().map(|(value, label, id)| {
                                        let selected = value == tab;
                                        self.button(id, label, cx)
                                            .w(px(143.))
                                            .h(px(26.))
                                            .custom(
                                                ButtonCustomVariant::new(cx)
                                                    .color(
                                                        rgb(if selected {
                                                            c.field
                                                        } else {
                                                            c.toolbar
                                                        })
                                                        .into(),
                                                    )
                                                    .foreground(rgb(c.ink).into())
                                                    .hover(rgb(c.field).into())
                                                    .shadow(selected),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.tab = value;
                                                this.error = None;
                                                cx.notify();
                                            }))
                                    })),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |root, error| {
                root.child(
                    div()
                        .px(px(27.))
                        .pt(px(9.))
                        .text_size(px(12.))
                        .text_color(rgb(0xd43a35))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(9.))
                    .p(px(18.))
                    .child(
                        self.button("restore-settings", "Restore All Defaults", cx)
                            .on_click(cx.listener(|this, _, window, cx| this.restore(window, cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                self.button("cancel-settings", "Cancel", cx)
                                    .w(px(66.))
                                    .on_click(|_, window, _| window.remove_window()),
                            )
                            .child(
                                self.button("apply-settings", "Apply", cx)
                                    .w(px(66.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.submit(false, window, cx)
                                    })),
                            )
                            .child(
                                self.button("save-settings", "OK", cx)
                                    .w(px(66.))
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(rgb(0x087ef5).into())
                                            .foreground(rgb(0xffffff).into())
                                            .hover(rgb(0x006de0).into())
                                            .shadow(false),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.submit(true, window, cx)
                                    })),
                            ),
                    ),
            )
    }
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
    use super::font_size;
    #[test]
    fn font_size_validation_accepts_boundaries_and_rejects_non_finite_input() {
        assert_eq!(font_size("1"), Ok(1.));
        assert_eq!(font_size("1638"), Ok(1638.));
        for invalid in ["NaN", "inf", "0", "1639", "twelve"] {
            assert!(font_size(invalid).is_err(), "invalid size {invalid}");
        }
    }
}
