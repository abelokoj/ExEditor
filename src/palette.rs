//! Searchable command palette over the application's existing enabled commands.
use crate::{
    dialogs::{Commands, MenuEntry},
    ui::{self, Palette, px},
};
use gpui_kit::base::Button;
use gpui_kit::component::{
    Sizable as _, Size,
    input::{Input, InputEvent, InputState},
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use std::time::Duration;

struct CommandPalette {
    title: &'static str,
    entries: Vec<MenuEntry>,
    matches: Vec<usize>,
    selection: Option<usize>,
    input: Entity<InputState>,
    commands: Commands,
    colors: Palette,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _filter_subscription: Subscription,
}

pub fn open(entries: Vec<MenuEntry>, commands: Commands, colors: Palette, cx: &mut App) {
    open_with_title(
        entries,
        commands,
        colors,
        "Command Palette",
        "Type a command…",
        cx,
    );
}
pub fn open_files(entries: Vec<MenuEntry>, commands: Commands, colors: Palette, cx: &mut App) {
    open_with_title(
        entries,
        commands,
        colors,
        "Quick Open",
        "Search open and recent files…",
        cx,
    );
}
pub fn open_shortcuts(entries: Vec<MenuEntry>, commands: Commands, colors: Palette, cx: &mut App) {
    open_with_title(
        entries,
        commands,
        colors,
        "Keyboard Shortcuts",
        "Search commands or keybindings...",
        cx,
    );
}
fn open_with_title(
    entries: Vec<MenuEntry>,
    commands: Commands,
    colors: Palette,
    title: &'static str,
    placeholder: &'static str,
    cx: &mut App,
) {
    let options = WindowOptions {
        kind: WindowKind::PopUp,
        titlebar: None,
        window_bounds: Some(WindowBounds::centered(size(px(450.), px(420.)), cx)),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        window.set_window_title(&format!("ExEditor {title}"));
        window.set_rem_size(ui::rem_size());
        let handle = window.window_handle();
        cx.new(|cx: &mut Context<CommandPalette>| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let focus = cx.focus_handle();
            let filter_subscription =
                cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let query = input.read(cx).value().to_string();
                        this.matches = filtered(&this.entries, &query);
                        this.selection = (!this.matches.is_empty()).then_some(0);
                        this.scroll.scroll_to_item(0);
                        cx.notify();
                    }
                });
            input.update(cx, |state, cx| state.focus(window, cx));
            cx.spawn(async move |_, cx| {
                loop {
                    smol::Timer::after(Duration::from_millis(160)).await;
                    let active = handle.update(cx, |_, window, _| {
                        if window.is_window_active() {
                            true
                        } else {
                            window.remove_window();
                            false
                        }
                    });
                    if !matches!(active, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
            let matches = filtered(&entries, "");
            let selection = (!matches.is_empty()).then_some(0);
            CommandPalette {
                title,
                entries,
                matches,
                selection,
                input,
                commands,
                colors,
                focus,
                scroll: ScrollHandle::new(),
                _filter_subscription: filter_subscription,
            }
        })
    });
    if let Err(error) = result {
        crate::app::show_error(&error);
    }
}

impl CommandPalette {
    fn run(&self, index: usize, window: &mut Window) {
        let Some(entry) = self
            .matches
            .get(index)
            .and_then(|index| self.entries.get(*index))
        else {
            return;
        };
        if entry.enabled
            && let Some(command) = entry.command.clone()
        {
            self.commands.borrow_mut().push_back(command);
            window.remove_window();
        }
    }
    fn move_selection(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let count = self.matches.len();
        if count == 0 {
            self.selection = None;
            return;
        }
        self.selection = Some(match (self.selection, backwards) {
            (Some(index), true) => (index + count - 1) % count,
            (Some(index), false) => (index + 1) % count,
            (None, true) => count - 1,
            (None, false) => 0,
        });
        if let Some(index) = self.selection {
            self.scroll.scroll_to_item(index);
        }
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => window.remove_window(),
            "up" => self.move_selection(true, cx),
            "down" => self.move_selection(false, cx),
            "enter" => {
                if let Some(index) = self.selection {
                    self.run(index, window);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(ui::rem_size());
        let c = self.colors;
        let count = self.matches.len();
        let selected = self.selection;
        div().id("command-palette").role(Role::Dialog).aria_label(self.title)
            .aria_description("Type to filter commands. Up and Down select a result. Enter runs the command. Escape closes the palette.")
            .track_focus(&self.focus).size_full().flex().flex_col()
            .rounded(px(7.)).overflow_hidden().border_1().border_color(rgb(c.border))
            .bg(rgb(c.field)).text_color(rgb(c.ink)).font_family(crate::fonts::INTER_FONT)
            .text_size(px(13.)).capture_key_down(cx.listener(Self::key))
            .child(div().h(px(58.)).flex_shrink_0().flex().items_center().gap(px(10.)).px(px(12.))
                .child(div().text_color(rgb(c.muted)).text_size(px(18.)).child(">"))
                .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.input)
                    .appearance(false).bordered(false).focus_bordered(false).with_size(Size::Medium)))
                .child(ui::control("close-command-palette", "Esc", 35., false, c)
                    .accessibility_label("Close Command Palette").on_click(|_, window, _| window.remove_window())))
            .child(div().h(px(1.)).w_full().bg(rgb(c.border)).flex_shrink_0())
            .child(div().id("command-palette-results").role(Role::Menu).aria_label("Matching commands")
                .flex_1().min_h(px(0.)).overflow_y_scroll().track_scroll(&self.scroll)
                .flex().flex_col().p(px(5.))
                .when(count == 0, |list| list.child(div().px(px(12.)).py(px(18.))
                    .text_color(rgb(c.muted)).child("No matching results.")))
                .children(self.matches.iter().enumerate().map(|(position, index)| {
                    let entry = &self.entries[*index];
                    let active = selected == Some(position);
                    let foreground = if active { 0xffffff } else { c.ink };
                    Button::new(("palette-command", *index)).role(Role::MenuItem)
                        .accessibility_label(entry.label.clone()).aria_selected(active)
                        .when(!entry.shortcut.is_empty(), |button| button.aria_keyshortcuts(entry.shortcut.clone()))
                        .focusable(false).tab_stop(false).h(px(31.)).w_full().flex_shrink_0()
                        .px(px(9.)).rounded(px(4.)).cursor_pointer()
                        .bg(rgb(if active { 0x0069dc } else { c.field })).text_color(rgb(foreground))
                        .child(div().w_full().min_w(px(0.)).flex().items_center().gap(px(10.))
                            .child(div().flex_1().min_w(px(0.)).truncate().child(entry.label.clone()))
                            .when(!entry.shortcut.is_empty(), |row| row.child(div().flex_shrink_0()
                                .text_size(px(11.)).text_color(rgb(if active { 0xffffff } else { c.muted }))
                                .child(entry.shortcut.clone()))))
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if *hovered && this.selection != Some(position) { this.selection = Some(position); cx.notify(); }
                        }))
                        .on_click(cx.listener(move |this, _, window, _| this.run(position, window)))
                })))
            .child(div().h(px(1.)).w_full().bg(rgb(c.border)).flex_shrink_0())
            .child(div().h(px(31.)).flex_shrink_0().flex().items_center().justify_between().px(px(12.))
                .text_size(px(10.)).text_color(rgb(c.muted))
                .child("Up/Down to navigate · Enter to run")
                .child(format!("{count} result{}", if count == 1 { "" } else { "s" })))
    }
}

fn filtered(entries: &[MenuEntry], query: &str) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (entry.enabled
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
    use super::{filtered, matches_query};
    #[test]
    fn palette_search_matches_command_names_and_keybindings_together() {
        use crate::{app::Command, dialogs::MenuEntry};
        let entries = [
            MenuEntry::item("New window", "Ctrl+Shift+N", Command::New),
            MenuEntry::item("New tab", "Ctrl+N", Command::NewTab),
        ];
        assert_eq!(filtered(&entries, "ctrl+shift+n"), [0]);
        assert_eq!(filtered(&entries, "new ctrl+n"), [1]);
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
