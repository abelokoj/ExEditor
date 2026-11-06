//! Separate GPUI windows keep menus and panels above the native document surface.
use crate::{
    app::Command,
    ui::{self, Palette, px},
};
use gpui_kit::base::Button as MenuButton;
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputState},
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};

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
    /// Sent while the entry is highlighted, so the window can show it before it is chosen.
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
struct MenuView {
    keyboard: crate::keyboard::State,
    entries: Vec<MenuEntry>,
    commands: Commands,
    colors: Palette,
    focus: FocusHandle,
    selection: Option<usize>,
    scroll: ScrollHandle,
}
#[derive(Clone, Copy)]
enum MenuStep {
    Previous,
    Next,
    First,
    Last,
}

pub fn menu(
    entries: Vec<MenuEntry>,
    position: Point<Pixels>,
    commands: Commands,
    colors: Palette,
    cx: &mut App,
) {
    let height = entries
        .iter()
        .map(|entry| if entry.command.is_some() { 26. } else { 9. })
        .sum::<f32>()
        + 12.;
    let options = WindowOptions {
        kind: WindowKind::PopUp,
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            position,
            size(px(278.), px(height.min(650.))),
        ))),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        let handle = window.window_handle();
        window.set_window_title("ExEditor menu");
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            cx.spawn(async move |_, cx| {
                loop {
                    smol::Timer::after(Duration::from_millis(160)).await;
                    let active = handle.update(cx, |_, window, _| {
                        if !window.is_window_active() {
                            window.remove_window();
                            false
                        } else {
                            true
                        }
                    });
                    if !matches!(active, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
            // Start on the checked entry so arrow keys browse from the current choice.
            let selection = entries
                .iter()
                .position(|entry| entry.checked && entry.enabled)
                .or_else(|| menu_selection(&entries, None, MenuStep::First));
            MenuView {
                keyboard: crate::keyboard::State::default(),
                entries,
                commands,
                colors,
                focus,
                selection,
                scroll: ScrollHandle::new(),
            }
        })
    });
    if let Err(error) = result {
        super::app::show_error(&error);
    }
}
impl Drop for MenuView {
    fn drop(&mut self) {
        // A previewing menu always ends its preview; a chosen entry has already been queued.
        if self.entries.iter().any(|entry| entry.preview.is_some()) {
            self.commands.borrow_mut().push_back(Command::EndPreview);
        }
    }
}
impl MenuView {
    fn select(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if self.selection == index {
            return;
        }
        self.selection = index;
        if let Some(preview) = index.and_then(|index| self.entries[index].preview.clone()) {
            self.commands.borrow_mut().push_back(preview);
        }
        cx.notify();
    }
    fn activate(&self, window: &mut Window) {
        if let Some(entry) = self.selection.and_then(|index| self.entries.get(index))
            && entry.enabled
            && let Some(command) = entry.command.clone()
        {
            self.commands.borrow_mut().push_back(command);
            window.remove_window();
        }
    }
    fn navigate(&mut self, step: MenuStep, cx: &mut Context<Self>) {
        let index = menu_selection(&self.entries, self.selection, step);
        if let Some(index) = index {
            self.scroll.scroll_to_item(index);
        }
        self.select(index, cx);
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        if m.platform {
            self.keyboard.reset();
            return;
        }
        let modifiers = crate::keyboard::modifiers_for_key(
            &event.keystroke.key,
            crate::keyboard::Modifiers {
                control: m.control,
                shift: m.shift,
                alt: m.alt,
            },
        );
        let key = crate::keyboard::virtual_key(&event.keystroke.key).unwrap_or(0);
        match self
            .keyboard
            .resolve(key, modifiers, std::time::Instant::now())
        {
            crate::keyboard::Outcome::Action(action) => {
                if let Some(command) = crate::app::shortcut_command(action) {
                    self.commands.borrow_mut().push_back(command);
                    window.remove_window();
                }
            }
            crate::keyboard::Outcome::Prefix | crate::keyboard::Outcome::Cancelled => {}
            crate::keyboard::Outcome::Ignored => {
                if m.control || m.alt {
                    return;
                }
                self.navigation_key(event, window, cx);
                return;
            }
        }
        cx.stop_propagation();
        window.prevent_default();
    }
    fn navigation_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" | "tab" => window.remove_window(),
            "up" => self.navigate(MenuStep::Previous, cx),
            "down" => self.navigate(MenuStep::Next, cx),
            "home" => self.navigate(MenuStep::First, cx),
            "end" => self.navigate(MenuStep::Last, cx),
            "enter" | "space" => self.activate(window),
            _ => return,
        }
        cx.stop_propagation();
    }
}
impl Render for MenuView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui-kit components size text in rem; scale it with View > Interface Size.
        window.set_rem_size(crate::ui::rem_size());
        let c = self.colors;
        div()
            .id("menu")
            .role(Role::Menu)
            .aria_label("ExEditor menu")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .py(px(6.))
            .px(px(5.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(rgb(c.chrome))
            .rounded(px(7.))
            .border_1()
            .border_color(rgb(c.border))
            .text_size(px(13.))
            .text_color(rgb(c.ink))
            .font_family("Segoe UI")
            .on_key_down(cx.listener(Self::key))
            .children(self.entries.iter().enumerate().map(|(index, entry)| {
                let Some(command) = entry.command.clone() else {
                    return div()
                        .id(("menu-separator", index))
                        .role(Role::Splitter)
                        .h(px(9.))
                        .px(px(7.))
                        .flex()
                        .items_center()
                        .child(div().h(px(1.)).w_full().bg(rgb(c.border)))
                        .into_any_element();
                };
                let selected = self.selection == Some(index) && entry.enabled;
                let foreground = if selected {
                    0xffffff
                } else if entry.enabled {
                    c.ink
                } else {
                    c.muted
                };
                let queue = self.commands.clone();
                MenuButton::new(("menu-item", index))
                    .role(if entry.checkable {
                        Role::MenuItemCheckBox
                    } else {
                        Role::MenuItem
                    })
                    .accessibility_label(entry.label.clone())
                    .disabled(!entry.enabled)
                    .focusable(false)
                    .tab_stop(false)
                    .aria_selected(selected)
                    .when(!entry.shortcut.is_empty(), |button| {
                        button.aria_keyshortcuts(entry.shortcut.clone())
                    })
                    .when(entry.checkable, |button| {
                        button.aria_toggled(if entry.checked {
                            Toggled::True
                        } else {
                            Toggled::False
                        })
                    })
                    .when(selected, |button| button.aria_active_descendant())
                    .w_full()
                    .h(px(26.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .bg(if selected {
                        rgb(0x0069dc)
                    } else {
                        rgb(c.chrome)
                    })
                    .text_color(rgb(foreground))
                    .when(entry.enabled, |button| button.cursor_pointer())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .w(px(19.))
                                    .flex_shrink_0()
                                    .when(entry.checked, |mark| {
                                        mark.child(ui::icon(MENU_CHECK, foreground))
                                    }),
                            )
                            .child(div().flex_1().child(entry.label.clone()))
                            .child(
                                div()
                                    .ml(px(20.))
                                    .text_size(px(11.))
                                    .child(entry.shortcut.clone()),
                            ),
                    )
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        if *hovered && this.entries[index].enabled {
                            this.select(Some(index), cx);
                        }
                    }))
                    .on_click(move |_, window, _| {
                        queue.borrow_mut().push_back(command.clone());
                        window.remove_window();
                    })
                    .into_any_element()
            }))
    }
}
const MENU_CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="m3 8 3 3 7-7"/></svg>"#;
fn menu_selection(entries: &[MenuEntry], current: Option<usize>, step: MenuStep) -> Option<usize> {
    let enabled: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (entry.enabled && entry.command.is_some()).then_some(index))
        .collect();
    if enabled.is_empty() {
        return None;
    }
    let position =
        current.and_then(|index| enabled.iter().position(|candidate| *candidate == index));
    let selected = match step {
        MenuStep::First => 0,
        MenuStep::Last => enabled.len() - 1,
        MenuStep::Next => position.map_or(0, |position| (position + 1) % enabled.len()),
        MenuStep::Previous => position.map_or(enabled.len() - 1, |position| {
            (position + enabled.len() - 1) % enabled.len()
        }),
    };
    Some(enabled[selected])
}

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
struct FormView {
    kind: FormKind,
    title: String,
    fields: Vec<(Field, Entity<InputState>)>,
    commands: Commands,
    colors: Palette,
    focus: FocusHandle,
    error: Option<String>,
}
pub fn form(
    kind: FormKind,
    title: &str,
    fields: Vec<Field>,
    commands: Commands,
    colors: Palette,
    cx: &mut App,
) {
    let title = title.to_string();
    let height = (fields.len() as f32 * 43. + 145.).clamp(250., 780.);
    let options = WindowOptions {
        kind: WindowKind::Dialog,
        window_bounds: Some(WindowBounds::centered(size(px(470.), px(height)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone().into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let fields: Vec<_> = fields
                .into_iter()
                .map(|field| {
                    let value = field.value.clone();
                    let placeholder = field.label.clone();
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(placeholder)
                            .default_value(value)
                    });
                    (field, input)
                })
                .collect();
            let focus = cx.focus_handle();
            if let Some((_, input)) = fields.iter().find(|(field, _)| !field.toggle) {
                input.update(cx, |state, cx| state.focus(window, cx));
            } else {
                window.focus(&focus, cx);
            }
            FormView {
                kind,
                title,
                fields,
                commands,
                colors,
                focus,
                error: None,
            }
        })
    });
    if let Err(error) = result {
        super::app::show_error(&error);
    }
}

impl FormView {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values: Vec<String> = self
            .fields
            .iter()
            .map(|(_, state)| state.read(cx).value().to_string())
            .collect();
        let valid = match self.kind {
            FormKind::Find | FormKind::Replace => values.first().is_some_and(|s| !s.is_empty()),
            FormKind::Font => {
                values.first().is_some_and(|s| !s.trim().is_empty())
                    && values
                        .get(1)
                        .and_then(|s| s.parse::<f32>().ok())
                        .is_some_and(|n| n.is_finite() && n > 0. && n <= 1000.)
            }
            FormKind::FontSize => values
                .first()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .is_some_and(|n| n.is_finite() && (1. ..=1000.).contains(&n)),
            FormKind::InterfaceScale => values
                .first()
                .and_then(|s| s.trim().trim_end_matches('%').trim().parse::<u16>().ok())
                .is_some_and(|n| ui::SCALE_RANGE.contains(&n)),
            FormKind::Table => values
                .iter()
                .all(|s| s.parse::<u16>().is_ok_and(|n| n > 0 && n <= 100)),
            _ => true,
        };
        if !valid {
            self.error = Some("Enter valid values before continuing.".into());
            cx.notify();
            return;
        }
        self.commands
            .borrow_mut()
            .push_back(Command::Submit(self.kind.clone(), values));
        window.remove_window();
    }
}
impl Render for FormView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui-kit components size text in rem; scale it with View > Interface Size.
        window.set_rem_size(crate::ui::rem_size());
        let c = self.colors;
        div()
            .id("form")
            .track_focus(&self.focus)
            .role(Role::Dialog)
            .aria_label(self.title.clone())
            .flex()
            .flex_col()
            .size_full()
            .p(px(22.))
            .gap(px(16.))
            .bg(rgb(c.chrome))
            .text_color(rgb(c.ink))
            .text_size(px(13.))
            .font_family(crate::fonts::RICH_FONT)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                } else if event.keystroke.key == "enter" && event.keystroke.modifiers.control {
                    this.submit(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .id("form-fields")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(px(12.))
                    .overflow_y_scroll()
                    .children(
                        self.fields
                            .iter()
                            .enumerate()
                            .map(|(index, (field, state))| {
                                let input = state.clone();
                                let value = state.read(cx).value().to_string();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .min_h(px(30.))
                                    .child(
                                        div()
                                            .w(px(180.))
                                            .flex_shrink_0()
                                            .child(field.label.clone()),
                                    )
                                    .child(if field.toggle {
                                        Checkbox::new(("field-toggle", index))
                                            .checked(value == "true")
                                            .label(field.label.clone())
                                            .on_click(move |checked, window, cx| {
                                                input.update(cx, |state, cx| {
                                                    state.set_value(
                                                        if *checked { "true" } else { "false" },
                                                        window,
                                                        cx,
                                                    )
                                                });
                                            })
                                            .into_any_element()
                                    } else {
                                        div().flex_1().child(Input::new(state)).into_any_element()
                                    })
                            }),
                    ),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(div().text_color(rgb(0xba2020)).child(error))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(9.))
                    .child(
                        ui::control("cancel", "Cancel", 80., false, c)
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        ui::control(
                            "confirm",
                            match self.kind {
                                FormKind::Find => "Find Next",
                                FormKind::Replace => "Replace All",
                                _ => "OK",
                            },
                            100.,
                            true,
                            c,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}
