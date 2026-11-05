//! Compact TextEdit controls, measured ruler, and document palette.
use gpui_kit::component::{
    Icon,
    button::{Button, ButtonVariants as _},
};
use gpui_kit::*;

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
                paper: 0xffffff,
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
pub const LIST: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M5 3h9M5 7h9M5 11h9"/><circle cx="2" cy="3" r=".6"/><circle cx="2" cy="7" r=".6"/><circle cx="2" cy="11" r=".6"/></svg>"#;

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
                        let x = origin.x + px(12. + tick as f32 * step / 8.);
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
