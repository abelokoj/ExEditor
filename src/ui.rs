//! Compact TextEdit controls, measured ruler, and document palette.
use gpui_kit::component::{
    Icon,
    button::{Button, ButtonVariants as _},
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
    Button::new(id)
        .ghost()
        .compact()
        .label(label)
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
                                    font: font(crate::fonts::RICH_FONT),
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
