//! Compact TextEdit controls, measured ruler, and document palette.
use gpui_kit::component::{
    Icon,
    button::{Button, ButtonVariants as _},
    input::Input,
};
use gpui_kit::*;
use std::sync::atomic::{AtomicU16, Ordering};

/// Interface size in percent (View > Interface Size). GPUI exposes no scale override
/// outside tests, so ExEditor's UI modules size themselves through [`px`] instead.
static SCALE_PERCENT: AtomicU16 = AtomicU16::new(100);
pub const SCALE_RANGE: std::ops::RangeInclusive<u16> = 50..=300;

pub fn set_scale_percent(percent: u16) {
    SCALE_PERCENT.store(
        percent.clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end()),
        Ordering::Relaxed,
    );
}
pub fn scale_percent() -> u16 {
    SCALE_PERCENT.load(Ordering::Relaxed)
}
/// Logical pixels multiplied by the interface size; shadows GPUI's `px` in UI modules.
pub fn px(value: f32) -> Pixels {
    gpui_kit::px(value * f32::from(scale_percent()) / 100.)
}

/// GPUI's 16px rem multiplied by the interface size, for gpui-kit components.
pub fn rem_size() -> Pixels {
    px(16.)
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub chrome: u32,
    pub toolbar: u32,
    pub paper: u32,
    pub ink: u32,
    pub border: u32,
    pub field: u32,
    pub muted: u32,
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
            }
        }
    }
}
// Keep labels and marks on the same scaled line; no unscaled absolute offsets.
fn selection_indicator(checked: bool, radio: bool, colors: Palette) -> Div {
    let indicator = div()
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(rgb(if checked { colors.ink } else { colors.border }))
        .rounded(px(if radio { 7. } else { 3. }))
        .bg(rgb(if checked && !radio {
            colors.ink
        } else {
            colors.field
        }));
    let indicator = if radio {
        indicator.when(checked, |indicator| {
            indicator.child(div().size(px(6.)).rounded_full().bg(rgb(colors.ink)))
        })
    } else {
        indicator.when(checked, |indicator| {
            indicator.child(
                Icon::default()
                    .data(CHECK_MARK.as_bytes())
                    .size(px(10.))
                    .text_color(rgb(colors.paper)),
            )
        })
    };
    #[cfg(test)]
    let indicator = {
        use gpui_kit::base::TestSupportExt as _;
        indicator.debug_selector(|| "selection-indicator".into())
    };
    div()
        .w(px(14.))
        .h(px(18.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(indicator)
}
fn selection_label(label: SharedString) -> Div {
    let label = div()
        .flex_1()
        .min_w(px(0.))
        .line_height(px(18.))
        .child(label);
    #[cfg(test)]
    let label = {
        use gpui_kit::base::TestSupportExt as _;
        label.debug_selector(|| "selection-label".into())
    };
    label
}
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    colors: Palette,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui_kit::base::Checkbox {
    let label = label.into();
    gpui_kit::base::Checkbox::new(id)
        .checked(checked)
        .accessibility_label(label.clone())
        .flex()
        .items_start()
        .gap(px(8.))
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(rgb(colors.ink))
        .font_family(crate::fonts::INTER_FONT)
        .rounded(px(3.))
        .border_1()
        .border_color(hsla(0., 0., 0., 0.))
        .focus(|style| style.border_color(rgb(0x087ef5)))
        .child(selection_indicator(checked, false, colors))
        .child(selection_label(label))
        .on_change(move |state, _, window, cx| {
            window.prevent_default();
            on_change(
                &(state == gpui_kit::base::CheckboxState::Checked),
                window,
                cx,
            );
        })
}
pub fn radio(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    colors: Palette,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui_kit::base::Radio {
    let label = label.into();
    gpui_kit::base::Radio::new(id)
        .checked(checked)
        .accessibility_label(label.clone())
        .flex()
        .items_start()
        .gap(px(8.))
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(rgb(colors.ink))
        .font_family(crate::fonts::INTER_FONT)
        .rounded(px(3.))
        .border_1()
        .border_color(hsla(0., 0., 0., 0.))
        .focus(|style| style.border_color(rgb(0x087ef5)))
        .child(selection_indicator(checked, true, colors))
        .child(selection_label(label))
        .on_change(move |checked, _, window, cx| {
            window.prevent_default();
            on_change(&checked, window, cx);
        })
}
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, font_size: f32) -> Button {
    let label = label.into();
    Button::new(id)
        .accessibility_label(label.clone())
        .py_0()
        .child(
            div()
                .size_full()
                .min_w(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(font_size))
                .line_height(relative(1.25))
                .child(label),
        )
}
pub fn input(input: Input, height: f32, font_size: f32) -> Input {
    input
        .h(px(height))
        .py_0()
        .text_size(px(font_size))
        .line_height(px(font_size * 1.25))
}
const CHECK_MARK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 8l3 3 7-7"/></svg>"#;

pub fn control(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    width: f32,
    active: bool,
    colors: Palette,
) -> Button {
    let id: ElementId = id.into();
    let label: SharedString = label.into();
    let description = match label.as_ref() {
        "B" => "Bold".to_string(),
        "I" => "Italic".to_string(),
        "U̲" => "Underline".to_string(),
        "" => format!("{id:?}"),
        _ => label.to_string(),
    };
    button(id, label, 11.)
        .ghost()
        .compact()
        .accessibility_label(description.clone())
        .tooltip(description)
        .w(px(width))
        .min_w(px(0.))
        .h(px(21.))
        .px(px(2.))
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(colors.border))
        .bg(rgb(if active { 0x737373 } else { colors.field }))
        .text_color(rgb(if active { 0xffffff } else { colors.ink }))
        .text_size(px(11.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(if active { 0x626262 } else { colors.chrome })))
}
pub fn icon(data: &'static str, color: u32) -> Icon {
    Icon::default()
        .data(data.as_bytes())
        .size(px(14.))
        .text_color(rgb(color))
}
pub const ALIGN_LEFT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M2 6h8M2 9h12M2 12h8"/></svg>"#;
pub const ALIGN_CENTER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M4 6h8M2 9h12M4 12h8"/></svg>"#;
pub const ALIGN_RIGHT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M6 6h8M2 9h12M6 12h8"/></svg>"#;
pub const ALIGN_JUSTIFY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 3h12M2 6h12M2 9h12M2 12h12"/></svg>"#;
pub const LIST: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M5 3h9M5 7h9M5 11h9"/><g fill="currentColor" stroke="none"><circle cx="2" cy="3" r="1"/><circle cx="2" cy="7" r="1"/><circle cx="2" cy="11" r="1"/></g></svg>"#;
/// Lines with an up/down arrow, as on TextEdit's line and paragraph spacing control.
pub const LINE_SPACING: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M7 3h7M7 6.5h7M7 10h7M7 13.5h7M3 2.5v11M1.5 4 3 2.5 4.5 4M1.5 12 3 13.5 4.5 12"/></svg>"#;

pub fn ruler(
    colors: Palette,
    zoom: u16,
    on_tab: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id("paragraph-ruler")
        .h(px(28.))
        .w_full()
        .flex_shrink_0()
        .bg(rgb(colors.toolbar))
        .border_b_1()
        .border_color(rgb(colors.border))
        .on_mouse_down(MouseButton::Left, move |e, window, cx| {
            on_tab(f32::from(e.position.x), window, cx)
        })
        .child(
            canvas(
                move |bounds, _, _| bounds,
                move |_, bounds, window, cx| {
                    let step = 72. * zoom as f32 / 100.;
                    let width = f32::from(bounds.size.width);
                    let origin = bounds.origin;
                    let mut ticks = PathBuilder::stroke(px(0.65));
                    for tick in 0..((width / (step / 8.)).ceil() as usize) {
                        // Tick spacing follows the document's inches, not the interface size.
                        let x = origin.x + gpui_kit::px(12. + tick as f32 * step / 8.);
                        let height = if tick % 8 == 0 {
                            10.
                        } else if tick % 4 == 0 {
                            7.
                        } else {
                            4.
                        };
                        ticks.move_to(point(x, origin.y + px(11.)));
                        ticks.line_to(point(x, origin.y + px(11. - height)));
                        if tick % 8 == 0 {
                            let label: SharedString = (tick / 8).to_string().into();
                            let len = label.len();
                            let shaped = window.text_system().shape_line(
                                label,
                                px(9.),
                                &[TextRun {
                                    len,
                                    font: font(crate::fonts::INTER_FONT),
                                    color: rgb(colors.muted).into(),
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                }],
                                None,
                            );
                            let _ = shaped.paint(
                                point(x + px(2.), origin.y + px(12.)),
                                px(12.),
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                    }
                    if let Ok(path) = ticks.build() {
                        window.paint_path(path, rgb(colors.muted));
                    }
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use gpui_kit::test::TestWindowExt as _;
    struct Controls {
        radio: bool,
        checked: bool,
        changes: usize,
    }
    impl Render for Controls {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let handler = cx.listener(|this, checked: &bool, _, cx| {
                this.checked = *checked;
                this.changes += 1;
                cx.notify();
            });
            let colors = Palette::new(true);
            let control = if self.radio {
                radio("choice", "Grammar", self.checked, colors, handler).into_any_element()
            } else {
                checkbox("choice", "Grammar", self.checked, colors, handler).into_any_element()
            };
            div()
                .tab_group()
                .size_full()
                .p(gpui_kit::px(16.))
                .child(div().w(gpui_kit::px(320.)).child(control))
        }
    }
    #[gpui_kit::test]
    fn selection_controls_center_marks_with_labels_and_keep_full_row_keyboard_activation(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        for is_radio in [false, true] {
            let (view, cx) = cx.add_window_view(move |_, _| Controls {
                radio: is_radio,
                checked: false,
                changes: 0,
            });
            cx.update(|window, cx| window.render_frame(cx));
            let square = cx
                .debug_bounds("selection-indicator")
                .expect("indicator geometry");
            let label = cx.debug_bounds("selection-label").expect("label geometry");
            assert!((f32::from(square.center().y - label.center().y)).abs() <= 0.5);
            cx.simulate_click(label.center(), Modifiers::default());
            assert!(view.read(cx).checked);
            assert_eq!(
                view.read(cx).changes,
                1,
                "the label activates the control exactly once"
            );
            cx.simulate_keystrokes("tab space");
            assert_eq!(
                view.read(cx).checked,
                is_radio,
                "radio stays selected; checkbox toggles off"
            );
        }
    }
}
