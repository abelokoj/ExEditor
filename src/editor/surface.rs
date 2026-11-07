//! The native GPUI document surface: WordCraft pages, input services, and selection.
use super::Editor;
use crate::{
    completion::{self, Completion},
    ui::Palette,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use std::{
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use wordcraft_doc::{Document, Pos, StoryRef};
use wordcraft_geom::Rect;
use wordcraft_layout::DocLayout;

mod input;
#[cfg(test)]
mod tests;

const POINT_TO_PIXEL: f32 = 96. / 72.;
const TILE_PIXELS: f32 = 768.;
const SCROLLBAR: f32 = 12.;
const CONTEXT_ACTIONS: [(&str, &str); 6] = [
    ("Undo", "undo"),
    ("Cut", "cut"),
    ("Copy", "copy"),
    ("Paste", "paste"),
    ("Delete", "delete"),
    ("Select All", "select-all"),
];

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct TileKey {
    page: usize,
    x: usize,
    y: usize,
}

struct CompletionPopup {
    result: Completion,
    selected: usize,
    revision: u64,
    caret: usize,
    manual: bool,
}

struct MarkedText {
    range: Range<usize>,
    selection: Range<usize>,
    revision: u64,
}

#[derive(Clone)]
enum Drag {
    Text {
        anchor: Pos,
        start: Pos,
        end: Pos,
        unit: Option<&'static str>,
    },
    Scroll {
        vertical: bool,
        grab: f32,
    },
}

pub(crate) struct EditorSurface {
    editor: Rc<Editor>,
    focus: FocusHandle,
    subscriptions: Vec<Subscription>,
    bounds: Bounds<Pixels>,
    layout: Arc<DocLayout>,
    document: Option<Document>,
    frames: Vec<Bounds<Pixels>>,
    content: Size<Pixels>,
    scroll: Point<Pixels>,
    scale: f32,
    raster_scale: f32,
    cache_revision: u64,
    cache_layout: usize,
    cache: HashMap<TileKey, Arc<RenderImage>>,
    marked: Option<MarkedText>,
    completion: Option<CompletionPopup>,
    completion_seen: Option<(u64, Range<usize>)>,
    dismissed_completion: Option<(u64, usize)>,
    context_menu: Option<Point<Pixels>>,
    context_selection: usize,
    drag: Option<Drag>,
    pointer: Point<Pixels>,
    last_drag: Instant,
    focused: bool,
    caret_visible: bool,
    caret_changed: Instant,
    ensure_caret: bool,
    error: Option<String>,
    _ticker: Task<()>,
}

impl EditorSurface {
    pub(crate) fn new(editor: Rc<Editor>, cx: &mut Context<Self>) -> Self {
        let (x, y) = editor.view_scroll();
        let ticker = cx.spawn(async move |view, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(33)).await;
                if view
                    .update(cx, |this, cx| {
                        let blinking = this.focused && this.editor.selection().is_empty();
                        let visible = this.caret_changed.elapsed().as_millis() % 1_000 < 500;
                        if (blinking && visible != this.caret_visible) || this.drag.is_some() {
                            this.caret_visible = visible;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            editor,
            focus: cx.focus_handle(),
            subscriptions: Vec::new(),
            bounds: Bounds::default(),
            layout: Arc::new(DocLayout::default()),
            document: None,
            frames: Vec::new(),
            content: size(px(0.), px(0.)),
            scroll: point(px(x.max(0.)), px(y.max(0.))),
            scale: POINT_TO_PIXEL,
            raster_scale: 0.,
            cache_revision: u64::MAX,
            cache_layout: 0,
            cache: HashMap::new(),
            marked: None,
            completion: None,
            completion_seen: None,
            dismissed_completion: None,
            context_menu: None,
            context_selection: 0,
            drag: None,
            pointer: Point::default(),
            last_drag: Instant::now(),
            focused: false,
            caret_visible: true,
            caret_changed: Instant::now(),
            ensure_caret: false,
            error: None,
            _ticker: ticker,
        }
    }

    pub(crate) fn editor(&self) -> &Rc<Editor> {
        &self.editor
    }

    fn viewport(&self) -> Bounds<Pixels> {
        Bounds::new(
            self.bounds.origin,
            size(
                (self.bounds.size.width - px(SCROLLBAR)).max(px(1.)),
                (self.bounds.size.height - px(SCROLLBAR)).max(px(1.)),
            ),
        )
    }

    fn palette(&self) -> Palette {
        let state = self.editor.display_state();
        let color = state.background;
        let luma =
            ((color >> 16 & 255) * 299 + (color >> 8 & 255) * 587 + (color & 255) * 114) / 1_000;
        Palette::new(luma < 128)
    }

    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds = bounds;
        let state = self.editor.display_state();
        let scale = POINT_TO_PIXEL * f32::from(state.zoom) / 100.;
        let raster_scale = scale * window.scale_factor();
        self.editor
            .set_viewport_width(f32::from(self.viewport().size.width) / scale);
        let layout = self.editor.layout();
        let layout_id = Arc::as_ptr(&layout) as usize;
        if self.cache_revision != state.revision
            || self.cache_layout != layout_id
            || (self.raster_scale - raster_scale).abs() > 0.001
        {
            self.cache.clear();
            self.document = Some(self.editor.display_snapshot());
            self.cache_revision = state.revision;
            self.cache_layout = layout_id;
            self.raster_scale = raster_scale;
        }
        self.scale = scale;
        self.layout = layout;
        let gutter = if state.wrap_to_page && !state.plain {
            16.
        } else {
            0.
        };
        let width = self
            .layout
            .pages
            .iter()
            .map(|p| p.w * scale + gutter * 2.)
            .fold(f32::from(self.viewport().size.width), f32::max);
        let mut top = gutter;
        self.frames.clear();
        for page in &self.layout.pages {
            let page_width = page.w * scale;
            self.frames.push(Bounds::new(
                point(px((width - page_width) / 2.), px(top)),
                size(px(page_width), px(page.h * scale)),
            ));
            top += page.h * scale + gutter;
        }
        self.content = size(px(width), px(top));
        let (amount, page) = self.editor.take_scroll_delta();
        self.scroll.y += px(amount as f32
            * if page {
                f32::from(self.viewport().size.height) * 0.9
            } else {
                20.
            });
        self.clamp_scroll();
        if self.editor.take_scroll_request() {
            self.ensure_caret = true;
        }
        if self.ensure_caret {
            self.reveal_caret();
            self.ensure_caret = false;
        }
        if self.editor.take_focus_request() && state.visible {
            window.focus(&self.focus, cx);
            self.reset_caret();
        }
        self.focused = self.focus.is_focused(window);
        if !self.focused {
            self.drag = None;
            self.completion = None;
            self.context_menu = None;
        }
        if self.marked.as_ref().is_some_and(|marked| {
            marked.revision != state.revision || marked.selection != self.editor.selection()
        }) {
            self.marked = None;
            window.notify_text_input_changed(&self.focus);
        }
        if self.drag.is_some() {
            self.autoscroll(window, cx);
        }
        let selection = self.editor.selection();
        if self.completion_seen.as_ref() != Some(&(state.revision, selection.clone())) {
            self.completion_seen = Some((state.revision, selection));
            if let Some(manual) = self.completion.as_ref().map(|popup| popup.manual) {
                self.refresh_completion(manual);
            }
            // Popups use the new caret geometry on the next layout pass.
            if self.completion.is_some() {
                cx.notify();
            }
        }
        self.editor
            .set_view_scroll(f32::from(self.scroll.x), f32::from(self.scroll.y));
    }

    fn clamp_scroll(&mut self) {
        let viewport = self.viewport();
        self.scroll.x = self.scroll.x.clamp(
            px(0.),
            (self.content.width - viewport.size.width).max(px(0.)),
        );
        self.scroll.y = self.scroll.y.clamp(
            px(0.),
            (self.content.height - viewport.size.height).max(px(0.)),
        );
    }

    fn page_bounds(&self, page: usize) -> Option<Bounds<Pixels>> {
        let frame = self.frames.get(page)?;
        Some(Bounds::new(
            self.bounds.origin + frame.origin - self.scroll,
            frame.size,
        ))
    }

    fn caret_bounds(&self) -> Option<Bounds<Pixels>> {
        let (position, hint) = {
            let session = self.editor.session();
            (session.sel.focus.clone(), session.page_hint)
        };
        let caret = self.layout.caret_on(&position, hint)?;
        let page = self.page_bounds(caret.page)?;
        Some(Bounds::new(
            page.origin + point(px(caret.x * self.scale), px(caret.top * self.scale)),
            size(
                px((caret.width * self.scale).max(1.5)),
                px((caret.height * self.scale).max(1.5)),
            ),
        ))
    }

    fn reveal_caret(&mut self) {
        let Some(caret) = self.caret_bounds() else {
            return;
        };
        let viewport = self.viewport();
        let inset = px(6.);
        if caret.origin.x < viewport.origin.x + inset {
            self.scroll.x -= viewport.origin.x + inset - caret.origin.x;
        }
        if caret.origin.x + caret.size.width > viewport.origin.x + viewport.size.width - inset {
            self.scroll.x += caret.origin.x + caret.size.width
                - (viewport.origin.x + viewport.size.width - inset);
        }
        if caret.origin.y < viewport.origin.y + inset {
            self.scroll.y -= viewport.origin.y + inset - caret.origin.y;
        }
        if caret.origin.y + caret.size.height > viewport.origin.y + viewport.size.height - inset {
            self.scroll.y += caret.origin.y + caret.size.height
                - (viewport.origin.y + viewport.size.height - inset);
        }
        self.clamp_scroll();
    }

    fn rect_bounds(&self, page: usize, rect: Rect) -> Option<Bounds<Pixels>> {
        let bounds = self.page_bounds(page)?;
        Some(Bounds::new(
            bounds.origin + point(px(rect.x * self.scale), px(rect.y * self.scale)),
            size(px(rect.w * self.scale), px(rect.h * self.scale)),
        ))
    }

    fn range_bounds(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let Ok(start) = self.editor.position_at_utf16(range.start) else {
            return Vec::new();
        };
        let Ok(end) = self.editor.position_at_utf16(range.end) else {
            return Vec::new();
        };
        let Some(document) = self.document.as_ref() else {
            return Vec::new();
        };
        let hint = self.editor.session().page_hint;
        self.layout
            .selection_rects(document, &start, &end, hint)
            .into_iter()
            .filter_map(|(page, rect)| self.rect_bounds(page, rect))
            .collect()
    }

    fn position_for_point(&self, position: Point<Pixels>) -> Option<(usize, Pos)> {
        if self.frames.is_empty() {
            return None;
        }
        let page = self
            .frames
            .iter()
            .enumerate()
            .min_by(|(a, _), (b, _)| {
                let distance = |index| {
                    self.page_bounds(index).map_or(f32::MAX, |bounds| {
                        let y = f32::from(position.y);
                        (f32::from(bounds.origin.y) - y).max(0.)
                            + (y - f32::from(bounds.origin.y + bounds.size.height)).max(0.)
                    })
                };
                distance(*a).total_cmp(&distance(*b))
            })
            .map(|(index, _)| index)?;
        let bounds = self.page_bounds(page)?;
        let x = f32::from(position.x - bounds.origin.x) / self.scale;
        let y = f32::from(position.y - bounds.origin.y) / self.scale;
        let story = self.layout.story_at(page, x, y).unwrap_or(StoryRef::Body);
        self.layout.hit(page, x, y, story).map(|pos| (page, pos))
    }

    fn reset_caret(&mut self) {
        self.caret_visible = true;
        self.caret_changed = Instant::now();
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>, content: bool) {
        self.reset_caret();
        self.ensure_caret = true;
        self.error = None;
        if content {
            window.notify_text_input_changed(&self.focus);
        }
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn command(
        &mut self,
        id: &str,
        params: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let revision = self.editor.revision();
        match self.editor.run_command(id, &params) {
            Ok(_) => {
                self.marked = None;
                self.changed(window, cx, self.editor.revision() != revision);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn refresh_completion(&mut self, manual: bool) {
        let state = self.editor.display_state();
        let selection = self.editor.selection();
        if !self.focused
            || (!state.autocomplete_enabled && !manual)
            || state.read_only
            || self.marked.is_some()
            || !selection.is_empty()
            || (!manual && self.dismissed_completion == Some((state.revision, selection.start)))
        {
            self.completion = None;
            return;
        }
        let language =
            (!state.completion_language.is_empty()).then_some(state.completion_language.as_str());
        let result = completion::suggest(
            &self.editor.plain_text_snapshot(),
            selection.start,
            language,
            manual,
        );
        self.completion = result.map(|result| CompletionPopup {
            result,
            selected: 0,
            revision: state.revision,
            caret: selection.start,
            manual,
        });
        if manual {
            self.dismissed_completion = None;
        }
    }

    fn accept_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.completion.take() else {
            return;
        };
        if self.marked.is_some()
            || self.editor.is_read_only()
            || self.editor.revision() != popup.revision
            || self.editor.selection() != (popup.caret..popup.caret)
        {
            return;
        }
        let Some(candidate) = popup.result.candidates.get(popup.selected) else {
            return;
        };
        self.editor.session_mut().close_typing();
        let result = self
            .editor
            .select_range(popup.result.range)
            .and_then(|()| self.editor.replace_current(candidate));
        self.editor.session_mut().close_typing();
        match result {
            Ok(()) => {
                self.changed(window, cx, true);
                self.dismissed_completion =
                    Some((self.editor.revision(), self.editor.selection().start));
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let control = modifiers.control || modifiers.platform;
        if self.marked.is_some() {
            return;
        }
        if self.context_menu.is_some()
            && !control
            && !modifiers.alt
            && matches!(
                key,
                "up" | "down" | "home" | "end" | "enter" | "space" | " " | "escape"
            )
        {
            match key {
                "escape" => self.context_menu = None,
                "enter" | "space" | " " => {
                    self.activate_context(self.context_selection, window, cx)
                }
                _ => {
                    let backwards = matches!(key, "up" | "end");
                    let mut index = match key {
                        "home" => 5,
                        "end" => 0,
                        _ => self.context_selection,
                    };
                    for _ in 0..6 {
                        index = if backwards {
                            (index + 5) % 6
                        } else {
                            (index + 1) % 6
                        };
                        if self.context_enabled(index) {
                            break;
                        }
                    }
                    self.context_selection = index;
                }
            }
            cx.notify();
        } else if control && !modifiers.alt && !modifiers.shift && matches!(key, "space" | " ") {
            self.context_menu = None;
            self.refresh_completion(true);
            cx.notify();
        } else if self.completion.is_some()
            && !control
            && !modifiers.alt
            && !modifiers.shift
            && matches!(key, "up" | "down" | "tab" | "enter" | "escape")
        {
            match key {
                "up" | "down" => {
                    if let Some(popup) = &mut self.completion {
                        let len = popup.result.candidates.len();
                        if len > 0 {
                            popup.selected = if key == "up" {
                                (popup.selected + len - 1) % len
                            } else {
                                (popup.selected + 1) % len
                            };
                        }
                    }
                    cx.notify();
                }
                "tab" | "enter" => self.accept_completion(window, cx),
                _ => {
                    self.completion = None;
                    self.dismissed_completion =
                        Some((self.editor.revision(), self.editor.selection().start));
                    cx.notify();
                }
            }
        } else if self.context_menu.is_some() && key == "escape" {
            self.context_menu = None;
            cx.notify();
        } else if (key == "f10" && modifiers.shift && !control && !modifiers.alt)
            || key == "context-menu"
        {
            self.completion = None;
            self.context_selection = if self.editor.is_read_only() { 2 } else { 0 };
            self.context_menu = self
                .caret_bounds()
                .map(|caret| caret.origin - self.bounds.origin + point(px(0.), caret.size.height));
            cx.notify();
        } else if let Some(command) = local_command(key, control, modifiers.shift, modifiers.alt) {
            self.context_menu = None;
            self.completion = None;
            self.command(
                command,
                serde_json::json!({"extend": modifiers.shift}),
                window,
                cx,
            );
        } else {
            // Application commands bubble into the window's one shared shortcut state.
            if control || modifiers.alt {
                self.completion = None;
            }
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.completion = None;
        window.focus(&self.focus, cx);
        window.request_virtual_keyboard();
        self.focused = true;
        self.pointer = event.position;
        self.last_drag = Instant::now();
        if let Some((vertical, thumb)) = self.scrollbar_at(event.position) {
            let coordinate = if vertical {
                event.position.y
            } else {
                event.position.x
            };
            let start = if vertical {
                thumb.origin.y
            } else {
                thumb.origin.x
            };
            let length = if vertical {
                thumb.size.height
            } else {
                thumb.size.width
            };
            let grab = if coordinate >= start && coordinate <= start + length {
                f32::from(coordinate - start)
            } else {
                f32::from(length) / 2.
            };
            self.drag = Some(Drag::Scroll { vertical, grab });
            self.drag_scroll(event.position);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let Some((page, position)) = self.position_for_point(event.position) else {
            return;
        };
        self.marked = None;
        self.editor.session_mut().page_hint = page;
        let anchor = if event.modifiers.shift {
            self.editor.session().sel.anchor.clone()
        } else {
            position.clone()
        };
        if let Err(error) = self
            .editor
            .set_document_selection(anchor.clone(), position.clone())
        {
            self.error = Some(error.to_string());
            cx.notify();
            return;
        }
        let unit = match event.click_count {
            2 => Some("word"),
            3.. => Some("paragraph"),
            _ => None,
        };
        if let Some(unit) = unit {
            self.command(
                if unit == "word" {
                    "select.word"
                } else {
                    "select.paragraph"
                },
                serde_json::json!({}),
                window,
                cx,
            );
        }
        let (start, end) = self.editor.session().sel.ordered();
        self.drag = Some(Drag::Text {
            anchor,
            start,
            end,
            unit,
        });
        self.changed(window, cx, false);
        cx.stop_propagation();
    }

    fn right_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.completion = None;
        self.marked = None;
        self.drag = None;
        window.focus(&self.focus, cx);
        self.focused = true;
        let selected = self
            .range_bounds(self.editor.selection())
            .into_iter()
            .any(|bounds| bounds.contains(&event.position));
        if !selected && let Some((page, position)) = self.position_for_point(event.position) {
            self.editor.session_mut().page_hint = page;
            if let Err(error) = self
                .editor
                .set_document_selection(position.clone(), position)
            {
                self.error = Some(error.to_string());
            }
        }
        self.context_selection = if self.editor.is_read_only() { 2 } else { 0 };
        self.context_menu = Some(event.position - self.bounds.origin);
        self.changed(window, cx, false);
        cx.stop_propagation();
        window.prevent_default();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.is_none() {
            return;
        }
        if !event.dragging() {
            self.drag = None;
            cx.notify();
            return;
        }
        self.pointer = event.position;
        if matches!(self.drag, Some(Drag::Scroll { .. })) {
            self.drag_scroll(event.position);
            cx.notify();
        } else {
            self.drag_selection(event.position, window, cx);
        }
        cx.stop_propagation();
    }

    fn drag_selection(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Drag::Text {
            anchor,
            start,
            end,
            unit,
        }) = self.drag.clone()
        else {
            return;
        };
        let Some((page, position)) = self.position_for_point(point) else {
            return;
        };
        if position.story != anchor.story {
            return;
        }
        self.editor.session_mut().page_hint = page;
        let previous = self.editor.session().sel.clone();
        let result = if let Some(unit) = unit {
            self.editor
                .run_command(
                    "select.dragUnit",
                    &serde_json::json!({
                        "unit": unit, "start": start, "end": end, "pos": position,
                    }),
                )
                .map(|_| ())
        } else {
            self.editor.set_document_selection(anchor, position)
        };
        match result {
            Ok(()) => {
                if self.editor.session().sel != previous {
                    self.reset_caret();
                    window.notify_text_selection_changed(&self.focus);
                    cx.notify();
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.drag, Some(Drag::Text { .. })) {
            return;
        }
        let viewport = self.viewport();
        let delta = |at: Pixels, start: Pixels, length: Pixels| {
            if at < start {
                f32::from(at - start).max(-120.) * 12.
            } else if at > start + length {
                f32::from(at - start - length).min(120.) * 12.
            } else {
                0.
            }
        };
        let elapsed = self.last_drag.elapsed().as_secs_f32().min(0.1);
        self.last_drag = Instant::now();
        let previous = self.scroll;
        self.scroll.x +=
            px(delta(self.pointer.x, viewport.origin.x, viewport.size.width) * elapsed);
        self.scroll.y +=
            px(delta(self.pointer.y, viewport.origin.y, viewport.size.height) * elapsed);
        self.clamp_scroll();
        if self.scroll != previous {
            self.drag_selection(self.pointer, window, cx);
        }
    }

    fn scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        self.context_menu = None;
        self.completion = None;
        if event.modifiers.control || event.modifiers.platform {
            if delta.y != px(0.) {
                self.editor.enqueue_shortcut(if delta.y > px(0.) {
                    "zoom-in"
                } else {
                    "zoom-out"
                });
            }
        } else {
            if event.modifiers.shift && delta.x == px(0.) {
                self.scroll.x -= delta.y;
            } else {
                self.scroll.x -= delta.x;
                self.scroll.y -= delta.y;
            }
            self.clamp_scroll();
            self.editor
                .set_view_scroll(f32::from(self.scroll.x), f32::from(self.scroll.y));
        }
        cx.notify();
        cx.stop_propagation();
        window.prevent_default();
    }

    fn scrollbar(&self, vertical: bool) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        let view = self.viewport();
        let (length, content, scroll) = if vertical {
            (view.size.height, self.content.height, self.scroll.y)
        } else {
            (view.size.width, self.content.width, self.scroll.x)
        };
        if content <= length {
            return None;
        }
        let track = if vertical {
            Bounds::new(
                point(view.origin.x + view.size.width, view.origin.y),
                size(px(SCROLLBAR), length),
            )
        } else {
            Bounds::new(
                point(view.origin.x, view.origin.y + view.size.height),
                size(length, px(SCROLLBAR)),
            )
        };
        let thumb_length = (length * (length / content)).max(px(24.)).min(length);
        let position = (length - thumb_length) * (scroll / (content - length));
        let thumb = if vertical {
            Bounds::new(
                point(track.origin.x + px(3.), track.origin.y + position),
                size(px(SCROLLBAR - 6.), thumb_length),
            )
        } else {
            Bounds::new(
                point(track.origin.x + position, track.origin.y + px(3.)),
                size(thumb_length, px(SCROLLBAR - 6.)),
            )
        };
        Some((track, thumb))
    }

    fn scrollbar_at(&self, point: Point<Pixels>) -> Option<(bool, Bounds<Pixels>)> {
        for vertical in [true, false] {
            if let Some((track, thumb)) = self.scrollbar(vertical)
                && track.contains(&point)
            {
                return Some((vertical, thumb));
            }
        }
        None
    }

    fn drag_scroll(&mut self, point: Point<Pixels>) {
        let Some(Drag::Scroll { vertical, grab }) = self.drag else {
            return;
        };
        let Some((track, thumb)) = self.scrollbar(vertical) else {
            return;
        };
        let (coordinate, start, length, thumb_length, content) = if vertical {
            (
                point.y,
                track.origin.y,
                track.size.height,
                thumb.size.height,
                self.content.height,
            )
        } else {
            (
                point.x,
                track.origin.x,
                track.size.width,
                thumb.size.width,
                self.content.width,
            )
        };
        let fraction = if length > thumb_length {
            (f32::from(coordinate - start) - grab) / f32::from(length - thumb_length)
        } else {
            0.
        };
        let scroll = (content - length) * fraction.clamp(0., 1.);
        if vertical {
            self.scroll.y = scroll;
        } else {
            self.scroll.x = scroll;
        }
        self.clamp_scroll();
        self.editor
            .set_view_scroll(f32::from(self.scroll.x), f32::from(self.scroll.y));
    }
}

fn local_command(key: &str, control: bool, shift: bool, alt: bool) -> Option<&'static str> {
    if alt {
        return None;
    }
    Some(match (key, control, shift) {
        ("left", false, _) => "caret.left",
        ("right", false, _) => "caret.right",
        ("up", false, _) => "caret.up",
        ("down", false, _) => "caret.down",
        ("home", false, _) => "caret.home",
        ("end", false, _) => "caret.end",
        ("pageup", false, _) => "caret.pageUp",
        ("pagedown", false, _) => "caret.pageDown",
        ("left", true, _) => "caret.wordLeft",
        ("right", true, _) => "caret.wordRight",
        ("home", true, _) => "caret.docStart",
        ("end", true, _) => "caret.docEnd",
        ("backspace", false, _) => "text.backspace",
        ("delete", false, false) => "text.delete",
        ("backspace", true, _) => "text.deleteWordBack",
        ("delete", true, false) => "text.deleteWordForward",
        ("enter", false, false) => "text.newParagraph",
        ("enter", false, true) => "text.lineBreak",
        ("tab", false, false) => "text.tab",
        ("tab", false, true) => "text.backTab",
        _ => return None,
    })
}

impl Focusable for EditorSurface {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorSurface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.subscriptions.is_empty() {
            self.subscriptions
                .push(cx.on_focus(&self.focus.clone(), window, |this, _, cx| {
                    this.focused = true;
                    this.reset_caret();
                    cx.notify();
                }));
            self.subscriptions
                .push(cx.on_blur(&self.focus.clone(), window, |this, _, cx| {
                    this.focused = false;
                    this.drag = None;
                    this.completion = None;
                    this.context_menu = None;
                    this.marked = None;
                    this.editor.session_mut().close_typing();
                    cx.notify();
                }));
        }
        let view = cx.entity();
        let paint_view = view.clone();
        let palette = self.palette();
        let accessible_focus = cx.weak_entity();
        let accessible_value = cx.weak_entity();
        let mut root = div()
            .id("wordcraft-editor")
            .role(Role::TextInput)
            .accessibility_id("ExEditor.Document")
            .aria_label("Document")
            .when(window.is_a11y_active() || cfg!(test), |root| {
                root.aria_value(self.editor.plain_text_snapshot())
            })
            .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                let _ = accessible_focus.update(cx, |surface, cx| window.focus(&surface.focus, cx));
            })
            .on_a11y_action(AccessibleAction::SetValue, move |data, window, cx| {
                if let Some(accesskit::ActionData::Value(value)) = data {
                    let _ = accessible_value.update(cx, |surface, cx| {
                        if surface.editor.is_read_only() {
                            return;
                        }
                        surface.editor.session_mut().close_typing();
                        let result = surface
                            .editor
                            .select_range(0..surface.editor.text_length())
                            .and_then(|()| surface.editor.replace_current(value));
                        match result {
                            Ok(()) => surface.changed(window, cx, true),
                            Err(error) => {
                                surface.error = Some(error.to_string());
                                cx.notify();
                            }
                        }
                    });
                }
            })
            .relative()
            .size_full()
            .min_h(px(0.))
            .min_w(px(0.))
            .overflow_hidden()
            .bg(rgb(self.editor.display_state().background))
            .cursor_text()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_mouse_down))
            .on_scroll(cx.listener(Self::scroll))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        view.update(cx, |this, cx| this.prepare(bounds, window, cx));
                    },
                    move |_, _, window, cx| {
                        paint_view.update(cx, |this, cx| this.paint(window, cx));
                    },
                )
                .size_full(),
            );
        if let Some(popup) = &self.completion
            && let Some(caret) = self.caret_bounds()
        {
            let viewport = self.viewport();
            if caret.origin.y + caret.size.height >= viewport.origin.y
                && caret.origin.y <= viewport.origin.y + viewport.size.height
            {
                let row_height = f32::from(crate::ui::px(22.));
                let max_rows = ((f32::from(viewport.size.height) - 8.) / row_height)
                    .floor()
                    .max(1.) as usize;
                let count = popup.result.candidates.len().min(max_rows);
                let first = popup.selected.saturating_sub(count.saturating_sub(1));
                let height = row_height * count as f32 + 4.;
                let width = f32::from(crate::ui::px(260.))
                    .min(f32::from(viewport.size.width) - 8.)
                    .max(1.);
                let caret_y = f32::from(caret.origin.y - self.bounds.origin.y);
                let below = caret_y + f32::from(caret.size.height) + 2.;
                let top = if below + height <= f32::from(viewport.size.height) {
                    below
                } else {
                    (caret_y - height - 2.).max(0.)
                };
                let left = f32::from(caret.origin.x - self.bounds.origin.x)
                    .clamp(0., (f32::from(viewport.size.width) - width - 4.).max(0.));
                let mut menu = div()
                    .id("editor-completions")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .py(px(2.))
                    .bg(rgb(palette.field))
                    .border_1()
                    .border_color(rgb(palette.border))
                    .rounded(px(4.))
                    .text_color(rgb(palette.ink))
                    .text_size(crate::ui::px(11.))
                    .font_family(crate::fonts::PLAIN_FONT)
                    .cursor_default();
                for (index, candidate) in popup
                    .result
                    .candidates
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take(count)
                {
                    menu = menu.child(
                        div()
                            .id(("completion", index))
                            .h(px(row_height))
                            .px(px(7.))
                            .truncate()
                            .bg(rgb(if index == popup.selected {
                                0x2864b4
                            } else {
                                palette.field
                            }))
                            .text_color(rgb(if index == popup.selected {
                                0xffffff
                            } else {
                                palette.ink
                            }))
                            .child(candidate.clone())
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    if let Some(popup) = &mut this.completion {
                                        popup.selected = index;
                                    }
                                    this.accept_completion(window, cx);
                                    cx.stop_propagation();
                                    window.prevent_default();
                                }),
                            ),
                    );
                }
                root = root.child(menu);
            }
        }
        if let Some(position) = self.context_menu {
            let width = f32::from(crate::ui::px(170.));
            let height = f32::from(crate::ui::px(23.)) * 6. + 4.;
            let left = f32::from(position.x)
                .clamp(0., (f32::from(self.bounds.size.width) - width).max(0.));
            let top = f32::from(position.y)
                .clamp(0., (f32::from(self.bounds.size.height) - height).max(0.));
            let mut menu = div()
                .id("editor-context-menu")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .py(px(2.))
                .bg(rgb(palette.field))
                .text_color(rgb(palette.ink))
                .text_size(crate::ui::px(11.))
                .border_1()
                .border_color(rgb(palette.border))
                .rounded(px(4.))
                .cursor_default();
            for (index, (label, _)) in CONTEXT_ACTIONS.into_iter().enumerate() {
                let enabled = self.context_enabled(index);
                let selected = index == self.context_selection;
                menu = menu.child(
                    div()
                        .id(("editor-context", index))
                        .role(Role::MenuItem)
                        .aria_label(label)
                        .h(crate::ui::px(23.))
                        .px(px(8.))
                        .bg(rgb(if selected && enabled {
                            0x2864b4
                        } else {
                            palette.field
                        }))
                        .text_color(rgb(if !enabled {
                            palette.muted
                        } else if selected {
                            0xffffff
                        } else {
                            palette.ink
                        }))
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if *hovered && this.context_enabled(index) {
                                this.context_selection = index;
                                cx.notify();
                            }
                        }))
                        .child(label)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.activate_context(index, window, cx);
                                cx.stop_propagation();
                                window.prevent_default();
                            }),
                        ),
                );
            }
            root = root.child(menu);
        }
        if let Some(error) = &self.error {
            root = root.child(
                div()
                    .absolute()
                    .left(px(6.))
                    .bottom(px(SCROLLBAR + 4.))
                    .max_w_full()
                    .px(px(6.))
                    .py(px(3.))
                    .bg(rgb(palette.field))
                    .text_color(rgb(palette.ink))
                    .text_size(crate::ui::px(11.))
                    .child(error.clone()),
            );
        }
        root
    }
}

impl EditorSurface {
    fn context_enabled(&self, index: usize) -> bool {
        index < CONTEXT_ACTIONS.len() && (!self.editor.is_read_only() || matches!(index, 2 | 5))
    }
    fn activate_context(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        if !self.context_enabled(index) {
            cx.notify();
            return;
        }
        let action = CONTEXT_ACTIONS[index].1;
        if action == "delete" {
            self.command("text.delete", serde_json::json!({}), window, cx);
        } else {
            self.editor.enqueue_shortcut(action);
        }
        cx.notify();
    }

    pub(crate) fn complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.focused = true;
        self.context_menu = None;
        self.refresh_completion(true);
        cx.notify();
    }

    fn paint(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.editor.display_state();
        let palette = self.palette();
        let viewport = self.viewport();
        window.paint_quad(fill(self.bounds, rgb(state.background)));
        if !state.visible {
            return;
        }
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(self.bounds, cx.entity()),
            cx,
        );
        let view = cx.weak_entity();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                let _ = view.update(cx, |this, cx| this.mouse_move(event, window, cx));
            }
        });
        let view = cx.weak_entity();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                let _ = view.update(cx, |this, cx| {
                    this.drag = None;
                    cx.notify();
                });
            }
        });
        window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
            self.paint_pages(window);
            let selection = {
                let session = self.editor.session();
                session.sel.clone()
            };
            if !selection.is_collapsed()
                && let Some(document) = self.document.as_ref()
            {
                let hint = self.editor.session().page_hint;
                for (page, rect) in
                    self.layout
                        .selection_rects(document, &selection.anchor, &selection.focus, hint)
                {
                    if let Some(bounds) = self.rect_bounds(page, rect) {
                        let color = if self.focused { 0x368be066 } else { 0x8090a044 };
                        window.paint_quad(fill(bounds, rgba(color)));
                    }
                }
            }
            for range in &state.proofing {
                for bounds in self.range_bounds(range.clone()) {
                    if bounds.intersect(&viewport).size.height <= px(0.) {
                        continue;
                    }
                    let y = bounds.origin.y + bounds.size.height - px(1.);
                    let mut path = PathBuilder::stroke(px(1.));
                    path.move_to(point(bounds.origin.x, y));
                    let steps = (f32::from(bounds.size.width) / 2.).ceil() as usize;
                    for step in 1..=steps {
                        path.line_to(point(
                            (bounds.origin.x + px(step as f32 * 2.))
                                .min(bounds.origin.x + bounds.size.width),
                            y + px(if step % 2 == 0 { 0. } else { -1.5 }),
                        ));
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgb(0xe25151));
                    }
                }
            }
            if let Some(marked) = &self.marked {
                for bounds in self.range_bounds(marked.range.clone()) {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                bounds.origin.x,
                                bounds.origin.y + bounds.size.height - px(1.),
                            ),
                            size(bounds.size.width, px(1.)),
                        ),
                        rgb(palette.ink),
                    ));
                }
            }
            if self.focused
                && self.caret_visible
                && selection.is_collapsed()
                && !state.read_only
                && let Some(bounds) = self.caret_bounds()
            {
                window.paint_quad(fill(
                    bounds,
                    rgb(state.text_override.unwrap_or(palette.ink)),
                ));
            }
        });
        for vertical in [true, false] {
            if let Some((track, thumb)) = self.scrollbar(vertical) {
                window.paint_quad(fill(track, rgb(palette.toolbar)));
                window.paint_quad(fill(thumb, rgb(palette.muted)));
            }
        }
    }

    fn paint_pages(&mut self, window: &mut Window) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let viewport = self.viewport();
        let state = self.editor.display_state();
        let color = state.background;
        let paper = wordcraft_doc::props::Rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
        let options = wordcraft_render::RenderOptions {
            paper,
            read_paper: Some(paper),
            text_darkening: true,
            ..Default::default()
        };
        let tile_points = TILE_PIXELS / self.raster_scale.max(0.1);
        let mut visible_keys = std::collections::HashSet::new();
        for (page_index, page) in self.layout.pages.iter().enumerate() {
            let Some(bounds) = self.page_bounds(page_index) else {
                continue;
            };
            let visible = bounds.intersect(&viewport);
            if visible.size.width <= px(0.) || visible.size.height <= px(0.) {
                continue;
            }
            let tile_size = self.scale * tile_points;
            let left = (f32::from(visible.origin.x - bounds.origin.x) / tile_size)
                .floor()
                .max(0.) as usize;
            let top = (f32::from(visible.origin.y - bounds.origin.y) / tile_size)
                .floor()
                .max(0.) as usize;
            let right = (f32::from(visible.origin.x + visible.size.width - bounds.origin.x)
                / tile_size)
                .ceil() as usize;
            let bottom = (f32::from(visible.origin.y + visible.size.height - bounds.origin.y)
                / tile_size)
                .ceil() as usize;
            for y in top..bottom {
                for x in left..right {
                    let area = Rect {
                        x: x as f32 * tile_points,
                        y: y as f32 * tile_points,
                        w: tile_points.min(page.w - x as f32 * tile_points),
                        h: tile_points.min(page.h - y as f32 * tile_points),
                    };
                    if area.w <= 0. || area.h <= 0. {
                        continue;
                    }
                    let key = TileKey {
                        page: page_index,
                        x,
                        y,
                    };
                    visible_keys.insert(key);
                    if !self.cache.contains_key(&key) {
                        let rendered = wordcraft_render::render_area(
                            document,
                            page,
                            area,
                            self.raster_scale,
                            &options,
                        );
                        if let Some(image) = render_image(rendered) {
                            self.cache.insert(key, image);
                        }
                    }
                    if let Some(image) = self.cache.get(&key) {
                        let image_bounds = Bounds::new(
                            bounds.origin + point(px(area.x * self.scale), px(area.y * self.scale)),
                            size(px(area.w * self.scale), px(area.h * self.scale)),
                        );
                        if let Err(error) = window.paint_image(
                            viewport,
                            image_bounds,
                            Corners::default(),
                            image.clone(),
                            0,
                            false,
                        ) {
                            self.error = Some(format!("Could not draw the document: {error}"));
                        }
                    }
                }
            }
        }
        // Keep only viewport tiles: memory remains bounded even in a many-page document.
        self.cache.retain(|key, _| visible_keys.contains(key));
    }
}

fn render_image(mut rendered: wordcraft_render::Rendered) -> Option<Arc<RenderImage>> {
    if rendered.width == 0 || rendered.height == 0 {
        return None;
    }
    // WordCraft supplies premultiplied RGBA; GPUI's atlas consumes premultiplied BGRA.
    for pixel in rendered.pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(rendered.width, rendered.height, rendered.pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}
