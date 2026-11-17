//! TextEdit's compact native controls, shared panel windows and measured ruler.
use nsh_base::{ScrollbarLook, ScrollbarMode, ScrollbarTheme, set_scrollbar_theme};
use nsh_core::desktop::{
    TitleBarOptions, TitleBarStyle, WindowAttributes, WindowFrameStyle, WindowKind, WindowPosition,
};
use nsh_core::element::{IntoReactive, desktop};
use nsh_core::prelude::*;
use std::{
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering},
};

static SCALE_PERCENT: AtomicU16 = AtomicU16::new(100);
static ACTIVE_APPEARANCE: AtomicU8 = AtomicU8::new(0);
static CONTROLS_LEFT: AtomicBool = AtomicBool::new(cfg!(target_os = "macos"));
pub const SCALE_RANGE: std::ops::RangeInclusive<u16> = 50..=300;
pub const CONTROL_MOTION_MS: f32 = 120.;

pub fn sync_chrome_preferences(
    appearance: crate::preferences::Appearance,
    controls: crate::preferences::WindowControls,
    controls_percent: u16,
) {
    let mode = match appearance {
        crate::preferences::Appearance::System => 0,
        crate::preferences::Appearance::Light => 1,
        crate::preferences::Appearance::Dark => 2,
    };
    ACTIVE_APPEARANCE.store(mode, Ordering::Relaxed);
    let left = matches!(controls, crate::preferences::WindowControls::Left)
        || (matches!(controls, crate::preferences::WindowControls::Custom)
            && controls_percent < 50);
    CONTROLS_LEFT.store(left, Ordering::Relaxed);
}

pub fn configure_scrollbars() {
    let mut vertical = ScrollbarLook::default();
    vertical.class = Some("absolute top-0 right-0 bottom-0 w-[12px] p-[2px]".into());
    vertical.thumb_class = Some(
        "w-full rounded-full bg-neutral-500/55 hover:bg-neutral-500/80 active:bg-neutral-500/95"
            .into(),
    );
    vertical.accessibility_label = Some("Document scrollbar".into());

    let mut horizontal = ScrollbarLook::default();
    horizontal.class = Some("absolute left-0 right-0 bottom-0 h-[12px] p-[2px]".into());
    horizontal.thumb_class = Some(
        "h-full rounded-full bg-neutral-500/55 hover:bg-neutral-500/80 active:bg-neutral-500/95"
            .into(),
    );
    horizontal.accessibility_label = Some("Horizontal scrollbar".into());

    let mut theme = ScrollbarTheme::default();
    theme.mode = ScrollbarMode::Always;
    theme.min_thumb_length = nsh_core::geometry::px(36.0);
    theme.vertical = Some(vertical);
    theme.horizontal = Some(horizontal);

    set_scrollbar_theme(Some(theme));
}

pub fn set_scale_percent(percent: u16) {
    SCALE_PERCENT.store(
        percent.clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end()),
        Ordering::Relaxed,
    );
}
pub fn scale_percent() -> u16 {
    SCALE_PERCENT.load(Ordering::Relaxed)
}
pub fn px(value: f32) -> Points {
    nsh_core::geometry::px(value * f32::from(scale_percent()) / 100.)
}
pub fn rgb(value: u32) -> Hsla {
    nsh_core::color::rgb(value).into()
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub chrome: u32,
    pub toolbar: u32,
    pub paper: u32,
    pub ink: u32,
    pub border: u32,
    pub field: u32,
    pub muted: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub danger: u32,
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                chrome: 0x303030,
                toolbar: 0x383838,
                paper: 0x1e1e1e,
                ink: 0xeeeeee,
                border: 0x545454,
                field: 0x474747,
                muted: 0xb6b6b6,
                accent: 0x087ef5,
                on_accent: 0xffffff,
                danger: 0xff8b85,
            }
        } else {
            Self {
                chrome: 0xe7e7e7,
                toolbar: 0xf0f0f0,
                paper: 0xffffff,
                ink: 0x242424,
                border: 0xb8b8b8,
                field: 0xffffff,
                muted: 0x626262,
                accent: 0x0069dc,
                on_accent: 0xffffff,
                danger: 0xba2020,
            }
        }
    }
    pub fn current() -> Self {
        let dark_mode = match ACTIVE_APPEARANCE.load(Ordering::Relaxed) {
            1 => false,
            2 => true,
            _ => dark(),
        };
        Self::new(dark_mode)
    }
}

/// Center the entire selection row, including its mark, on the label's line.
pub fn checkbox(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: impl IntoReactive<bool>,
    colors: Palette,
    on_change: impl Fn(bool) + 'static,
) -> nsh_base::CheckboxElement {
    let checked: Prop<bool> = Prop::from(checked.into_reactive());
    let label: SharedString = label.into();
    nsh_base::checkbox()
        .key(Into::<SharedString>::into(id))
        .checked(checked)
        .accessibility_label(label.clone())
        .flex_row()
        .items_center()
        .gap(px(8.))
        .min_h(px(22.))
        .rounded(px(3.))
        .text_color(rgb(colors.ink))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
        .hover(move |s| s.bg(rgb(colors.toolbar)))
        .active(|s| s.opacity(0.8))
        .disabled_style(|s| s.opacity(0.5))
        .indicator_style(move |s| {
            s.size(px(14.))
                .flex_shrink(0.)
                .items_center()
                .justify_center()
                .border_1()
                .border_color(rgb(colors.border))
                .rounded(px(3.))
                .bg(rgb(colors.field))
        })
        .indicator(
            text(move || if checked.get() { "✓" } else { "" })
                .text_size(px(12.))
                .text_color(rgb(colors.ink)),
        )
        .child(text(label).text_size(px(12.)).line_height(px(18.)))
        .on_change(move |checked: &bool| on_change(*checked))
}

pub fn radio(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: impl IntoReactive<bool>,
    colors: Palette,
    on_change: impl Fn(bool) + 'static,
) -> nsh_base::RadioElement {
    let checked: Prop<bool> = Prop::from(checked.into_reactive());
    let label: SharedString = label.into();
    nsh_base::radio()
        .key(Into::<SharedString>::into(id))
        .checked(checked)
        .accessibility_label(label.clone())
        .flex_row()
        .items_center()
        .gap(px(8.))
        .min_h(px(22.))
        .rounded(px(3.))
        .hover(move |s| s.bg(rgb(colors.toolbar)))
        .active(|s| s.opacity(0.8))
        .disabled_style(|s| s.opacity(0.5))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
        .child(
            view()
                .size(px(14.))
                .flex_shrink(0.)
                .rounded_full()
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(colors.field))
                .items_center()
                .justify_center()
                .child(
                    view()
                        .size(px(6.))
                        .rounded_full()
                        .bg(rgb(colors.ink))
                        .opacity(move || if checked.get() { 1_f32 } else { 0_f32 }),
                ),
        )
        .child(
            text(label)
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(rgb(colors.ink)),
        )
        .on_change(move |value: &bool| on_change(*value))
}

pub fn radio_group(label: &str) -> nsh_base::RadioGroupElement {
    nsh_base::radio_group().key(label.to_owned()).gap(px(6.))
}

pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    font_size: f32,
) -> nsh_base::ButtonElement {
    button_styled(id, label, font_size, Palette::current())
}

pub fn button_styled(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    font_size: f32,
    colors: Palette,
) -> nsh_base::ButtonElement {
    let label: SharedString = label.into();
    nsh_base::button()
        .key(Into::<SharedString>::into(id))
        .accessibility_label(label.clone())
        .h(px(28.))
        .min_w(px(0.))
        .px(px(10.))
        .flex_row()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(colors.border))
        .bg(rgb(colors.field))
        .text_color(rgb(colors.ink))
        .transition_colors()
        .duration(move || {
            if reduce_motion() {
                0.
            } else {
                CONTROL_MOTION_MS
            }
        })
        .hover(move |s| s.bg(rgb(colors.toolbar)))
        .active(|s| s.opacity(0.75))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
        .disabled_style(|s| s.opacity(0.5))
        .child(
            text(label)
                .text_size(px(font_size))
                .line_height(px(font_size * 1.25)),
        )
}

pub fn control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    width: f32,
    active: bool,
    colors: Palette,
) -> nsh_base::ButtonElement {
    let label: SharedString = label.into();
    let description = match label.as_ref() {
        "B" => "Bold",
        "I" => "Italic",
        "U̲" => "Underline",
        _ => label.as_ref(),
    }
    .to_owned();
    button_styled(id, label, 11., colors)
        .accessibility_label(description)
        .w(px(width))
        .h(px(23.))
        .px(px(4.))
        .selected(active)
        .border_color(rgb(if active { colors.accent } else { colors.border }))
        .bg(rgb(if active { colors.accent } else { colors.field }))
        .text_color(rgb(if active { colors.on_accent } else { colors.ink }))
        .hover(move |s| {
            s.bg(rgb(if active {
                colors.accent
            } else {
                colors.toolbar
            }))
        })
}

pub fn menu_bar_item(label: &'static str, width: f32, colors: Palette) -> nsh_base::ButtonElement {
    let is_app = label == "ExEditor";
    nsh_base::button()
        .key(label)
        .accessibility_label(label)
        .w(px(width))
        .h(px(22.))
        .px(px(6.))
        .flex_row()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_color(rgb(colors.ink))
        .transition_colors()
        .duration(move || {
            if reduce_motion() {
                0.
            } else {
                CONTROL_MOTION_MS
            }
        })
        .hover(move |s| s.bg(rgb(colors.field)))
        .active(move |s| s.bg(rgb(colors.accent)).text_color(rgb(colors.on_accent)))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
        .child(
            text(label)
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(if is_app {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                }),
        )
}

pub fn dropdown_control(
    id: &'static str,
    value: impl Into<SharedString>,
    width: f32,
    colors: Palette,
) -> nsh_base::ButtonElement {
    let value: SharedString = value.into();
    nsh_base::button()
        .key(id)
        .accessibility_label(id)
        .w(px(width))
        .h(px(23.))
        .px(px(6.))
        .gap(px(4.))
        .flex_row()
        .items_center()
        .justify_between()
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(colors.border))
        .bg(rgb(colors.field))
        .text_color(rgb(colors.ink))
        .hover(move |s| s.bg(rgb(colors.toolbar)))
        .active(|s| s.opacity(0.8))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
        .child(
            text(value)
                .text_size(px(11.))
                .line_height(px(15.))
                .truncate()
                .flex_1()
                .min_w(px(0.)),
        )
        .child(
            text("▾")
                .text_size(px(9.))
                .text_color(rgb(colors.muted))
                .flex_shrink_0(),
        )
}

pub fn color_control(
    id: &'static str,
    glyph: &'static str,
    swatch: u32,
    background_swatch: bool,
    colors: Palette,
) -> nsh_base::ButtonElement {
    let button = nsh_base::button()
        .key(id)
        .accessibility_label(id)
        .w(px(26.))
        .h(px(23.))
        .px(px(3.))
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(1.))
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(colors.border))
        .bg(rgb(colors.field))
        .text_color(rgb(colors.ink))
        .hover(move |s| s.bg(rgb(colors.toolbar)))
        .active(|s| s.opacity(0.8))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)));
    if background_swatch {
        button.child(
            view()
                .size(px(14.))
                .rounded(px(2.5))
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(swatch & 0xffffff)),
        )
    } else {
        button
            .child(
                text(glyph)
                    .text_size(px(11.))
                    .line_height(px(12.))
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                view()
                    .w(px(14.))
                    .h(px(3.))
                    .rounded_full()
                    .bg(rgb(swatch & 0xffffff)),
            )
    }
}

pub fn toolbar_divider(colors: Palette) -> impl IntoElement {
    view()
        .w(px(1.))
        .h(px(16.))
        .mx(px(2.))
        .flex_shrink_0()
        .bg(rgb(colors.border))
}

pub fn segmented_tabs<T: Copy + PartialEq + 'static>(
    group_label: &'static str,
    tabs: &[(T, &'static str)],
    selected: Signal<T>,
    colors: Palette,
) -> impl IntoElement {
    view()
        .key(group_label)
        .role(AccessibilityRole::Tablist)
        .label(group_label)
        .flex_row()
        .items_center()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(colors.border))
        .bg(rgb(colors.toolbar))
        .children(tabs.iter().enumerate().map(|(index, &(tab, title))| {
            nsh_base::button()
                .key(format!("segment-tab-{index}"))
                .role(AccessibilityRole::Tab)
                .accessibility_label(title)
                .selected(move || selected.get() == tab)
                .flex_1()
                .h(px(26.))
                .px(px(10.))
                .rounded(px(4.))
                .items_center()
                .justify_center()
                .bg(move || {
                    rgb(if selected.get() == tab {
                        colors.accent
                    } else {
                        colors.toolbar
                    })
                })
                .text_color(move || {
                    rgb(if selected.get() == tab {
                        colors.on_accent
                    } else {
                        colors.ink
                    })
                })
                .hover(move |s| {
                    s.bg(rgb(if selected.get() == tab {
                        colors.accent
                    } else {
                        colors.field
                    }))
                })
                .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
                .child(
                    text(title)
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM),
                )
                .on_press(move || selected.set(tab))
                .into_element()
        }))
}

pub fn input(
    label: impl Into<SharedString>,
    value: Signal<String>,
    colors: Palette,
) -> nsh_base::Input {
    let label: SharedString = label.into();
    nsh_base::input()
        .label(label.clone())
        .placeholder(label.to_string())
        .value(move || value.get())
        .caret_color(rgb(colors.ink))
        .selection_color(rgba(0x3399ff66))
        .placeholder_color(rgb(colors.muted))
        .on_change(move |next: &str| value.set(next.to_owned()))
        .submit_behavior(SubmitBehavior::Submit)
        .h(px(30.))
        .min_w(px(0.))
        .px(px(8.))
        .text_size(px(12.))
        .line_height(px(18.))
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(colors.border))
        .bg(rgb(colors.field))
        .text_color(rgb(colors.ink))
        .focus(move |s| s.border_color(rgb(colors.accent)))
}

pub fn select(
    label: impl Into<SharedString>,
    items: Vec<nsh_base::SelectItem>,
    value: impl IntoReactive<Option<SharedString>>,
    colors: Palette,
    on_change: impl Fn(Option<SharedString>) + 'static,
) -> nsh_base::SelectElement {
    let label: SharedString = label.into();
    let value: Prop<_> = Prop::from(value.into_reactive());
    let titles = items.clone();
    nsh_base::select()
        .accessibility_label(label)
        .items(items)
        .value(move || value.get())
        .on_change(move |next: &Option<SharedString>| on_change(next.clone()))
        .popup_style(move |s| {
            s.p(px(4.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(colors.field))
                .shadow_md()
        })
        .item_class("rounded focus:bg-accent focus:text-accent-foreground disabled:opacity-50")
        .render_item(|entry: &nsh_base::SelectEntry| {
            text(entry.title.clone()).px(px(8.)).py(px(5.))
        })
        .trigger(
            view()
                .h(px(30.))
                .px(px(8.))
                .min_w(px(0.))
                .flex_row()
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(colors.field))
                .child(
                    text(move || {
                        let chosen = value.get();
                        titles
                            .iter()
                            .find(|item| Some(&item.value) == chosen.as_ref())
                            .map(|item| item.title.clone())
                            .unwrap_or_default()
                    })
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(rgb(colors.ink))
                    .text_size(px(12.)),
                )
                .child(
                    text("▾")
                        .text_size(px(10.))
                        .text_color(rgb(colors.muted))
                        .flex_shrink_0(),
                ),
        )
}

pub fn slider(
    label: impl Into<SharedString>,
    value: impl IntoReactive<f64>,
    colors: Palette,
    on_change: impl Fn(f64) + 'static,
) -> nsh_base::SliderElement {
    let value: Prop<_> = Prop::from(value.into_reactive());
    nsh_base::slider()
        .accessibility_label(Into::<SharedString>::into(label))
        .value(move || Some(nsh_base::SliderValue::from(value.get())))
        .min(0.)
        .max(100.)
        .step(0.1)
        .track_style(|s| s.h(px(24.)).justify_center())
        .indicator_style(move |s| s.h(px(5.)).rounded_full().bg(rgb(colors.border)))
        .range_style(move |s| s.rounded_full().bg(rgb(colors.accent)))
        .thumb_class("hover:opacity-80 focus-visible:outline-2")
        .thumb_style(move |s| {
            s.size(px(14.))
                .rounded_full()
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(colors.field))
                .outline_color(rgb(colors.accent))
        })
        .on_change(move |next: &nsh_base::SliderValue| on_change(next.end()))
}

pub fn command_list(
    items: impl IntoReactive<Vec<nsh_base::CommandItem>>,
    label: &str,
    searchable: bool,
    colors: Palette,
) -> nsh_base::CommandElement {
    nsh_base::command()
        .items(Prop::from(items.into_reactive()))
        .accessibility_label(label.to_owned())
        .searchable(searchable)
        .auto_focus(true)
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(colors.field))
        .text_color(rgb(colors.ink))
        .search_bar_style(move |s| s.p(px(10.)).border_b_1().border_color(rgb(colors.border)))
        .search_style(move |s| s.h(px(30.)).text_size(px(13.)).text_color(rgb(colors.ink)))
        .list_style(|s| s.flex_1().min_h(px(0.)))
        .item_class(format!(
            "focus:bg-[#{:06x}] focus:text-[#{:06x}]",
            colors.accent, colors.on_accent
        ))
        .item_style(|s| s.h(px(30.)).px(px(10.)).rounded(px(3.)).text_size(px(12.)))
        .separator_style(move |s| s.h(px(1.)).my(px(4.)).bg(rgb(colors.border)))
        .empty(
            text("No matching results.")
                .p(px(14.))
                .text_color(rgb(colors.muted)),
        )
}

pub fn heading(label: impl Into<SharedString>) -> impl IntoElement {
    text(Into::<SharedString>::into(label))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
}
pub fn hint(label: impl IntoReactive<SharedString>, colors: Palette) -> impl IntoElement {
    text(label)
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(rgb(colors.muted))
}
pub fn row(label: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .min_h(px(30.))
        .child(
            text(Into::<SharedString>::into(label))
                .w(px(150.))
                .flex_shrink(0.)
                .text_size(px(12.)),
        )
        .child(view().flex_1().min_w(px(0.)).child(control))
}
pub fn surface(colors: Palette) -> nsh_core::element::View {
    view()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .gap(px(10.))
        .p(px(14.))
        .bg(rgb(colors.chrome))
        .text_color(rgb(colors.ink))
        .font_family(crate::fonts::INTER_FONT)
}
pub fn error(message: Signal<String>, colors: Palette) -> impl IntoElement {
    when(
        move || !message.get().is_empty(),
        move || {
            text(move || message.get())
                .text_size(px(12.))
                .text_color(rgb(colors.danger))
                .role(AccessibilityRole::Alert)
        },
    )
}

#[derive(Clone)]
struct Panel {
    name: &'static str,
    serial: u64,
    title: String,
    width: f32,
    height: f32,
    kind: WindowKind,
    colors: Palette,
    content: Rc<dyn Fn() -> AnyElement>,
}
impl PartialEq for Panel {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial
    }
}
#[derive(Clone, Default)]
struct Panels {
    next: u64,
    windows: Vec<Panel>,
}

fn panel_window_attributes(kind: WindowKind, title: &str) -> WindowAttributes {
    let mut attributes = WindowAttributes::default();
    attributes.title = Some(title.to_owned());
    let mut bar = TitleBarOptions::default();
    bar.style = TitleBarStyle::Hidden;
    bar.height = Some(px(if kind == WindowKind::Popup { 0. } else { 32. }));
    attributes.title_bar = Some(bar);
    attributes.frame = Some(WindowFrameStyle::None);
    attributes.close_on_escape = Some(true);
    attributes.minimizable = Some(false);
    attributes.maximizable = Some(false);
    if kind == WindowKind::Popup {
        attributes.resizable = Some(false);
    }
    attributes
}

fn panel_title_bar(name: &'static str, title: &str, colors: Palette) -> AnyElement {
    let display_title = title
        .strip_suffix(" | ExEditor")
        .or_else(|| title.strip_prefix("ExEditor "))
        .filter(|trimmed| !trimmed.is_empty())
        .unwrap_or(title)
        .to_owned();
    let close = nsh_base::button()
        .key(format!("close-panel-{name}"))
        .accessibility_label("Close")
        .size(px(15.))
        .rounded_full()
        .bg(rgb(0xff5f57))
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.7))
        .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
        .on_press(move || close_panel(name));
    let controls_left = CONTROLS_LEFT.load(Ordering::Relaxed);
    let mut header = view()
        .flex_row()
        .items_center()
        .h(px(32.))
        .px(px(12.))
        .flex_shrink_0()
        .bg(rgb(colors.chrome))
        .border_b_1()
        .border_color(rgb(colors.border))
        .window_drag(true);
    let caption = view()
        .flex_1()
        .min_w(px(0.))
        .items_center()
        .justify_center()
        .overflow_hidden()
        .child(
            text(display_title)
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(colors.ink))
                .truncate(),
        );
    header = if controls_left {
        header
            .child(view().w(px(24.)).items_start().child(close))
            .child(caption)
            .child(view().w(px(24.)))
    } else {
        header
            .child(view().w(px(24.)))
            .child(caption)
            .child(view().w(px(24.)).items_end().child(close))
    };
    header.into_element()
}

/// Requests are made on the UI thread; content is created under its native window owner.
pub fn show_panel(
    name: &'static str,
    title: &str,
    width: f32,
    height: f32,
    kind: WindowKind,
    content: impl Fn() -> AnyElement + 'static,
) {
    show_panel_with_colors(
        name,
        title,
        width,
        height,
        kind,
        Palette::current(),
        content,
    );
}

pub fn show_panel_with_colors(
    name: &'static str,
    title: &str,
    width: f32,
    height: f32,
    kind: WindowKind,
    colors: Palette,
    content: impl Fn() -> AnyElement + 'static,
) {
    global::<Panels>().update(|panels| {
        panels.next += 1;
        panels.windows.retain(|panel| panel.name != name);
        panels.windows.push(Panel {
            name,
            serial: panels.next,
            title: title.into(),
            width,
            height,
            kind,
            colors,
            content: Rc::new(content),
        });
    });
}
pub fn close_panel(name: &str) {
    global::<Panels>().update(|panels| panels.windows.retain(|panel| panel.name != name));
}
pub fn close_all_panels() {
    global::<Panels>().update(|panels| panels.windows.clear());
}
/// Mount once under the application root, beside the document windows.
pub fn panel_host() -> impl IntoElement {
    let panels = global::<Panels>();
    each(
        move || panels.with(|state| state.windows.clone()),
        |panel: &Panel| panel.serial,
        move |item, _| {
            let panel = item.get_untracked();
            let name = panel.name;
            let serial = panel.serial;
            let kind = panel.kind;
            let colors = panel.colors;
            let title = panel.title.clone();
            let attributes = panel_window_attributes(kind, &title);
            desktop::window()
                .id(format!("exeditor-{name}-{}", panel.serial))
                .kind(kind)
                .title(panel.title)
                .attributes(attributes.clone())
                .default_position(WindowPosition::ParentCenter)
                .default_size(size(px(panel.width), px(panel.height)))
                .on_open_change(move |change| {
                    if !change.open {
                        panels
                            .update(|state| state.windows.retain(|window| window.serial != serial));
                    }
                })
                .content(move || {
                    desktop::current_window().set(attributes.clone());
                    crate::window_style::after_open();
                    if kind != WindowKind::Popup {
                        on_key("escape", move |_| close_panel(name));
                    }
                    let mut frame = view()
                        .flex_1()
                        .min_w(px(0.))
                        .min_h(px(0.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(colors.border))
                        .bg(rgb(if kind == WindowKind::Popup {
                            colors.field
                        } else {
                            colors.chrome
                        }))
                        .font_family(crate::fonts::INTER_FONT)
                        .on_layout(|_: &Bounds<Points>| crate::window_style::refresh());
                    if kind != WindowKind::Popup {
                        frame = frame.child(panel_title_bar(name, &title, colors));
                    }
                    frame.child((panel.content)())
                })
        },
    )
}

pub fn ruler(colors: Palette, zoom: u16, left_inset: f32) -> AnyElement {
    let width = signal(0_f32);
    view()
        .h(px(26.))
        .w_full()
        .flex_shrink(0.)
        .bg(rgb(colors.toolbar))
        .border_b_1()
        .border_color(rgb(colors.border))
        .label("Paragraph ruler, measured in inches")
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(
            canvas(move || {
                let scale = f32::from(zoom) / 100.;
                let step = 72. * scale;
                let origin = left_inset + 20. * scale;
                let mut painter = Painter::new()
                    .stroke_style(rgb(colors.muted))
                    .fill_style(rgb(colors.muted))
                    .line_width(0.75)
                    .font("9px Inter");
                let available = (width.get() - origin).max(0.);
                let ticks = ((available / (step / 8.)).ceil() as usize).min(2048);
                let bottom = px(25.).0;
                for tick in 0..=ticks {
                    let x = origin + tick as f32 * step / 8.;
                    let height = if tick % 8 == 0 {
                        10.
                    } else if tick % 4 == 0 {
                        6.5
                    } else {
                        3.5
                    };
                    painter = painter
                        .begin_path()
                        .move_to(x, bottom)
                        .line_to(x, bottom - px(height).0)
                        .stroke();
                    if tick % 8 == 0 {
                        painter =
                            painter.fill_text(&(tick / 8).to_string(), x + px(3.).0, px(11.).0);
                    }
                }
                painter
            })
            .flex_1(),
        )
        .into_element()
}

pub fn icon_control(
    id: &str,
    markup: &str,
    width: f32,
    selected: bool,
    colors: Palette,
) -> nsh_base::ButtonElement {
    control(id.to_owned(), "", width, selected, colors)
        .h(px(23.))
        .accessibility_label(id.to_owned())
        .child(
            svg(nsh_core::element::ImageSrc::svg_markup(markup))
                .size(px(14.))
                .text_color(rgb(if selected {
                    colors.on_accent
                } else {
                    colors.ink
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn textedit_palette_keeps_separate_document_and_chrome_roles() {
        for dark in [false, true] {
            let palette = Palette::new(dark);
            assert_ne!(palette.ink, palette.field);
            assert_ne!(palette.chrome, palette.paper);
            assert_ne!(palette.accent, palette.on_accent);
        }
    }
}
