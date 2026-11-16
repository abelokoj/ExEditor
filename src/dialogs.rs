//! Native menus and forms share one panel host beside the document windows.
use crate::{
    app::Command,
    ui::{self, Palette, px, rgb},
};
use nsh_core::{desktop::WindowKind, prelude::*};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

pub type Commands = Rc<RefCell<VecDeque<Command>>>;
#[derive(Clone, Debug)]
pub enum FormKind {
    Font,
    Table,
    Link,
    Spacing,
    FontSize,
    InterfaceScale,
    PageSetup,
    Print {
        printer: String,
        document: std::sync::Arc<crate::formats::RichDocument>,
        page: crate::editor::PageSettings,
    },
    Preferences,
    Find,
    Replace,
    GoToLine,
}
#[derive(Clone)]
pub struct MenuEntry {
    pub label: String,
    pub shortcut: String,
    pub command: Option<Command>,
    pub checked: bool,
    pub enabled: bool,
    checkable: bool,
    preview: Option<Command>,
}
impl MenuEntry {
    pub fn item(label: &str, shortcut: &str, command: Command) -> Self {
        Self {
            label: label.into(),
            shortcut: shortcut.into(),
            command: Some(command),
            checked: false,
            enabled: true,
            checkable: false,
            preview: None,
        }
    }
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            shortcut: String::new(),
            command: None,
            checked: false,
            enabled: false,
            checkable: false,
            preview: None,
        }
    }
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self.checkable = true;
        self
    }
    pub fn previewing(mut self, command: Command) -> Self {
        self.preview = Some(command);
        self
    }
}

pub fn menu(entries: Vec<MenuEntry>, commands: Commands, colors: Palette) {
    let width = entries
        .iter()
        .filter(|e| e.command.is_some())
        .map(|e| {
            text_metrics("12px Inter", &e.label).width.0
                + text_metrics("11px Inter", &e.shortcut).width.0
                + 64.
        })
        .fold(112_f32, f32::max)
        .min(480.);
    let height = (entries
        .iter()
        .map(|e| if e.command.is_some() { 25. } else { 9. })
        .sum::<f32>()
        + 12.)
        .min(500.);
    ui::show_panel(
        "menu",
        "ExEditor menu",
        width,
        height,
        WindowKind::Popup,
        move || {
            let queue = commands.clone();
            if entries.iter().any(|entry| entry.preview.is_some()) {
                let queue = queue.clone();
                cleanup(move || queue.borrow_mut().push_back(Command::EndPreview));
            }
            let focus = element_ref();
            let initial = entries
                .iter()
                .position(|entry| entry.enabled && entry.checked)
                .or_else(|| {
                    entries
                        .iter()
                        .position(|entry| entry.enabled && entry.command.is_some())
                });
            let items: Vec<AnyElement> = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let Some(command) = entry.command.clone() else {
                        return nsh_base::menu_separator()
                            .h(px(1.))
                            .my(px(4.))
                            .bg(rgb(colors.border))
                            .into_element();
                    };
                    let queue = queue.clone();
                    let preview_queue = queue.clone();
                    let focus_queue = queue.clone();
                    let preview = entry.preview.clone().filter(|_| entry.enabled);
                    let focus_preview = preview.clone();
                    let item_ref = if initial == Some(index) {
                        focus
                    } else {
                        element_ref()
                    };
                    item_ref.on_focus_in(move || {
                        if let Some(command) = focus_preview.clone() {
                            focus_queue.borrow_mut().push_back(command);
                        }
                    });
                    let mut item = nsh_base::menu_item()
                        .element_ref(&item_ref)
                        .disabled(!entry.enabled)
                        .accessibility_label(entry.label.clone())
                        .h(px(25.))
                        .px(px(8.))
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(3.))
                        .text_color(rgb(colors.ink))
                        .hover(move |s| s.bg(rgb(colors.accent)).text_color(rgb(colors.on_accent)))
                        .focus(move |s| s.bg(rgb(colors.accent)).text_color(rgb(colors.on_accent)))
                        .active(|s| s.opacity(0.8))
                        .disabled_style(|s| s.opacity(0.5))
                        .indicator_style(|s| s.w(px(18.)).items_center().justify_center())
                        .indicator(text(if entry.checked { "✓" } else { "" }))
                        .child(text(entry.label.clone()).flex_1().text_size(px(12.)))
                        .child(text(entry.shortcut.clone()).text_size(px(11.)))
                        .on_select(move || {
                            queue.borrow_mut().push_back(command.clone());
                            ui::close_panel("menu");
                        });
                    if entry.checkable {
                        item = item.checked(entry.checked);
                    }
                    view()
                        .on_mouse_enter(move |_: &nsh_core::MouseEvent| {
                            if let Some(command) = preview.clone() {
                                preview_queue.borrow_mut().push_back(command);
                            }
                        })
                        .child(item)
                        .into_element()
                })
                .collect();
            effect(move || focus.focus());
            view()
                .flex_1()
                .rounded(px(8.))
                .overflow_hidden()
                .bg(rgb(colors.field))
                .child(
                    nsh_base::menu()
                        .default_open(true)
                        .offset(px(0.))
                        .margin(px(0.))
                        .trigger(view().w_full().h(px(0.)))
                        .popup_style(move |s| {
                            s.w(px(width - 2.))
                                .max_h(px(height - 2.))
                                .p(px(5.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(rgb(colors.border))
                                .bg(rgb(colors.field))
                        })
                        .on_cancel(|| ui::close_panel("menu"))
                        .children(items),
                )
                .into_element()
        },
    );
}

#[derive(Clone, Debug)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub toggle: bool,
}
impl Field {
    pub fn text(label: &str, value: impl ToString) -> Self {
        Self {
            label: label.into(),
            value: value.to_string(),
            toggle: false,
        }
    }
    pub fn toggle(label: &str, value: bool) -> Self {
        Self {
            label: label.into(),
            value: value.to_string(),
            toggle: true,
        }
    }
}
pub fn form(kind: FormKind, title: &str, fields: Vec<Field>, commands: Commands, colors: Palette) {
    let title = title.to_owned();
    let height = (fields.len() as f32 * 43. + 145.).clamp(250., 780.);
    ui::show_panel(
        "form",
        &title.clone(),
        490.,
        height,
        WindowKind::Dialog,
        move || {
            let error = signal(String::new());
            let values: Vec<_> = fields
                .iter()
                .map(|field| signal(field.value.clone()))
                .collect();
            let submit_values = values.clone();
            let kind = kind.clone();
            let submit_kind = kind.clone();
            let queue = commands.clone();
            let submit = Rc::new(move || {
                let values: Vec<_> = submit_values.iter().map(Signal::get_untracked).collect();
                if !valid_form(&submit_kind, &values) {
                    error.set("Enter valid values before continuing.".into());
                    return;
                }
                queue
                    .borrow_mut()
                    .push_back(Command::Submit(submit_kind.clone(), values));
                ui::close_panel("form");
            });
            let key_submit = submit.clone();
            on_key("mod+enter", move |_| key_submit());
            let fields: Vec<_> = fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    let value = values[index];
                    if field.toggle {
                        ui::checkbox(
                            format!("form-toggle-{index}"),
                            field.label.clone(),
                            move || value.get() == "true",
                            colors,
                            move |checked| value.set(checked.to_string()),
                        )
                        .into_element()
                    } else {
                        ui::row(
                            field.label.clone(),
                            ui::input(field.label.clone(), value, colors).auto_focus(
                                index == fields.iter().position(|field| !field.toggle).unwrap_or(0),
                            ),
                        )
                        .into_element()
                    }
                })
                .collect();
            ui::surface(colors)
                .child(ui::heading(title.clone()))
                .child(
                    scroll()
                        .flex_1()
                        .min_h(px(0.))
                        .child(view().gap(px(12.)).children(fields)),
                )
                .child(ui::error(error, colors))
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            ui::control("cancel-form", "Cancel", 80., false, colors)
                                .on_press(|| ui::close_panel("form")),
                        )
                        .child(
                            ui::control(
                                "submit-form",
                                match kind {
                                    FormKind::Find => "Find Next",
                                    FormKind::Replace => "Replace All",
                                    FormKind::Print { .. } => "Print",
                                    _ => "OK",
                                },
                                100.,
                                true,
                                colors,
                            )
                            .on_press(move || submit()),
                        ),
                )
                .into_element()
        },
    );
}

fn valid_form(kind: &FormKind, values: &[String]) -> bool {
    let number = |value: Option<&String>| value.and_then(|s| s.trim().parse::<f32>().ok());
    match kind {
        FormKind::Find | FormKind::Replace => values.first().is_some_and(|s| !s.is_empty()),
        FormKind::Font => {
            values.first().is_some_and(|s| !s.trim().is_empty())
                && number(values.get(1)).is_some_and(|n| n.is_finite() && n > 0. && n <= 1000.)
        }
        FormKind::FontSize => {
            number(values.first()).is_some_and(|n| n.is_finite() && (1. ..=1000.).contains(&n))
        }
        FormKind::InterfaceScale => values
            .first()
            .and_then(|s| s.trim().trim_end_matches('%').trim().parse::<u16>().ok())
            .is_some_and(|n| ui::SCALE_RANGE.contains(&n)),
        FormKind::Table => {
            let rows = values.first().and_then(|s| s.trim().parse::<u16>().ok());
            let cols = values.get(1).and_then(|s| s.trim().parse::<u16>().ok());
            rows.is_some_and(|r| (1..=100).contains(&r))
                && cols.is_some_and(|c| (1..=30).contains(&c))
        }
        FormKind::Link => values.first().is_some_and(|s| {
            let s = s.trim();
            s.starts_with("https://") || s.starts_with("http://") || s.starts_with("mailto:")
        }),
        FormKind::Spacing => {
            number(values.first()).is_some_and(|n| n.is_finite() && (0.5..=10.).contains(&n))
        }
        FormKind::GoToLine => values
            .first()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .is_some_and(|n| n > 0),
        FormKind::PageSetup => {
            let width = number(values.first());
            let height = number(values.get(1));
            let ml = number(values.get(2));
            let mr = number(values.get(3));
            let mt = number(values.get(4));
            let mb = number(values.get(5));
            if let (Some(w), Some(h), Some(l), Some(r), Some(t), Some(b)) =
                (width, height, ml, mr, mt, mb)
            {
                w > 0.
                    && h > 0.
                    && l >= 0.
                    && r >= 0.
                    && t >= 0.
                    && b >= 0.
                    && (l + r) < w
                    && (t + b) < h
            } else {
                false
            }
        }
        FormKind::Preferences => {
            let rf = values.get(1).map(|s| s.trim()).unwrap_or("");
            let rs = number(values.get(2));
            let pf = values.get(3).map(|s| s.trim()).unwrap_or("");
            let ps = number(values.get(4));
            !rf.is_empty()
                && !pf.is_empty()
                && rs.is_some_and(|s| s.is_finite() && s > 0.)
                && ps.is_some_and(|s| s.is_finite() && s > 0.)
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_validation_rejects_empty_search_non_finite_fonts_and_out_of_range_scale() {
        assert!(!valid_form(&FormKind::Find, &[String::new()]));
        assert!(valid_form(&FormKind::FontSize, &["12.5".into()]));
        assert!(!valid_form(&FormKind::FontSize, &["NaN".into()]));
        assert!(valid_form(&FormKind::InterfaceScale, &["150 %".into()]));
        assert!(!valid_form(&FormKind::InterfaceScale, &["301".into()]));
        assert!(!valid_form(&FormKind::Table, &[]));
        assert!(valid_form(&FormKind::Table, &["5".into(), "10".into()]));
        assert!(!valid_form(&FormKind::Table, &["5".into(), "35".into()]));
        assert!(valid_form(
            &FormKind::Link,
            &["https://example.com".into(), "Example".into()]
        ));
        assert!(!valid_form(&FormKind::Link, &["ftp://example.com".into()]));
        assert!(valid_form(&FormKind::GoToLine, &["42".into()]));
        assert!(!valid_form(&FormKind::GoToLine, &["0".into()]));
        assert!(!valid_form(&FormKind::GoToLine, &["abc".into()]));
        assert!(valid_form(&FormKind::Spacing, &["1.5".into()]));
        assert!(!valid_form(&FormKind::Spacing, &["0.2".into()]));
    }
}
