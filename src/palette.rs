//! Searchable native command launcher, quick open, and shortcut reference.
use crate::{
    dialogs::{Commands, MenuEntry},
    ui::{self, Palette, px},
};
use nsh_core::{desktop::WindowKind, prelude::*};
use std::rc::Rc;

pub fn open(entries: Vec<MenuEntry>, commands: Commands, colors: Palette) {
    open_with_title(
        entries,
        commands,
        colors,
        "Command Palette",
        "Type a command…",
        false,
    );
}
pub fn open_files(entries: Vec<MenuEntry>, commands: Commands, colors: Palette) {
    open_with_title(
        entries,
        commands,
        colors,
        "Quick Open",
        "Search open and recent files…",
        false,
    );
}
pub fn open_shortcuts(entries: Vec<MenuEntry>, commands: Commands, colors: Palette) {
    open_with_title(
        entries,
        commands,
        colors,
        "Keyboard Shortcuts",
        "Search commands or keybindings…",
        true,
    );
}
fn open_with_title(
    entries: Vec<MenuEntry>,
    commands: Commands,
    colors: Palette,
    title: &'static str,
    placeholder: &'static str,
    allow_disabled: bool,
) {
    let entries = Rc::new(entries);
    ui::show_panel_with_colors(
        "command-palette",
        &format!("ExEditor {title}"),
        480.,
        450.,
        WindowKind::Popup,
        colors,
        move || {
            let query = signal(String::new());
            let filter_entries = entries.clone();
            let run_entries = entries.clone();
            let queue = commands.clone();
            ui::surface(colors)
                .p(px(10.))
                .gap(px(8.))
                .child(ui::heading(title))
                .child(
                    ui::command_list(
                        move || {
                            filtered_with(&filter_entries, &query.get(), allow_disabled)
                                .into_iter()
                                .map(|index| {
                                    let entry = &filter_entries[index];
                                    nsh_base::CommandItem::new(
                                        index.to_string(),
                                        entry.label.clone(),
                                    )
                                    .keywords([entry.shortcut.clone()])
                                    .checked(entry.checked)
                                })
                                .collect::<Vec<_>>()
                        },
                        title,
                        true,
                        colors,
                    )
                    .filterable(false)
                    .placeholder(placeholder)
                    .on_query_change(move |value: &SharedString| query.set(value.to_string()))
                    .render_item({
                        let entries = entries.clone();
                        move |item: &nsh_base::CommandEntry| {
                            let shortcut = item
                                .id
                                .parse::<usize>()
                                .ok()
                                .and_then(|index| entries.get(index))
                                .map(|entry| entry.shortcut.clone())
                                .unwrap_or_default();
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(10.))
                                .min_w(px(0.))
                                .child(
                                    text(item.label.clone())
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(px(12.)),
                                )
                                .child(
                                    text(shortcut)
                                        .text_size(px(11.))
                                        .font_family(crate::fonts::default_plain_font())
                                        .opacity(0.75)
                                        .flex_shrink_0(),
                                )
                        }
                    })
                    .on_select(move |id: &SharedString| {
                        if let Some(command) = id
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| run_entries.get(index))
                            .filter(|entry| entry.enabled)
                            .and_then(|entry| entry.command.clone())
                        {
                            queue.borrow_mut().push_back(command);
                            ui::close_panel("command-palette");
                        }
                    })
                    .on_cancel(|| ui::close_panel("command-palette")),
                )
                .child(ui::hint(
                    "Up/Down to navigate · Enter to run · Escape to close",
                    colors,
                ))
                .into_element()
        },
    );
}
#[cfg_attr(not(test), allow(dead_code))]
fn filtered(entries: &[MenuEntry], query: &str) -> Vec<usize> {
    filtered_with(entries, query, false)
}
fn filtered_with(entries: &[MenuEntry], query: &str, allow_disabled: bool) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            ((allow_disabled || entry.enabled)
                && entry.command.is_some()
                && matches_query(&format!("{} {}", entry.label, entry.shortcut), query))
            .then_some(index)
        })
        .collect()
}
fn matches_query(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| label.contains(word))
}

#[cfg(test)]
mod tests {
    use super::{filtered, filtered_with, matches_query};
    #[test]
    fn palette_search_matches_command_names_and_keybindings_together() {
        use crate::{app::Command, dialogs::MenuEntry};
        let mut disabled = MenuEntry::item(
            "Scroll line up",
            "Ctrl+Up",
            Command::ScrollEditor(-1, false),
        );
        disabled.enabled = false;
        let entries = [
            MenuEntry::item("New window", "Ctrl+Shift+N", Command::New),
            MenuEntry::item("New tab", "Ctrl+N", Command::NewTab),
            disabled,
        ];
        assert_eq!(filtered(&entries, "ctrl+shift+n"), [0]);
        assert_eq!(filtered(&entries, "new ctrl+n"), [1]);
        assert!(filtered(&entries, "ctrl+up").is_empty());
        assert_eq!(filtered_with(&entries, "ctrl+up", true), [2]);
        assert!(filtered(&entries, "missing").is_empty());
    }
    #[test]
    fn command_search_matches_case_insensitive_words_and_ignores_extra_spaces() {
        assert!(matches_query(
            "Format: Increase Font Size",
            "  FONT   size "
        ));
        assert!(matches_query("Édit: Copy", "édit"));
        assert!(matches_query("File: Save", ""));
        assert!(!matches_query("File: Save", "save as"));
    }
}
