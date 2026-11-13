//! CotEditor-style Colors in a shared native NSH tool window.
use crate::{
    app::{Command, show_error},
    color::{ColorFormat, ColorTarget, PickerState, Rgba, named_colors},
    dialogs::Commands,
    ui::{self, Palette, px, rgb},
};
use nsh_core::{
    desktop::WindowKind,
    element::{ImageSrc, desktop},
    prelude::*,
};
use std::{
    f32::consts::TAU,
    path::Path,
    rc::{Rc, Weak},
    sync::Arc,
};

const WHEEL_SIZE: f32 = 190.;
const CHANNEL_MODELS: [&str; 4] = [
    "RGB Sliders",
    "HSB Sliders",
    "CMYK Sliders",
    "Gray Scale Slider",
];
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Wheel,
    Sliders,
    Palette,
    Image,
    Pencils,
}
impl Mode {
    const ALL: [(Self, &'static str); 5] = [
        (Self::Wheel, "Color Wheel"),
        (Self::Sliders, "Color Sliders"),
        (Self::Palette, "Color Palettes"),
        (Self::Image, "Image Palettes"),
        (Self::Pencils, "Colored Pencils"),
    ];
}
struct SampleImage {
    pixels: image::RgbaImage,
    source: ImageSrc,
    name: String,
}
#[derive(Default)]
struct ActiveColors {
    panel: Weak<ColorsPanel>,
    window: Option<desktop::WindowRef>,
}
struct ColorsPanel {
    color: Signal<Rgba>,
    target: Signal<ColorTarget>,
    commands: Stored<Commands>,
    format: Signal<ColorFormat>,
    uppercase: Signal<bool>,
    code: Signal<String>,
    dirty: Signal<bool>,
    query: Signal<String>,
    mode: Signal<Mode>,
    model: Signal<usize>,
    hue: Signal<f32>,
    saturation: Signal<f32>,
    selected_swatch: Signal<usize>,
    swatches: Signal<Vec<Rgba>>,
    image: Stored<Option<SampleImage>>,
    image_version: Signal<u64>,
    pixel_x: Signal<String>,
    pixel_y: Signal<String>,
    sampling: Signal<bool>,
    error: Signal<String>,
    colors: Palette,
}
pub fn open(initial: Rgba, target: ColorTarget, commands: Commands) {
    open_with_code(initial, target, commands, None);
}
pub fn open_with_code(
    initial: Rgba,
    target: ColorTarget,
    commands: Commands,
    initial_code: Option<String>,
) {
    if let Some((panel, window)) = global::<ActiveColors>()
        .with_untracked(|active| active.panel.upgrade().zip(active.window.clone()))
    {
        panel.target.set(target);
        panel.commands.replace(commands);
        panel.initial_color(initial, initial_code.as_deref());
        window.show();
        window.focus();
        return;
    }
    let saved = match PickerState::load() {
        Ok(saved) => saved,
        Err(error) => {
            show_error(&error);
            return;
        }
    };
    ui::show_panel(
        "colors",
        "Colors",
        450.,
        770.,
        WindowKind::Panel,
        move || {
            let state = Rc::new(ColorsPanel {
                color: signal(initial),
                target: signal(target),
                commands: stored(commands.clone()),
                format: signal(saved.format),
                uppercase: signal(false),
                code: signal(String::new()),
                dirty: signal(false),
                query: signal(String::new()),
                mode: signal(Mode::Wheel),
                model: signal(0),
                hue: signal(initial.hsv().0),
                saturation: signal(initial.hsv().1),
                selected_swatch: signal(0),
                swatches: signal(saved.swatches.clone()),
                image: stored(None),
                image_version: signal(0),
                pixel_x: signal("0".into()),
                pixel_y: signal("0".into()),
                sampling: signal(false),
                error: signal(String::new()),
                colors: Palette::current(),
            });
            state.initial_color(initial, initial_code.as_deref());
            global::<ActiveColors>().update(|active| {
                active.panel = Rc::downgrade(&state);
                active.window = Some(desktop::current_window());
            });
            state.render()
        },
    );
}
impl ColorsPanel {
    fn initial_color(&self, color: Rgba, code: Option<&str>) {
        self.uppercase
            .set(code.is_some_and(|value| value.bytes().any(|byte| byte.is_ascii_uppercase())));
        let color = if let Some((parsed, format)) = code.and_then(|value| Rgba::parse(value).ok()) {
            self.format.set(format);
            parsed
        } else {
            color
        };
        self.set_color(color);
    }
    fn set_color(&self, color: Rgba) {
        let (hue, saturation, value) = color.hsv();
        if value > 0. {
            self.saturation.set(saturation);
            if saturation > 0. {
                self.hue.set(hue);
            }
        }
        self.color.set(color);
        self.error.set(String::new());
        self.sync_code();
    }
    fn sync_code(&self) {
        let color = self.color.get_untracked();
        let mut format = self.format.get_untracked();
        if format == ColorFormat::Keyword
            && !named_colors()
                .iter()
                .any(|(_, value)| *value == color.rgb())
        {
            format = ColorFormat::Hex;
            self.format.set(format);
        }
        let code = format.format(color, self.uppercase.get_untracked());
        self.uppercase
            .set(code.bytes().any(|byte| byte.is_ascii_uppercase()));
        self.code.set(code);
        self.dirty.set(false);
    }
    fn parse_code(&self) -> bool {
        if !self.dirty.get_untracked() {
            return true;
        }
        let code = self.code.get_untracked();
        match Rgba::parse(code.trim()) {
            Ok((color, format)) => {
                self.uppercase
                    .set(code.bytes().any(|byte| byte.is_ascii_uppercase()));
                self.format.set(format);
                self.set_color(color);
                true
            }
            Err(error) => {
                self.error.set(error.to_string());
                false
            }
        }
    }
    fn persist(&self) -> bool {
        let saved = PickerState {
            swatches: self.swatches.get_untracked(),
            format: self.format.get_untracked(),
            last_color: self.color.get_untracked(),
        };
        match saved.save() {
            Ok(()) => true,
            Err(error) => {
                self.error.set(format!("Saving colors: {error}"));
                false
            }
        }
    }
    fn apply(&self) {
        if self.parse_code() && self.persist() {
            let queue = self.commands.get();
            queue.borrow_mut().push_back(Command::ApplyColor(
                self.target.get_untracked(),
                self.color.get_untracked(),
                self.code.get_untracked(),
            ));
        }
    }
    fn save_swatch(&self) {
        let color = self.color.get_untracked();
        let selected = self.selected_swatch.get_untracked();
        self.swatches.update(|swatches| {
            if let Some(swatch) = swatches.get_mut(selected) {
                *swatch = color;
            }
        });
        self.persist();
    }
    fn render(self: &Rc<Self>) -> AnyElement {
        let c = self.colors;
        let mode = self.mode;
        let content = self.clone();
        let alpha = self.color;
        let change_alpha = self.clone();
        let edit_code = self.clone();
        let blur_code = self.clone();
        let parse_code = self.clone();
        let format = self.format;
        let format_choice = self.clone();
        let apply = self.clone();
        let copy = self.clone();
        let save = self.clone();
        let sample = self.clone();
        let shortcut = self.clone();
        on_key("mod+enter", move |_| shortcut.apply());
        ui::surface(c)
            .p(px(12.))
            .child(ui::heading("Colors"))
            .child(
                scroll().flex_1().min_h(px(0.)).child(
                    view()
                        .gap(px(12.))
                        .child(ui::select(
                            "Color picker mode",
                            Mode::ALL
                                .iter()
                                .enumerate()
                                .map(|(index, (_, label))| {
                                    nsh_base::SelectItem::new(index.to_string(), *label)
                                })
                                .collect(),
                            move || {
                                Some(
                                    Mode::ALL
                                        .iter()
                                        .position(|(value, _)| *value == mode.get())
                                        .unwrap_or(0)
                                        .to_string()
                                        .into(),
                                )
                            },
                            c,
                            move |selected| {
                                if let Some((value, _)) = selected
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .and_then(|index| Mode::ALL.get(index))
                                {
                                    mode.set(*value);
                                }
                            },
                        ))
                        .child(view().min_h(px(290.)).child(dynamic(
                            move || match content.mode.get() {
                                Mode::Wheel => content.wheel_view(),
                                Mode::Sliders => content.sliders_view(),
                                Mode::Palette => content.palette_view(),
                                Mode::Image => content.image_view(),
                                Mode::Pencils => content.pencils_view(),
                            },
                        )))
                        .child(ui::heading("Opacity"))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    ui::slider(
                                        "Opacity",
                                        move || f64::from(alpha.get().a) / 2.55,
                                        c,
                                        move |value| {
                                            let mut color = change_alpha.color.get_untracked();
                                            color.a = (value * 2.55).round().clamp(0., 255.) as u8;
                                            change_alpha.set_color(color);
                                        },
                                    )
                                    .flex_1(),
                                )
                                .child(
                                    text(move || {
                                        format!("{:.0}%", f32::from(alpha.get().a) / 2.55)
                                    })
                                    .w(px(50.))
                                    .text_size(px(12.)),
                                ),
                        )
                        .child(
                            ui::input("Color code", self.code, c)
                                .text_size(px(16.))
                                .font_family(crate::fonts::default_plain_font())
                                .on_change(move |value: &str| {
                                    edit_code.code.set(value.to_owned());
                                    edit_code.dirty.set(true);
                                    edit_code.error.set(String::new());
                                })
                                .on_submit(move |_: &str| {
                                    parse_code.parse_code();
                                })
                                .on_blur(move || {
                                    blur_code.parse_code();
                                }),
                        )
                        .child(ui::select(
                            "Color code format",
                            ColorFormat::ALL
                                .iter()
                                .enumerate()
                                .map(|(index, format)| {
                                    nsh_base::SelectItem::new(index.to_string(), format.label())
                                })
                                .collect(),
                            move || {
                                Some(
                                    ColorFormat::ALL
                                        .iter()
                                        .position(|value| *value == format.get())
                                        .unwrap_or(0)
                                        .to_string()
                                        .into(),
                                )
                            },
                            c,
                            move |selected| {
                                if let Some(format) = selected
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .and_then(|index| ColorFormat::ALL.get(index))
                                {
                                    format_choice.format.set(*format);
                                    format_choice.sync_code();
                                    format_choice.persist();
                                }
                            },
                        ))
                        .child(ui::error(self.error, c))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    ui::button("apply-color", "Apply", 12.)
                                        .flex_1()
                                        .on_press(move || apply.apply()),
                                )
                                .child(ui::button("copy-color-code", "Copy Code", 12.).on_press(
                                    move || {
                                        if copy.parse_code() {
                                            set_clipboard(copy.code.get_untracked());
                                        }
                                    },
                                )),
                        )
                        .child(when(
                            {
                                let target = self.target;
                                move || {
                                    alpha.get().a < 255 && target.get() != ColorTarget::InsertCode
                                }
                            },
                            move || {
                                ui::hint("Text formatting blends opacity into its background.", c)
                            },
                        ))
                        .child(ui::heading("Saved Swatches"))
                        .child(self.swatches_view())
                        .child(
                            view()
                                .flex_row()
                                .gap(px(8.))
                                .child(
                                    ui::button(
                                        "save-current-color",
                                        "Save to Selected Swatch",
                                        12.,
                                    )
                                    .flex_1()
                                    .on_press(move || save.save_swatch()),
                                )
                                .child(
                                    ui::button("screen-color-sampler", "Pick from Screen", 12.)
                                        .disabled({
                                            let sampling = self.sampling;
                                            move || {
                                                sampling.get()
                                                    || !(cfg!(target_os = "windows")
                                                        || can_pick_screen_color())
                                            }
                                        })
                                        .on_press(move || sample.sample_screen()),
                                ),
                        ),
                ),
            )
            .into_element()
    }
    fn wheel_view(self: &Rc<Self>) -> AnyElement {
        let c = self.colors;
        let color = self.color;
        let hue = self.hue;
        let saturation = self.saturation;
        let focused = signal(false);
        let size = stored(Size::<Points>::default());
        let state = self.clone();
        let dragging = self.clone();
        let keyboard = self.clone();
        let brightness = computed(move || color.get().hsv().2);
        let wheel = computed(move || wheel_image(brightness.get()));
        let thumb_hue = hue;
        let thumb_saturation = saturation;
        view().gap(px(8.)).items_center()
            .child(view().size(px(WHEEL_SIZE)).relative().focusable(true)
                .label("Hue and saturation").role(AccessibilityRole::Adjustable)
                .hint("Left and Right change hue. Up and Down change saturation. Shift changes by ten.")
                .accessibility_value(move ||Some(nsh_core::AccessibilityValue {
                    min:Some(0.),max:Some(360.),now:Some(hue.get()),
                    text:Some(format!("Hue {:.0} degrees, saturation {:.0} percent",hue.get(),saturation.get()*100.).into()),
                }))
                .focus_visible(move |s|s.outline(px(2.)).outline_color(rgb(c.accent)).rounded_full())
                .on_focus(move ||focused.set(true)).on_blur(move ||focused.set(false))
                .on_layout(move |bounds:&Bounds<Points>|{size.replace(bounds.size);})
                .on_press(move |event:&PressEvent|state.choose_wheel(event.position,size.get()))
                .on_mouse_move(move |event:&MouseEvent| {
                    if event.pressed_button==Some(MouseButton::Left) {dragging.choose_wheel(event.position,size.get());}
                })
                .child(image(move ||wheel.get()).size_full().pointer_events(PointerEvents::None))
                .child(view().absolute().size(px(12.)).rounded_full().border_2().border_color(rgb(c.ink))
                    .left(move ||px((1.+(thumb_hue.get()/360.*TAU).cos()*thumb_saturation.get())*WHEEL_SIZE/2.-6.))
                    .top(move ||px((1.-(thumb_hue.get()/360.*TAU).sin()*thumb_saturation.get())*WHEEL_SIZE/2.-6.))
                    .pointer_events(PointerEvents::None))
                .child(when(focused,move || {
                    for (key,dh,ds) in [("left",-1.,0.),("right",1.,0.),("up",0.,0.01),("down",0.,-0.01),
                        ("shift+left",-10.,0.),("shift+right",10.,0.),("shift+up",0.,0.1),("shift+down",0.,-0.1)]
                    {
                        let state=keyboard.clone();
                        on_key(key,move |_|state.nudge_wheel(dh,ds));
                    }
                    view()
                })))
            .child(self.hsv_slider("Hue",0))
            .child(self.hsv_slider("Saturation",1))
            .child(self.hsv_slider("Brightness",2)).into_element()
    }
    fn choose_wheel(&self, position: Point<Points>, size: Size<Points>) {
        if size.width.0 <= 0. || size.height.0 <= 0. {
            return;
        }
        let dx = position.x.0 / size.width.0 * 2. - 1.;
        let dy = position.y.0 / size.height.0 * 2. - 1.;
        let hue = (-dy).atan2(dx).to_degrees().rem_euclid(360.);
        let saturation = dx.hypot(dy).min(1.);
        self.hue.set(hue);
        self.saturation.set(saturation);
        let color = self.color.get_untracked();
        self.set_color(Rgba::from_hsv(hue, saturation, wheel_value(color), color.a));
    }
    fn nudge_wheel(&self, dh: f32, ds: f32) {
        let hue = (self.hue.get_untracked() + dh).rem_euclid(360.);
        let saturation = (self.saturation.get_untracked() + ds).clamp(0., 1.);
        self.hue.set(hue);
        self.saturation.set(saturation);
        let color = self.color.get_untracked();
        self.set_color(Rgba::from_hsv(hue, saturation, wheel_value(color), color.a));
    }
    fn hsv_slider(self: &Rc<Self>, label: &'static str, index: usize) -> AnyElement {
        let color = self.color;
        let hue = self.hue;
        let saturation = self.saturation;
        let state = self.clone();
        view()
            .w_full()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .child(text(label).w(px(74.)).text_size(px(11.)))
            .child(
                ui::slider(
                    label,
                    move || {
                        f64::from(match index {
                            0 => hue.get() / 3.6,
                            1 => saturation.get() * 100.,
                            _ => color.get().hsv().2 * 100.,
                        })
                    },
                    self.colors,
                    move |value| {
                        let color = state.color.get_untracked();
                        let mut channels = [
                            state.hue.get_untracked(),
                            state.saturation.get_untracked(),
                            color.hsv().2,
                        ];
                        channels[index] = value as f32 / if index == 0 { 1. / 3.6 } else { 100. };
                        state.hue.set(channels[0]);
                        state.saturation.set(channels[1]);
                        state.set_color(Rgba::from_hsv(
                            channels[0],
                            channels[1],
                            channels[2],
                            color.a,
                        ));
                    },
                )
                .flex_1(),
            )
            .into_element()
    }
    fn sliders_view(self: &Rc<Self>) -> AnyElement {
        let model = self.model;
        let state = self.clone();
        view()
            .gap(px(10.))
            .child(ui::select(
                "Color slider model",
                CHANNEL_MODELS
                    .iter()
                    .enumerate()
                    .map(|(index, label)| nsh_base::SelectItem::new(index.to_string(), *label))
                    .collect(),
                move || Some(model.get().to_string().into()),
                self.colors,
                move |value| {
                    if let Some(index) = value
                        .and_then(|value| value.parse::<usize>().ok())
                        .filter(|index| *index < 4)
                    {
                        model.set(index);
                    }
                },
            ))
            .child(dynamic(move || {
                let model = state.model.get();
                let labels: &[&'static str] = match model {
                    1 => &["Hue", "Saturation", "Brightness"],
                    2 => &["Cyan", "Magenta", "Yellow", "Black"],
                    3 => &["Brightness"],
                    _ => &["Red", "Green", "Blue"],
                };
                view()
                    .gap(px(12.))
                    .children(labels.iter().enumerate().map(|(index, label)| {
                        let color = state.color;
                        let set = state.clone();
                        view()
                            .gap(px(2.))
                            .child(
                                view()
                                    .flex_row()
                                    .justify_between()
                                    .child(text(*label).text_size(px(12.)))
                                    .child(
                                        text(move || {
                                            let value = to_channels(model, color.get())[index];
                                            if model == 0 {
                                                format!("{:.0}", value * 255.)
                                            } else if model == 1 && index == 0 {
                                                format!("{:.0}°", value * 360.)
                                            } else {
                                                format!("{:.0}%", value * 100.)
                                            }
                                        })
                                        .text_size(px(11.)),
                                    ),
                            )
                            .child(ui::slider(
                                *label,
                                move || f64::from(to_channels(model, color.get())[index]) * 100.,
                                state.colors,
                                move |value| {
                                    let color = set.color.get_untracked();
                                    let mut channels = to_channels(model, color);
                                    channels[index] = value as f32 / 100.;
                                    set.set_color(from_channels(model, channels, color.a));
                                },
                            ))
                    }))
            }))
            .into_element()
    }
    fn palette_view(self: &Rc<Self>) -> AnyElement {
        let query = self.query;
        let state = self.clone();
        let colors = self.colors;
        view()
            .gap(px(8.))
            .child(ui::input("Search stylesheet colors", self.query, colors))
            .child(
                virtual_list(
                    move || matching_colors(&query.get()).len(),
                    move |at| {
                        let state = state.clone();
                        let select = state.clone();
                        let query = state.query;
                        ui::button("named-color", "", 12.)
                            .h(px(28.))
                            .justify_start()
                            .accessibility_label(move || {
                                matching_colors(&query.get())
                                    .get(at.index())
                                    .map(|(name, value)| format!("{name}, #{value:06x}"))
                                    .unwrap_or_default()
                            })
                            .child(view().size(px(16.)).rounded(px(2.)).bg(move || {
                                rgb(matching_colors(&query.get())
                                    .get(at.index())
                                    .map(|(_, value)| *value)
                                    .unwrap_or(colors.field))
                            }))
                            .child(
                                text(move || {
                                    matching_colors(&query.get())
                                        .get(at.index())
                                        .map(|(name, _)| *name)
                                        .unwrap_or_default()
                                })
                                .text_size(px(12.)),
                            )
                            .on_press(move || {
                                if let Some((_, value)) =
                                    matching_colors(&query.get_untracked()).get(at.index())
                                {
                                    let mut color = Rgba::from_rgb(*value);
                                    color.a = select.color.get_untracked().a;
                                    select.set_color(color);
                                }
                            })
                    },
                )
                .estimated_item_size(px(28.))
                .h(px(238.))
                .empty(ui::hint("No matching color names.", colors)),
            )
            .into_element()
    }
    fn image_view(self: &Rc<Self>) -> AnyElement {
        self.image_version.get();
        let state = self.clone();
        let mut panel = view().gap(px(8.)).child(
            ui::button("load-palette-image", "Open Image…", 12.)
                .on_press(move || state.load_image()),
        );
        let sample = self.image.with(|image| {
            image.as_ref().map(|sample| {
                (
                    sample.source.clone(),
                    sample.name.clone(),
                    sample.pixels.width(),
                    sample.pixels.height(),
                )
            })
        });
        if let Some((source, name, width, height)) = sample {
            let scale = (390. / width as f32).min(190. / height as f32);
            let size = Size {
                width: px(width as f32 * scale),
                height: px(height as f32 * scale),
            };
            let click = self.clone();
            let drag = self.clone();
            let choose = self.clone();
            panel = panel
                .child(
                    image(source)
                        .w(size.width)
                        .h(size.height)
                        .label(format!("Sample colors from {name}"))
                        .on_press(move |event: &PressEvent| {
                            click.choose_image(event.position, size)
                        })
                        .on_mouse_move(move |event: &MouseEvent| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                drag.choose_image(event.position, size);
                            }
                        }),
                )
                .child(ui::hint(name, self.colors))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .child(text("X:").text_size(px(12.)))
                        .child(ui::input("Image pixel X", self.pixel_x, self.colors).w(px(64.)))
                        .child(text("Y:").text_size(px(12.)))
                        .child(ui::input("Image pixel Y", self.pixel_y, self.colors).w(px(64.)))
                        .child(
                            ui::button("sample-image-pixel", "Sample Pixel", 12.)
                                .on_press(move || choose.choose_pixel()),
                        ),
                );
        } else {
            panel = panel.child(ui::hint(
                "Open an image, then click a pixel to sample its color.",
                self.colors,
            ));
        }
        panel.into_element()
    }
    fn load_image(self: &Rc<Self>) {
        let state = self.clone();
        spawn(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Choose a Color Palette Image")
                .add_filter(
                    "Images",
                    &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff"],
                )
                .pick_file()
                .await
            else {
                return;
            };
            let path = file.path().to_owned();
            match smol::unblock(move || read_sample_image(&path)).await {
                Ok(image) => {
                    state.image.replace(Some(image));
                    state.image_version.update(|v| *v += 1);
                    state.error.set(String::new());
                }
                Err(error) => state.error.set(format!("Opening image: {error}")),
            }
        });
    }
    fn choose_image(&self, position: Point<Points>, size: Size<Points>) {
        if size.width.0 <= 0. || size.height.0 <= 0. {
            return;
        }
        let color = self.image.with(|sample| {
            sample.as_ref().map(|sample| {
                let x = ((position.x.0 / size.width.0).clamp(0., 0.999999)
                    * sample.pixels.width() as f32) as u32;
                let y = ((position.y.0 / size.height.0).clamp(0., 0.999999)
                    * sample.pixels.height() as f32) as u32;
                self.pixel_x.set(x.to_string());
                self.pixel_y.set(y.to_string());
                pixel_color(sample.pixels.get_pixel(x, y).0)
            })
        });
        if let Some(color) = color {
            self.set_color(color);
        }
    }
    fn choose_pixel(&self) {
        let point = self
            .pixel_x
            .get_untracked()
            .parse::<u32>()
            .ok()
            .zip(self.pixel_y.get_untracked().parse::<u32>().ok());
        let color = point.and_then(|(x, y)| {
            self.image.with(|image| {
                image.as_ref().and_then(|image| {
                    (x < image.pixels.width() && y < image.pixels.height())
                        .then(|| pixel_color(image.pixels.get_pixel(x, y).0))
                })
            })
        });
        if let Some(color) = color {
            self.set_color(color);
        } else {
            self.error
                .set("Enter pixel coordinates within the image.".into());
        }
    }
    fn pencils_view(self: &Rc<Self>) -> AnyElement {
        view()
            .gap(px(8.))
            .children((0..3).map(|row| {
                view()
                    .flex_row()
                    .justify_between()
                    .children((0..12).map(|column| {
                        let index = row * 12 + column;
                        let color = pencil_color(index);
                        let state = self.clone();
                        ui::button(format!("pencil-{index}"), "", 12.)
                            .accessibility_label(format!(
                                "Pencil color {}: {}",
                                index + 1,
                                color.hex(false, false)
                            ))
                            .tooltip(color.hex(false, false))
                            .w(px(24.))
                            .h(px(56.))
                            .px(px(2.))
                            .child(
                                view()
                                    .w(px(14.))
                                    .h(px(44.))
                                    .rounded(px(2.))
                                    .bg(rgb(color.rgb())),
                            )
                            .on_press(move || {
                                let mut color = color;
                                color.a = state.color.get_untracked().a;
                                state.set_color(color);
                            })
                    }))
            }))
            .child(ui::hint("Choose a pencil color.", self.colors))
            .into_element()
    }
    fn swatches_view(self: &Rc<Self>) -> AnyElement {
        let swatches = self.swatches;
        let selected = self.selected_swatch;
        let colors = self.colors;
        view()
            .gap(px(4.))
            .children((0..2).map(|row| {
                view()
                    .flex_row()
                    .gap(px(4.))
                    .children((0..12).map(|column| {
                        let index = row * 12 + column;
                        let state = self.clone();
                        ui::button(format!("saved-color-{index}"), "", 12.)
                            .w(px(25.))
                            .h(px(25.))
                            .px(px(0.))
                            .accessibility_label(move || {
                                format!(
                                    "Saved color {}: {}",
                                    index + 1,
                                    swatches.with(|colors| colors[index].hex(true, false))
                                )
                            })
                            .bg(move || {
                                rgb(swatches.with(|colors| colors[index].composite(0xffffff)))
                            })
                            .border_color(move || {
                                rgb(if selected.get() == index {
                                    colors.accent
                                } else {
                                    colors.border
                                })
                            })
                            .on_press(move || {
                                selected.set(index);
                                state.set_color(swatches.with_untracked(|colors| colors[index]));
                            })
                    }))
            }))
            .into_element()
    }
    fn sample_screen(self: &Rc<Self>) {
        if self.sampling.get_untracked() {
            return;
        }
        self.sampling.set(true);
        let state = self.clone();
        spawn(async move {
            #[cfg(target_os = "windows")]
            let result = smol::unblock(crate::color::pick_screen_color).await;
            #[cfg(not(target_os = "windows"))]
            let result: anyhow::Result<Option<Rgba>> =
                Ok(nsh_core::element::pick_screen_color().await.map(|color| {
                    let color: nsh_core::Rgba = color.into();
                    Rgba {
                        r: (color.r * 255.).round() as u8,
                        g: (color.g * 255.).round() as u8,
                        b: (color.b * 255.).round() as u8,
                        a: 255,
                    }
                }));
            state.sampling.set(false);
            match result {
                Ok(Some(color)) => state.set_color(color),
                Ok(None) => {}
                Err(error) => state.error.set(format!("Sampling screen color: {error}")),
            }
        });
    }
}
fn pixel_color(pixel: [u8; 4]) -> Rgba {
    Rgba {
        r: pixel[0],
        g: pixel[1],
        b: pixel[2],
        a: pixel[3],
    }
}
fn matching_colors(query: &str) -> Vec<(&'static str, u32)> {
    let query = query.to_lowercase();
    named_colors()
        .iter()
        .filter(|(name, _)| name.contains(&query))
        .copied()
        .collect()
}
fn wheel_value(color: Rgba) -> f32 {
    let value = color.hsv().2;
    if value < 0.01 { 1. } else { value }
}
fn wheel_image(value: f32) -> ImageSrc {
    let side = 380;
    let mut pixels = image::RgbaImage::new(side, side);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let dx = (x as f32 + 0.5) / side as f32 * 2. - 1.;
        let dy = (y as f32 + 0.5) / side as f32 * 2. - 1.;
        let radius = dx.hypot(dy);
        if radius > 1. {
            continue;
        }
        let color = Rgba::from_hsv(
            (-dy).atan2(dx).to_degrees().rem_euclid(360.),
            radius,
            value,
            255,
        );
        *pixel = image::Rgba([
            color.r,
            color.g,
            color.b,
            (((1. - radius) * side as f32).clamp(0., 1.) * 255.) as u8,
        ]);
    }
    ImageSrc::rgba(side, side, pixels.into_raw())
}
fn read_sample_image(path: &Path) -> anyhow::Result<SampleImage> {
    use anyhow::{Context as _, ensure};
    let mut reader = image::ImageReader::open(path)
        .context("opening the image")?
        .with_guessed_format()
        .context("identifying the image format")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let pixels = reader.decode().context("decoding the image")?.to_rgba8();
    ensure!(
        pixels.width() > 0 && pixels.height() > 0,
        "The image contains no pixels."
    );
    let bytes: Arc<[u8]> = pixels.as_raw().clone().into();
    let source = ImageSrc::rgba(pixels.width(), pixels.height(), bytes);
    Ok(SampleImage {
        pixels,
        source,
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    })
}
fn to_channels(model: usize, color: Rgba) -> [f32; 4] {
    let r = f32::from(color.r) / 255.;
    let g = f32::from(color.g) / 255.;
    let b = f32::from(color.b) / 255.;
    match model {
        1 => {
            let (h, s, v) = color.hsv();
            [h / 360., s, v, 0.]
        }
        2 => {
            let black = 1. - r.max(g).max(b);
            if black >= 0.99999 {
                [0., 0., 0., 1.]
            } else {
                [
                    (1. - r - black) / (1. - black),
                    (1. - g - black) / (1. - black),
                    (1. - b - black) / (1. - black),
                    black,
                ]
            }
        }
        3 => [0.2126 * r + 0.7152 * g + 0.0722 * b, 0., 0., 0.],
        _ => [r, g, b, 0.],
    }
}
fn from_channels(model: usize, values: [f32; 4], alpha: u8) -> Rgba {
    let values = values.map(|value| value.clamp(0., 1.));
    let byte = |value: f32| (value * 255.).round() as u8;
    match model {
        1 => Rgba::from_hsv(values[0] * 360., values[1], values[2], alpha),
        2 => Rgba {
            r: byte((1. - values[0]) * (1. - values[3])),
            g: byte((1. - values[1]) * (1. - values[3])),
            b: byte((1. - values[2]) * (1. - values[3])),
            a: alpha,
        },
        3 => Rgba {
            r: byte(values[0]),
            g: byte(values[0]),
            b: byte(values[0]),
            a: alpha,
        },
        _ => Rgba {
            r: byte(values[0]),
            g: byte(values[1]),
            b: byte(values[2]),
            a: alpha,
        },
    }
}
fn pencil_color(index: usize) -> Rgba {
    match index / 12 {
        0 => Rgba::from_hsv((index % 12) as f32 * 30., 0.9, 0.95, 255),
        1 => Rgba::from_hsv((index % 12) as f32 * 30., 0.55, 0.68, 255),
        _ => {
            let value = ((index % 12) as f32 / 11. * 255.).round() as u8;
            Rgba {
                r: value,
                g: value,
                b: value,
                a: 255,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channel_models_preserve_opaque_primaries_and_alpha() {
        for model in [0, 1, 2] {
            for rgb in [0xff0000, 0x00ff00, 0x0000ff, 0xffffff, 0] {
                let mut color = Rgba::from_rgb(rgb);
                color.a = 128;
                let roundtrip = from_channels(model, to_channels(model, color), color.a);
                assert_eq!(roundtrip, color, "model {model}, {rgb:06x}");
            }
        }
        assert_eq!(
            from_channels(3, [0.5, 0., 0., 0.], 64),
            Rgba {
                r: 128,
                g: 128,
                b: 128,
                a: 64
            }
        );
    }
    #[test]
    fn black_wheel_picks_restore_visible_brightness_and_names_search_without_case() {
        assert_eq!(wheel_value(Rgba::from_rgb(0)), 1.);
        assert!(
            matching_colors("REBECCA")
                .iter()
                .any(|(name, _)| *name == "rebeccapurple")
        );
    }
}
