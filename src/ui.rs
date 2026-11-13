//! TextEdit's compact native controls, shared panel windows and measured ruler.
use nsh_core::desktop::{WindowKind, WindowPosition};
use nsh_core::element::{IntoReactive, desktop};
use nsh_core::prelude::*;
use std::{
    rc::Rc,
    sync::atomic::{AtomicU16, Ordering},
};

static SCALE_PERCENT: AtomicU16 = AtomicU16::new(100);
pub const SCALE_RANGE: std::ops::RangeInclusive<u16> = 50..=300;
pub const CONTROL_MOTION_MS: f32 = 120.;

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
pub fn rem_size() -> Points {
    px(16.)
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
        Self::new(dark())
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
        .test_id(Into::<SharedString>::into(id).as_ref())
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
        .test_id(Into::<SharedString>::into(id).as_ref())
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
                        .opacity(move || if checked.get() { 1. } else { 0. }),
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
    nsh_base::radio_group().label(label.to_owned()).gap(px(6.))
}

pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    font_size: f32,
) -> nsh_base::ButtonElement {
    let colors = Palette::current();
    let label: SharedString = label.into();
    nsh_base::button()
        .test_id(Into::<SharedString>::into(id).as_ref())
        .accessibility_label(label.clone())
        .h(px(28.))
        .min_w(px(0.))
        .px(px(10.))
        .flex_row()
        .items_center()
        .justify_center()
        .rounded(px(4.))
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
    button(id, label, 11.)
        .accessibility_label(description)
        .w(px(width))
        .h(px(24.))
        .px(px(2.))
        .selected(active)
        .bg(rgb(if active { colors.accent } else { colors.field }))
        .text_color(rgb(if active { colors.on_accent } else { colors.ink }))
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
                    .text_color(rgb(colors.ink))
                    .text_size(px(12.)),
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
        .thumb_style(move |s| {
            s.size(px(14.))
                .rounded_full()
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(colors.field))
                .hover(|s| s.opacity(0.8))
                .focus_visible(move |s| s.outline(px(2.)).outline_color(rgb(colors.accent)))
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
        .item_style(move |s| {
            s.h(px(30.))
                .px(px(10.))
                .rounded(px(3.))
                .text_size(px(12.))
                .text_color(rgb(colors.ink))
                .focus(move |s| s.bg(rgb(colors.accent)).text_color(rgb(colors.on_accent)))
        })
        .separator_style(move |s| s.h(px(1.)).my(px(4.)).bg(rgb(colors.border)))
        .empty(
            text("No matching results.")
                .p(px(14.))
                .text_color(rgb(colors.muted)),
        )
}

pub fn icon(data: &'static str, color: u32) -> nsh_core::element::Svg {
    svg(nsh_core::element::ImageSrc::svg_markup(data))
        .size(px(14.))
        .text_color(rgb(color))
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
                .w(px(174.))
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
        .gap(px(12.))
        .p(px(16.))
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
    content: Rc<dyn Fn() -> AnyElement>,
}
impl PartialEq for Panel {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial
    }
}
#[derive(Default)]
struct Panels {
    next: u64,
    windows: Vec<Panel>,
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
            content: Rc::new(content),
        });
    });
}
pub fn close_panel(name: &str) {
    global::<Panels>().update(|panels| panels.windows.retain(|panel| panel.name != name));
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
            desktop::window()
                .id(format!("exeditor-{name}-{}", panel.serial))
                .kind(panel.kind)
                .title(panel.title)
                .default_position(WindowPosition::ParentCenter)
                .default_size(size(px(panel.width), px(panel.height)))
                .on_open_change(move |change| {
                    if !change.open {
                        panels
                            .update(|state| state.windows.retain(|window| window.serial != serial));
                    }
                })
                .content(move || (panel.content)())
        },
    )
}

pub fn ruler(colors: Palette, zoom: u16, on_tab: impl Fn(f32) + 'static) -> AnyElement {
    let width = signal(0_f32);
    view()
        .h(px(28.))
        .w_full()
        .flex_shrink(0.)
        .bg(rgb(colors.toolbar))
        .border_b_1()
        .border_color(rgb(colors.border))
        .label("Paragraph ruler, measured in inches")
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .on_press(move |event: &PressEvent| on_tab(event.page_position.x.0))
        .child(
            canvas(move || {
                let step = 72. * f32::from(zoom) / 100.;
                let mut painter = Painter::new()
                    .stroke_style(rgb(colors.muted))
                    .fill_style(rgb(colors.muted))
                    .line_width(0.65)
                    .font("9px Inter");
                for tick in 0..((width.get() / (step / 8.)).ceil() as usize) {
                    let x = 12. + tick as f32 * step / 8.;
                    let height = if tick % 8 == 0 {
                        10.
                    } else if tick % 4 == 0 {
                        7.
                    } else {
                        4.
                    };
                    painter = painter
                        .begin_path()
                        .move_to(x, px(11.).0)
                        .line_to(x, px(11. - height).0)
                        .stroke();
                    if tick % 8 == 0 {
                        painter =
                            painter.fill_text(&(tick / 8).to_string(), x + px(2.).0, px(24.).0);
                    }
                }
                painter
            })
            .flex_1(),
        )
        .into_element()
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
pub const ALIGN_LEFT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M2 6h8M2 9h12M2 12h8"/></svg>"#;
pub const ALIGN_CENTER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M4 6h8M2 9h12M4 12h8"/></svg>"#;
pub const ALIGN_RIGHT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M6 6h8M2 9h12M6 12h8"/></svg>"#;
pub const ALIGN_JUSTIFY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M2 6h12M2 9h12M2 12h12"/></svg>"#;
pub const LIST: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M5 3h9M5 7h9M5 11h9"/><g fill="currentColor" stroke="none"><circle cx="2" cy="3" r="1"/><circle cx="2" cy="7" r="1"/><circle cx="2" cy="11" r="1"/></g></svg>"#;
/// Lines with an up/down arrow, as on TextEdit's line and paragraph spacing control.
pub const LINE_SPACING: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M7 3h7M7 6.5h7M7 10h7M7 13.5h7M3 2.5v11M1.5 4 3 2.5 4.5 4M1.5 12 3 13.5 4.5 12"/></svg>"#;

pub fn icon_control(
    id: &str,
    markup: &str,
    width: f32,
    selected: bool,
    colors: Palette,
) -> nsh_base::ButtonElement {
    control(id.to_owned(), "", width, selected, colors)
        .h(px(24.))
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
