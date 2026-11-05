//! CotEditor-style shared Colors panel: native GPUI controls and CSS color codes.
use crate::{
    app::{Command, show_error},
    color::{ColorFormat, ColorTarget, PickerState, Rgba, named_colors},
    dialogs::Commands,
};
use gpui_kit::base::{Slider as BaseSlider, SliderThumb, SliderTrack};
use gpui_kit::component::{
    Icon, Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    select::{Select, SelectEvent, SelectState},
    slider::{SliderEvent, SliderState},
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use std::{cell::Cell, f32::consts::TAU, path::Path, rc::Rc, sync::Arc};

const PAPER: u32 = 0xffffff;
const INK: u32 = 0x292929;
const MUTED: u32 = 0x737373;
const FIELD: u32 = 0xf3f3f3;
const WHEEL_SIZE: f32 = 190.;
const CHANNEL_MODELS: [&str; 4] = [
    "RGB Sliders",
    "HSB Sliders",
    "CMYK Sliders",
    "Gray Scale Slider",
];
type Choice = SelectState<Vec<&'static str>>;

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
#[derive(Clone)]
struct PickerWindow {
    handle: AnyWindowHandle,
    panel: WeakEntity<ColorsPanel>,
}
impl Global for PickerWindow {}
struct SampleImage {
    pixels: image::RgbaImage,
    rendered: Arc<RenderImage>,
    name: String,
}
struct ColorsPanel {
    color: Rgba,
    target: ColorTarget,
    commands: Commands,
    saved: PickerState,
    mode: Mode,
    slider_model: usize,
    uppercase: bool,
    code_dirty: bool,
    hue: f32,
    saturation: f32,
    selected_swatch: usize,
    code: Entity<InputState>,
    search: Entity<InputState>,
    formats: Entity<Choice>,
    models: Entity<Choice>,
    opacity: Entity<SliderState>,
    brightness: Entity<SliderState>,
    channels: [Entity<SliderState>; 4],
    focus: FocusHandle,
    wheel_focus: FocusHandle,
    slider_focus: [FocusHandle; 6],
    wheel_bounds: Rc<Cell<Bounds<Pixels>>>,
    image_bounds: Rc<Cell<Bounds<Pixels>>>,
    wheel: Arc<RenderImage>,
    image: Option<SampleImage>,
    image_dragging: bool,
    wheel_dragging: bool,
    screen_sampling: bool,
    error: Option<String>,
    subscriptions: Vec<Subscription>,
}

/// Opens the shared Colors panel for text formatting or literal color-code insertion.
pub fn open(initial: Rgba, target: ColorTarget, commands: Commands, cx: &mut App) {
    open_with_code(initial, target, commands, None, cx);
}

/// Retains a selected color code's detected syntax and uppercase hex letters.
pub fn open_with_code(
    initial: Rgba,
    target: ColorTarget,
    commands: Commands,
    initial_code: Option<String>,
    cx: &mut App,
) {
    if let Some(previous) = cx.try_global::<PickerWindow>().cloned() {
        if let Some(panel) = previous.panel.upgrade() {
            if previous
                .handle
                .update(cx, |_, window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.target = target;
                        panel.commands = commands.clone();
                        panel.initial_color(initial, initial_code.as_deref(), window, cx);
                    });
                    window.activate_window();
                })
                .is_ok()
            {
                return;
            }
        }
    }
    let saved = match PickerState::load() {
        Ok(saved) => saved,
        Err(error) => {
            show_error(&error);
            return;
        }
    };
    let options = WindowOptions {
        kind: WindowKind::Dialog,
        window_bounds: Some(WindowBounds::centered(size(px(400.), px(630.)), cx)),
        titlebar: None,
        app_owns_titlebar_drag: true,
        window_decorations: Some(WindowDecorations::Client),
        is_resizable: false,
        is_minimizable: true,
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, move |window, cx| {
        window.set_window_title("Colors");
        cx.new(|cx| {
            ColorsPanel::new(
                initial,
                target,
                commands,
                saved,
                initial_code.as_deref(),
                window,
                cx,
            )
        })
    }) {
        Ok((handle, panel)) => cx.set_global(PickerWindow {
            handle,
            panel: panel.downgrade(),
        }),
        Err(error) => show_error(&error),
    }
}

impl ColorsPanel {
    fn new(
        initial: Rgba,
        target: ColorTarget,
        commands: Commands,
        mut saved: PickerState,
        initial_code: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        saved.swatches.resize(24, Rgba::from_rgb(PAPER));
        saved.swatches.truncate(24);
        let selected_format = ColorFormat::ALL
            .iter()
            .position(|candidate| *candidate == saved.format)
            .unwrap_or(0);
        let formats = cx.new(|cx| {
            SelectState::new(
                ColorFormat::ALL
                    .iter()
                    .map(|format| format.label())
                    .collect::<Vec<_>>(),
                Some(IndexPath::default().row(selected_format)),
                window,
                cx,
            )
        });
        let models = cx.new(|cx| {
            SelectState::new(
                CHANNEL_MODELS.to_vec(),
                Some(IndexPath::default()),
                window,
                cx,
            )
        });
        let code = cx.new(|cx| InputState::new(window, cx).placeholder("Color code"));
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search stylesheet colors"));
        let slider =
            |cx: &mut Context<Self>| cx.new(|_| SliderState::new().min(0.).max(100.).step(0.1));
        let opacity = slider(cx);
        let brightness = slider(cx);
        let channels = std::array::from_fn(|_| slider(cx));
        let mut panel = Self {
            color: initial,
            target,
            commands,
            saved,
            mode: Mode::Wheel,
            slider_model: 0,
            uppercase: false,
            code_dirty: false,
            hue: initial.hsv().0,
            saturation: initial.hsv().1,
            selected_swatch: 0,
            code,
            search,
            formats,
            models,
            opacity,
            brightness,
            channels,
            focus: cx.focus_handle(),
            wheel_focus: cx.focus_handle(),
            slider_focus: std::array::from_fn(|_| cx.focus_handle()),
            wheel_bounds: Rc::new(Cell::new(Bounds::default())),
            image_bounds: Rc::new(Cell::new(Bounds::default())),
            wheel: wheel_image(initial.hsv().2),
            image: None,
            image_dragging: false,
            wheel_dragging: false,
            screen_sampling: false,
            error: None,
            subscriptions: Vec::new(),
        };
        panel.initial_color(initial, initial_code, window, cx);
        panel.subscriptions.push(cx.subscribe_in(
            &panel.opacity,
            window,
            |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(value) = event {
                    let mut color = this.color;
                    color.a = (value.end() * 2.55).round().clamp(0., 255.) as u8;
                    this.set_color(color, window, cx);
                }
            },
        ));
        panel.subscriptions.push(cx.subscribe_in(
            &panel.brightness,
            window,
            |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(value) = event {
                    let (hue, saturation) = (this.hue, this.saturation);
                    this.set_color(
                        Rgba::from_hsv(hue, saturation, 1. - value.end() / 100., this.color.a),
                        window,
                        cx,
                    );
                }
            },
        ));
        for (index, state) in panel.channels.iter().enumerate() {
            panel.subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, _, event: &SliderEvent, window, cx| {
                    if let SliderEvent::Change(value) = event {
                        let mut channels = this.channel_values();
                        channels[index] = value.end() / 100.;
                        this.set_color(
                            from_channels(this.slider_model, channels, this.color.a),
                            window,
                            cx,
                        );
                    }
                },
            ));
        }
        panel.subscriptions.push(cx.subscribe_in(
            &panel.formats,
            window,
            |this, _, event: &SelectEvent<Vec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    if let Some(format) = ColorFormat::ALL
                        .into_iter()
                        .find(|format| format.label() == *label)
                    {
                        this.saved.format = format;
                        this.sync_code(window, cx);
                        this.persist(cx);
                    }
                }
            },
        ));
        panel.subscriptions.push(cx.subscribe_in(
            &panel.models,
            window,
            |this, _, event: &SelectEvent<Vec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    this.slider_model = CHANNEL_MODELS
                        .iter()
                        .position(|candidate| candidate == label)
                        .unwrap_or(0);
                    this.sync_sliders(window, cx);
                    cx.notify();
                }
            },
        ));
        panel.subscriptions.push(cx.subscribe_in(
            &panel.code,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.code_dirty = true;
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.apply(window, cx),
                InputEvent::Blur => {
                    let _ = this.parse_code(window, cx);
                }
                InputEvent::Focus => {}
            },
        ));
        panel
            .subscriptions
            .push(cx.subscribe(&panel.search, |_, _, _: &InputEvent, cx| cx.notify()));
        window.focus(&panel.wheel_focus, cx);
        panel
    }
    fn initial_color(
        &mut self,
        initial: Rgba,
        source: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.uppercase =
            source.is_some_and(|code| code.chars().any(|character| character.is_ascii_uppercase()));
        if let Some((color, format)) = source.and_then(|source| Rgba::parse(source).ok()) {
            self.saved.format = format;
            self.color = color;
        } else {
            self.color = initial;
        }
        self.hue = self.color.hsv().0;
        self.saturation = self.color.hsv().1;
        self.wheel = wheel_image(self.color.hsv().2);
        self.sync_sliders(window, cx);
        self.sync_code(window, cx);
        self.error = None;
        cx.notify();
    }
    fn set_color(&mut self, color: Rgba, window: &mut Window, cx: &mut Context<Self>) {
        if (self.color.hsv().2 - color.hsv().2).abs() > 0.002 {
            self.wheel = wheel_image(color.hsv().2);
        }
        let (hue, saturation, value) = color.hsv();
        if value > 0. {
            self.saturation = saturation;
            if saturation > 0. {
                self.hue = hue;
            }
        }
        self.color = color;
        self.saved.last_color = color;
        self.error = None;
        self.sync_sliders(window, cx);
        self.sync_code(window, cx);
        cx.notify();
    }
    fn sync_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values = self.channel_values();
        for (index, channel) in self.channels.iter().enumerate() {
            channel.update(cx, |state, cx| {
                state.set_value(values[index] * 100., window, cx)
            });
        }
        self.opacity.update(cx, |state, cx| {
            state.set_value(f32::from(self.color.a) / 2.55, window, cx)
        });
        self.brightness.update(cx, |state, cx| {
            state.set_value((1. - self.color.hsv().2) * 100., window, cx)
        });
    }
    fn sync_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saved.format == ColorFormat::Keyword
            && (self.color.a != 255
                || !named_colors()
                    .iter()
                    .any(|(_, rgb)| *rgb == self.color.rgb()))
        {
            self.saved.format = ColorFormat::Hex;
        }
        self.code_dirty = false;
        let text = self.saved.format.format(self.color, self.uppercase);
        self.code
            .update(cx, |state, cx| state.set_value(text, window, cx));
        let index = ColorFormat::ALL
            .iter()
            .position(|format| *format == self.saved.format)
            .unwrap_or(0);
        self.formats.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::default().row(index)), window, cx)
        });
    }
    fn parse_code(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.code_dirty {
            return true;
        }
        let value = self.code.read(cx).value().to_string();
        match Rgba::parse(value.trim()) {
            Ok((color, format)) => {
                self.uppercase = value
                    .chars()
                    .any(|character| character.is_ascii_uppercase());
                self.saved.format = format;
                self.set_color(color, window, cx);
                true
            }
            Err(error) => {
                self.error = Some(format!("{error}"));
                cx.notify();
                false
            }
        }
    }
    fn persist(&mut self, cx: &mut Context<Self>) -> bool {
        self.saved.last_color = self.color;
        match self.saved.save() {
            Ok(()) => true,
            Err(error) => {
                self.error = Some(format!("Saving colors: {error}"));
                cx.notify();
                false
            }
        }
    }
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.parse_code(window, cx) || !self.persist(cx) {
            return;
        }
        let code = self.code.read(cx).value().to_string();
        self.commands
            .borrow_mut()
            .push_back(Command::ApplyColor(self.target, self.color, code));
    }
    fn save_swatch(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(slot) = self.saved.swatches.get_mut(index) {
            *slot = self.color;
            self.selected_swatch = index;
            self.persist(cx);
            cx.notify();
        }
    }
    fn channel_values(&self) -> [f32; 4] {
        to_channels(self.slider_model, self.color)
    }
}

impl ColorsPanel {
    fn button(&self, id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Button {
        Button::new(id)
            .label(label)
            .with_size(Size::Small)
            .compact()
            .rounded(px(6.))
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(rgb(FIELD).into())
                    .foreground(rgb(INK).into())
                    .hover(rgb(0xe8e8e8).into())
                    .shadow(false),
            )
            .h(px(30.))
            .min_w(px(0.))
            .text_size(px(12.))
    }
    fn wheel_view(&self, cx: &mut Context<Self>) -> Div {
        let (hue, saturation) = (self.hue, self.saturation);
        let angle = hue / 360. * TAU;
        let x = (1. + angle.cos() * saturation) * WHEEL_SIZE / 2.;
        let y = (1. - angle.sin() * saturation) * WHEEL_SIZE / 2.;
        let bounds = self.wheel_bounds.clone();
        let (h, s, _) = self.color.hsv();
        let gradient = gradient_image(|t| Rgba::from_hsv(h, s, 1. - t, 255), false);
        div().flex().flex_col().items_center().gap(px(16.)).pt(px(10.))
            .child(div().id("color-wheel").role(Role::Slider).aria_label("Hue and saturation")
                .aria_value(format!("Hue {:.0} degrees, saturation {:.0} percent", hue, saturation * 100.))
                .aria_description("Left and Right change hue. Up and Down change saturation. Shift makes larger changes.")
                .track_focus(&self.wheel_focus).size(px(WHEEL_SIZE)).relative()
                .focus_visible(|style| style.border_1().border_color(rgb(0x76aebd)).rounded(px(95.)))
                .cursor(CursorStyle::Crosshair)
                .child(img(self.wheel.clone()).size_full())
                .child(canvas(move |rectangle, _, _| { bounds.set(rectangle); }, |_, _, _, _| {}).absolute().size_full())
                .child(div().absolute().left(px(x - 9.)).top(px(y - 9.)).size(px(18.))
                    .child(symbol(CROSSHAIR, if self.color.hsv().2 > 0.45 { 0x263918 } else { PAPER }).size(px(18.))))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, event, window, cx| {
                    window.focus(&this.wheel_focus, cx); this.wheel_dragging = true;
                    this.choose_wheel(event.position, window, cx);
                }))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                    if this.wheel_dragging && event.dragging() { this.choose_wheel(event.position, window, cx); }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.wheel_dragging = false))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.wheel_dragging = false))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    let (mut hue, mut saturation, value) = (this.hue, this.saturation, this.color.hsv().2);
                    let step = if event.keystroke.modifiers.shift { 10. } else { 1. };
                    match event.keystroke.key.as_str() {
                        "left" => hue = (hue - step).rem_euclid(360.), "right" => hue = (hue + step).rem_euclid(360.),
                        "up" => saturation = (saturation + step / 100.).min(1.),
                        "down" => saturation = (saturation - step / 100.).max(0.), _ => return,
                    }
                    this.hue = hue; this.saturation = saturation;
                    this.set_color(Rgba::from_hsv(hue, saturation, value, this.color.a), window, cx);
                    cx.stop_propagation();
                })))
            .child(gradient_slider("brightness-slider", "Brightness", &self.brightness, &self.slider_focus[0], gradient, cx).w_full())
    }
    fn choose_wheel(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.wheel_bounds.get();
        if f32::from(bounds.size.width) <= 0. {
            return;
        }
        let dx = f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width) * 2. - 1.;
        let dy = f32::from(position.y - bounds.origin.y) / f32::from(bounds.size.height) * 2. - 1.;
        let saturation = dx.hypot(dy).min(1.);
        let hue = (-dy).atan2(dx).to_degrees().rem_euclid(360.);
        self.hue = hue;
        self.saturation = saturation;
        self.set_color(
            Rgba::from_hsv(hue, saturation, self.color.hsv().2, self.color.a),
            window,
            cx,
        );
    }
    fn sliders_view(&self, cx: &mut Context<Self>) -> Div {
        let model = self.slider_model;
        let values = self.channel_values();
        let labels: &[&str] = match model {
            1 => &["Hue", "Saturation", "Brightness"],
            2 => &["Cyan", "Magenta", "Yellow", "Black"],
            3 => &["Brightness"],
            _ => &["Red", "Green", "Blue"],
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .pt(px(8.))
            .child(
                Select::new(&self.models)
                    .accessibility_label("Color slider model")
                    .with_size(Size::Small)
                    .w_full(),
            )
            .children(labels.iter().enumerate().map(|(index, label)| {
                let mut base = values;
                let alpha = self.color.a;
                let gradient = gradient_image(
                    move |t| {
                        base[index] = t;
                        from_channels(model, base, alpha)
                    },
                    false,
                );
                let number = if model == 0 {
                    format!("{:.0}", values[index] * 255.)
                } else if model == 1 && index == 0 {
                    format!("{:.0}°", values[index] * 360.)
                } else {
                    format!("{:.0}%", values[index] * 100.)
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(12.))
                            .child(*label)
                            .child(div().text_color(rgb(MUTED)).child(number)),
                    )
                    .child(gradient_slider(
                        ("color-channel", index),
                        label,
                        &self.channels[index],
                        &self.slider_focus[index + 2],
                        gradient,
                        cx,
                    ))
            }))
    }
    fn palette_view(&self, cx: &mut Context<Self>) -> Div {
        let query = self.search.read(cx).value().to_lowercase();
        let colors: Vec<_> = named_colors()
            .iter()
            .filter(|(name, _)| name.contains(&query))
            .copied()
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(8.))
            .child(
                Input::new(&self.search)
                    .with_size(Size::Small)
                    .cleanable(true),
            )
            .child(
                div()
                    .id("stylesheet-palette")
                    .role(Role::ListBox)
                    .aria_label("Stylesheet keyword colors")
                    .h(px(202.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when(colors.is_empty(), |list| {
                        list.child(
                            div()
                                .text_color(rgb(MUTED))
                                .child("No matching color names."),
                        )
                    })
                    .children(
                        colors
                            .into_iter()
                            .enumerate()
                            .map(|(index, (name, rgb_value))| {
                                self.button(("named-color", index), name, cx)
                                    .w_full()
                                    .h(px(25.))
                                    .justify_start()
                                    .icon(symbol(SWATCH, rgb_value))
                                    .accessibility_label(format!("{name}, #{rgb_value:06x}"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let mut color = Rgba::from_rgb(rgb_value);
                                        color.a = this.color.a;
                                        this.set_color(color, window, cx);
                                    }))
                            }),
                    ),
            )
    }
    fn image_view(&self, cx: &mut Context<Self>) -> Div {
        let mut view = div().flex().flex_col().gap(px(9.)).pt(px(8.)).child(
            self.button("load-palette-image", "Open Image…", cx)
                .w_full()
                .on_click(cx.listener(|this, _, window, cx| this.load_image(window, cx))),
        );
        if let Some(image) = &self.image {
            let width = image.pixels.width() as f32;
            let height = image.pixels.height() as f32;
            let scale = (344. / width).min(183. / height);
            let bounds = self.image_bounds.clone();
            view = view
                .child(
                    div()
                        .w_full()
                        .h(px(183.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("image-color-sampler")
                                .relative()
                                .w(px(width * scale))
                                .h(px(height * scale))
                                .role(Role::Image)
                                .aria_label(format!("Sample colors from {}", image.name))
                                .cursor(CursorStyle::Crosshair)
                                .child(img(image.rendered.clone()).size_full())
                                .child(
                                    canvas(
                                        move |rectangle, _, _| bounds.set(rectangle),
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full(),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event, window, cx| {
                                        this.image_dragging = true;
                                        this.choose_image(event.position, window, cx);
                                    }),
                                )
                                .on_mouse_move(cx.listener(
                                    |this, event: &MouseMoveEvent, window, cx| {
                                        if this.image_dragging && event.dragging() {
                                            this.choose_image(event.position, window, cx);
                                        }
                                    },
                                ))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, _| this.image_dragging = false),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, _| this.image_dragging = false),
                                ),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .truncate()
                        .child(image.name.clone()),
                );
        } else {
            view = view.child(
                div()
                    .h(px(183.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .text_size(px(12.))
                    .child("Open an image, then click a pixel to sample its color."),
            );
        }
        view
    }
    fn load_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a Color Palette Image")
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff"],
            )
            .pick_file()
        else {
            return;
        };
        match read_sample_image(&path) {
            Ok(image) => {
                self.image = Some(image);
                self.error = None;
                cx.notify();
            }
            Err(error) => {
                self.error = Some(format!("Opening image: {error}"));
                cx.notify();
            }
        }
        window.activate_window();
    }
    fn choose_image(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(image) = &self.image else {
            return;
        };
        let bounds = self.image_bounds.get();
        let w = f32::from(bounds.size.width);
        let h = f32::from(bounds.size.height);
        if w <= 0. || h <= 0. {
            return;
        }
        let x = ((f32::from(position.x - bounds.origin.x) / w).clamp(0., 0.999999)
            * image.pixels.width() as f32) as u32;
        let y = ((f32::from(position.y - bounds.origin.y) / h).clamp(0., 0.999999)
            * image.pixels.height() as f32) as u32;
        let pixel = image.pixels.get_pixel(x, y).0;
        self.set_color(
            Rgba {
                r: pixel[0],
                g: pixel[1],
                b: pixel[2],
                a: pixel[3],
            },
            window,
            cx,
        );
    }
    fn pencils_view(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(12.))
            .children((0..3).map(|row| {
                div()
                    .flex()
                    .justify_between()
                    .children((0..12).map(|column| {
                        let index = row * 12 + column;
                        let color = pencil_color(index);
                        Button::new(("pencil-color", index))
                            .accessibility_label(format!(
                                "Pencil color {}: {}",
                                index + 1,
                                color.hex(false, false)
                            ))
                            .tooltip(color.hex(false, false))
                            .ghost()
                            .compact()
                            .w(px(24.))
                            .h(px(59.))
                            .p_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(if self.color.rgb() == color.rgb() {
                                INK
                            } else {
                                PAPER
                            }))
                            .child(
                                div()
                                    .w(px(18.))
                                    .h(px(52.))
                                    .child(symbol(PENCIL, color.rgb()).w(px(18.)).h(px(52.))),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let mut color = color;
                                color.a = this.color.a;
                                this.set_color(color, window, cx);
                            }))
                    }))
            }))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .text_center()
                    .child("Choose a pencil color."),
            )
    }
}

impl Render for ColorsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = self.color;
        let accent = action_color(color);
        let mode = self.mode;
        let content = match mode {
            Mode::Wheel => self.wheel_view(cx),
            Mode::Sliders => self.sliders_view(cx),
            Mode::Palette => self.palette_view(cx),
            Mode::Image => self.image_view(cx),
            Mode::Pencils => self.pencils_view(cx),
        };
        let opacity = gradient_image(
            move |t| Rgba {
                a: (t * 255.).round() as u8,
                ..color
            },
            true,
        );
        let preview = gradient_image(move |_| color, true);
        let apply_label = if matches!(self.target, ColorTarget::InsertCode) {
            "Insert"
        } else {
            "Apply"
        };
        div()
            .id("colors-panel")
            .role(Role::Dialog)
            .aria_label("Colors")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(PAPER))
            .text_color(rgb(INK))
            .font_family(crate::fonts::INTER_FONT)
            .text_size(px(13.))
            .rounded(px(18.))
            .border_1()
            .border_color(rgb(0xc5c5c5))
            .overflow_hidden()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if this.persist(cx) {
                        window.remove_window();
                    }
                    cx.stop_propagation();
                } else if event.keystroke.key == "enter" && event.keystroke.modifiers.control {
                    this.apply(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .h(px(34.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px(px(13.))
                    .child(
                        div()
                            .w(px(65.))
                            .flex()
                            .gap(px(9.))
                            .items_center()
                            .child(
                                Button::new("close-colors")
                                    .accessibility_label("Close Colors")
                                    .tooltip("Close Colors")
                                    .ghost()
                                    .compact()
                                    .size(px(15.))
                                    .p_0()
                                    .rounded(px(8.))
                                    .bg(rgb(0xff6059))
                                    .border_1()
                                    .border_color(rgb(0xe44039))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.persist(cx) {
                                            window.remove_window();
                                        }
                                    })),
                            )
                            .child(
                                Button::new("minimize-colors")
                                    .accessibility_label("Minimize Colors")
                                    .tooltip("Minimize Colors")
                                    .ghost()
                                    .compact()
                                    .size(px(15.))
                                    .p_0()
                                    .rounded(px(8.))
                                    .bg(rgb(0xd9d9d9))
                                    .on_click(|_, window, _| window.minimize_window()),
                            )
                            .child(
                                div()
                                    .size(px(15.))
                                    .rounded(px(8.))
                                    .bg(rgb(0x28c840))
                                    .border_1()
                                    .border_color(rgb(0x1dad34)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .child("Colors")
                            .on_mouse_down(MouseButton::Left, |_, window, _| {
                                window.start_window_move()
                            }),
                    )
                    .child(div().w(px(65.))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(25.))
                    .pb(px(7.))
                    .h(px(58.))
                    .flex_shrink_0()
                    .children(
                        Mode::ALL
                            .into_iter()
                            .enumerate()
                            .map(|(index, (tab, label))| {
                                let selected = mode == tab;
                                Button::new(("color-mode", index))
                                    .accessibility_label(label)
                                    .tooltip(label)
                                    .role(Role::Tab)
                                    .toggled(selected)
                                    .ghost()
                                    .compact()
                                    .w(px(57.))
                                    .h(px(44.))
                                    .p(px(6.))
                                    .rounded(px(11.))
                                    .bg(rgb(if selected { 0xf5f5f5 } else { PAPER }))
                                    .child(if tab == Mode::Wheel {
                                        img(self.wheel.clone()).size(px(29.)).into_any_element()
                                    } else {
                                        symbol(
                                            match tab {
                                                Mode::Sliders => SLIDERS,
                                                Mode::Palette => PALETTE,
                                                Mode::Image => PHOTO,
                                                Mode::Pencils => PENCILS,
                                                Mode::Wheel => CROSSHAIR,
                                            },
                                            INK,
                                        )
                                        .size(px(32.))
                                        .into_any_element()
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.mode = tab;
                                        this.error = None;
                                        if tab == Mode::Wheel {
                                            window.focus(&this.wheel_focus, cx);
                                        }
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(rgb(0xe7e7e7)).flex_shrink_0())
            .child(div().px(px(30.)).h(px(256.)).flex_shrink_0().child(content))
            .child(
                div()
                    .px(px(30.))
                    .pt(px(10.))
                    .pb(px(12.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .child(div().text_size(px(14.)).child("Opacity"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(17.))
                            .child(
                                gradient_slider(
                                    "opacity-slider",
                                    "Opacity",
                                    &self.opacity,
                                    &self.slider_focus[1],
                                    opacity,
                                    cx,
                                )
                                .flex_1(),
                            )
                            .child(
                                div()
                                    .w(px(73.))
                                    .h(px(33.))
                                    .border_1()
                                    .border_color(rgb(0xe7e7e7))
                                    .rounded(px(9.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .child(format!("{:.0}%", f32::from(color.a) / 2.55)),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(25.))
                    .pt(px(4.))
                    .pb(px(15.))
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .flex_shrink_0()
                    .child(
                        div()
                            .border_2()
                            .border_color(rgb(if self.error.is_some() {
                                0xc74b43
                            } else {
                                color.composite(0xffffff)
                            }))
                            .rounded(px(12.))
                            .p(px(2.))
                            .h(px(44.))
                            .child(
                                Input::new(&self.code)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .text_center()
                                    .text_size(px(18.))
                                    .font_family(crate::fonts::PLAIN_FONT),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                Select::new(&self.formats)
                                    .accessibility_label("Color code format")
                                    .with_size(Size::Small)
                                    .h(px(31.))
                                    .flex_1(),
                            )
                            .child(
                                self.button("apply-color", apply_label, cx)
                                    .w(px(78.))
                                    .h(px(31.))
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(rgb(accent).into())
                                            .foreground(rgb(PAPER).into())
                                            .hover(rgb(darken(accent, 0.88)).into())
                                            .shadow(false),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.apply(window, cx)),
                                    ),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |root, error| {
                root.child(
                    div()
                        .id("color-error")
                        .role(Role::Alert)
                        .px(px(25.))
                        .text_size(px(10.))
                        .text_color(rgb(0xb83f38))
                        .child(error),
                )
            })
            .when(
                color.a < 255 && !matches!(self.target, ColorTarget::InsertCode),
                |root| {
                    root.child(
                        div()
                            .px(px(25.))
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child("Text formatting blends opacity into its background."),
                    )
                },
            )
            .child(div().flex_1().min_h(px(3.)))
            .child(
                div()
                    .mx(px(17.))
                    .h(px(1.))
                    .bg(rgb(0xe0e0e0))
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .px(px(25.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .flex_shrink_0()
                    .child(
                        Button::new("save-current-color")
                            .accessibility_label("Save current color to the selected swatch")
                            .tooltip("Save current color in the selected swatch")
                            .ghost()
                            .compact()
                            .w(px(48.))
                            .h(px(46.))
                            .p_0()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(darken(color.composite(PAPER), 0.7)))
                            .overflow_hidden()
                            .child(
                                div()
                                    .size_full()
                                    .relative()
                                    .child(img(preview).size_full())
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom(px(2.))
                                            .right(px(2.))
                                            .size(px(12.))
                                            .rounded(px(3.))
                                            .bg(rgb(PAPER))
                                            .child(symbol(PLUS, INK).size(px(12.))),
                                    ),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_swatch(this.selected_swatch, cx)
                            })),
                    )
                    .child(
                        Button::new("screen-color-sampler")
                            .accessibility_label("Pick a color from the screen")
                            .tooltip("Pick a color from the screen")
                            .ghost()
                            .compact()
                            .w(px(24.))
                            .h(px(30.))
                            .p_0()
                            .child(symbol(EYEDROPPER, 0x858585).size(px(21.)))
                            .disabled(self.screen_sampling)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.screen_sampling {
                                    return;
                                }
                                this.screen_sampling = true;
                                cx.notify();
                                let handle = Window::window_handle(window);
                                cx.spawn(async move |panel, cx| {
                                    // The overlay pumps messages on its own thread, keeping GPUI responsive.
                                    let result =
                                        smol::unblock(crate::color::pick_screen_color).await;
                                    let _ = handle.update(cx, |_, window, cx| {
                                        panel.update(cx, |this, cx| {
                                            this.screen_sampling = false;
                                            match result {
                                                Ok(Some(color)) => {
                                                    this.set_color(color, window, cx)
                                                }
                                                Ok(None) => {}
                                                Err(error) => {
                                                    this.error = Some(format!(
                                                        "Sampling screen color: {error}"
                                                    ))
                                                }
                                            }
                                            window.activate_window();
                                            cx.notify();
                                        })
                                    });
                                })
                                .detach();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .children((0..2).map(|row| {
                                div().flex().gap(px(3.)).children((0..12).map(|column| {
                                    let index = row * 12 + column;
                                    let swatch = self.saved.swatches[index];
                                    Button::new(("saved-color", index))
                                        .accessibility_label(format!(
                                            "Saved color {}: {}",
                                            index + 1,
                                            swatch.hex(true, false)
                                        ))
                                        .tooltip(format!(
                                            "{} — right-click to replace",
                                            swatch.hex(true, false)
                                        ))
                                        .ghost()
                                        .compact()
                                        .w(px(16.))
                                        .h(px(18.))
                                        .min_w(px(0.))
                                        .p_0()
                                        .rounded(px(3.))
                                        .border_1()
                                        .border_color(rgb(if index == self.selected_swatch {
                                            0x555555
                                        } else {
                                            0xbfc0c1
                                        }))
                                        .bg(rgb(swatch.composite(PAPER)))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.selected_swatch = index;
                                            this.set_color(swatch, window, cx);
                                        }))
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(move |this, _, _, cx| {
                                                this.save_swatch(index, cx);
                                                cx.stop_propagation();
                                            }),
                                        )
                                }))
                            })),
                    ),
            )
    }
}

/// Uses Kit's real slider track and thumb so pointer, touch and accessibility
/// actions share its authoritative value; the wrapper supplies keyboard movement.
fn gradient_slider(
    id: impl Into<ElementId>,
    label: &str,
    state: &Entity<SliderState>,
    focus: &FocusHandle,
    gradient: Arc<RenderImage>,
    cx: &App,
) -> Stateful<Div> {
    let percentage = state.read(cx).percentage().end;
    let keyboard = state.clone();
    let focus_on_pointer = focus.clone();
    div()
        .id(id)
        .track_focus(focus)
        .h(px(25.))
        .min_w(px(0.))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            window.focus(&focus_on_pointer, cx)
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let delta = if event.keystroke.modifiers.shift {
                10.
            } else {
                1.
            };
            let current = keyboard.read(cx).value().end();
            let value = match event.keystroke.key.as_str() {
                "left" | "down" => (current - delta).max(0.),
                "right" | "up" => (current + delta).min(100.),
                "home" => 0.,
                "end" => 100.,
                _ => return,
            };
            keyboard.update(cx, |state, cx| {
                state.set_value(value, window, cx);
                cx.emit(SliderEvent::Change(value.into()));
            });
            cx.stop_propagation();
        })
        .child(
            BaseSlider::new(state)
                .accessibility_label(label.to_owned())
                .w_full()
                .h_full()
                .child(
                    SliderTrack::new(state)
                        .relative()
                        .w_full()
                        .h_full()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .h(px(16.))
                                .rounded(px(4.))
                                .overflow_hidden()
                                .border_1()
                                .border_color(rgb(0xbcbcbc))
                                .child(img(gradient).size_full()),
                        )
                        .child(
                            SliderThumb::new(state)
                                .absolute()
                                .left(relative(percentage))
                                .ml(px(-6.))
                                .top(px(0.))
                                .w(px(12.))
                                .h(px(25.))
                                .rounded(px(6.))
                                .bg(rgb(PAPER))
                                .border_1()
                                .border_color(rgb(0xe0e0e0)),
                        ),
                ),
        )
}

fn render_image(mut rgba: image::RgbaImage) -> Arc<RenderImage> {
    // GPUI's RenderImage consumes BGRA frames, while image decoding is RGBA.
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(rgba)]))
}
fn wheel_image(value: f32) -> Arc<RenderImage> {
    let side = 380;
    let mut pixels = image::RgbaImage::new(side, side);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let dx = (x as f32 + 0.5) / side as f32 * 2. - 1.;
        let dy = (y as f32 + 0.5) / side as f32 * 2. - 1.;
        let radius = dx.hypot(dy);
        if radius > 1. {
            continue;
        }
        let hue = (-dy).atan2(dx).to_degrees().rem_euclid(360.);
        let color = Rgba::from_hsv(hue, radius, value, 255);
        let alpha = ((1. - radius) * side as f32).clamp(0., 1.) * 255.;
        *pixel = image::Rgba([color.r, color.g, color.b, alpha as u8]);
    }
    render_image(pixels)
}
fn gradient_image(mut color: impl FnMut(f32) -> Rgba, checkerboard: bool) -> Arc<RenderImage> {
    let mut pixels = image::RgbaImage::new(320, 16);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let color = color(x as f32 / 319.);
        if checkerboard {
            let background = if (x / 8 + y / 8) % 2 == 0 {
                PAPER
            } else {
                0xdcdcdc
            };
            let value = color.composite(background);
            *pixel = image::Rgba([(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]);
        } else {
            *pixel = image::Rgba([color.r, color.g, color.b, 255]);
        }
    }
    render_image(pixels)
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
    let rendered = render_image(pixels.clone());
    Ok(SampleImage {
        pixels,
        rendered,
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
fn action_color(color: Rgba) -> u32 {
    let rgb = color.composite(PAPER);
    let r = ((rgb >> 16) & 0xff) as f32 / 255.;
    let g = ((rgb >> 8) & 0xff) as f32 / 255.;
    let b = (rgb & 0xff) as f32 / 255.;
    // The selected hue stays visible while the white action label stays legible.
    let lightness = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if lightness > 0.52 {
        darken(rgb, 0.52 / lightness)
    } else {
        rgb
    }
}
fn darken(color: u32, scale: f32) -> u32 {
    let channel = |shift| ((((color >> shift) & 0xff) as f32 * scale).round() as u32).min(255);
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}
fn symbol(svg: &'static str, color: u32) -> Icon {
    Icon::default().data(svg.as_bytes()).text_color(rgb(color))
}
const CROSSHAIR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1"><circle cx="10" cy="10" r="6"/><path d="M10 0v20M0 10h20"/></svg>"##;
const SWATCH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><rect x="1" y="1" width="18" height="18" rx="3" fill="currentColor"/></svg>"##;
const PLUS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M8 3v10M3 8h10"/></svg>"##;
const EYEDROPPER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="m8 9 7 7m-5-5-6 6-1 4 4-1 6-6m-1-5 4-4a3 3 0 0 1 4 4l-4 4-4-4Z"/></svg>"##;
const SLIDERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 36 36" stroke-width="1"><rect x="2" y="6" width="32" height="5" rx="2" fill="#ffc8c8" stroke="#dc9696"/><rect x="2" y="16" width="32" height="5" rx="2" fill="#c7efcb" stroke="#94c29a"/><rect x="2" y="26" width="32" height="5" rx="2" fill="#ccccff" stroke="#9e9ed6"/><rect x="10" y="4" width="7" height="9" rx="2" fill="#ff595c" stroke="#f5f5f5"/><rect x="23" y="14" width="7" height="9" rx="2" fill="#56c869" stroke="#f5f5f5"/><rect x="14" y="24" width="7" height="9" rx="2" fill="#6962e4" stroke="#f5f5f5"/></svg>"##;
const PALETTE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 36 36"><path d="M9 7h24v27H9Z" fill="#eee" stroke="#999"/><path d="M3 2h24v27H3Z" fill="#fff" stroke="#aaa"/><rect x="6" y="5" width="8" height="8" rx="1" fill="#ff2e32"/><rect x="17" y="5" width="7" height="8" rx="1" fill="#16c84b"/><rect x="6" y="16" width="8" height="8" rx="1" fill="#ffd000"/><rect x="17" y="16" width="7" height="8" rx="1" fill="#4164ee"/><rect x="27" y="25" width="5" height="7" fill="#333"/></svg>"##;
const PHOTO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 36 36"><path d="M1 2h34v31H1Z" fill="#eee" stroke="#aaa"/><path d="M4 5h28v24H4Z" fill="#b1b7e4"/><path d="M4 16h28v13H4Z" fill="#f4b4a1"/><circle cx="25" cy="12" r="3" fill="#f7e7b0"/><path d="m4 26 10-10 8 7 4-4 6 8v2H4Z" fill="#65526f"/><path d="m4 29 12-8 8 7 8-3v4Z" fill="#354255"/></svg>"##;
const PENCILS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 36 36"><path d="M3 32V9l3-6 3 6v23" fill="#ef5552"/><path d="M9 32V9l3-6 3 6v23" fill="#edb13d"/><path d="M15 32V9l3-6 3 6v23" fill="#6fbd46"/><path d="M21 32V9l3-6 3 6v23" fill="#45aed2"/><path d="M27 32V9l3-6 3 6v23" fill="#a768c5"/><path d="M2 23h32v11H2Z" fill="#eee" stroke="#aaa"/></svg>"##;
const PENCIL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 18 52"><path d="m3 12 6-11 6 11v37H3Z" fill="currentColor"/><path d="m3 12 6-11 6 11Z" fill="#e9cba2"/><path d="m7 5 2-4 2 4Z" fill="currentColor"/><path d="M3 12h4v37H3Z" fill="#ffffff" opacity=".22"/><path d="M12 12h3v37h-3Z" fill="#000000" opacity=".16"/><path d="M3 49h12v2H3Z" fill="#e2e2e2"/></svg>"##;
