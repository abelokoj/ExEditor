use super::{EditorSurface, MarkedText};
use gpui_kit::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window};
use std::ops::Range;

impl EntityInputHandler for EditorSurface {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.editor.plain_text_snapshot();
        let (range, bytes) = utf16_range(&text, range);
        *adjusted_range = Some(range);
        text.get(bytes).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.editor.is_read_only() && !ignore_disabled_input {
            return None;
        }
        let session = self.editor.session();
        Some(UTF16Selection {
            range: self.editor.selection(),
            reversed: session.sel.focus < session.sel.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .filter(|marked| {
                marked.revision == self.editor.revision()
                    && marked.selection == self.editor.selection()
            })
            .map(|marked| marked.range.clone())
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            self.editor.session_mut().close_typing();
            self.completion_seen = None;
            self.changed(window, cx, false);
            self.refresh_completion(false);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.is_read_only() {
            return;
        }
        let marked = self.marked.take().filter(|marked| {
            marked.revision == self.editor.revision() && marked.selection == self.editor.selection()
        });
        let join = marked.is_some();
        let requested = range
            .or_else(|| marked.map(|marked| marked.range))
            .unwrap_or_else(|| self.editor.selection());
        let (range, _) = utf16_range(&self.editor.plain_text_snapshot(), requested);
        match self.editor.replace_ime(range, text, join) {
            Ok(()) => {
                if join {
                    self.editor.session_mut().close_typing();
                }
                self.completion_seen = None;
                self.changed(window, cx, true);
                let manual = self.completion.as_ref().is_some_and(|popup| popup.manual);
                self.refresh_completion(manual);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.is_read_only() {
            return;
        }
        let previous = self.marked.take().filter(|marked| {
            marked.revision == self.editor.revision() && marked.selection == self.editor.selection()
        });
        let join = previous.is_some();
        let requested = range
            .or_else(|| previous.as_ref().map(|marked| marked.range.clone()))
            .unwrap_or_else(|| self.editor.selection());
        let (range, _) = utf16_range(&self.editor.plain_text_snapshot(), requested);
        let start = range.start;
        if !join {
            self.editor.session_mut().close_typing();
        }
        match self.editor.replace_ime(range, new_text, join) {
            Ok(()) => {
                let len = new_text.encode_utf16().count();
                let selected = new_selected_range.unwrap_or(len..len);
                let (selected, _) = utf16_range(new_text, selected);
                let selection =
                    start.saturating_add(selected.start)..start.saturating_add(selected.end);
                if let Err(error) = self.editor.select_range(selection.clone()) {
                    self.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
                self.marked = (!new_text.is_empty()).then(|| MarkedText {
                    range: start..start.saturating_add(len),
                    selection,
                    revision: self.editor.revision(),
                });
                self.completion = None;
                self.completion_seen = None;
                self.changed(window, cx, true);
            }
            Err(error) => {
                self.marked = previous;
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if !range.is_empty() {
            return self.range_bounds(range).into_iter().next();
        }
        let position = self.editor.position_at_utf16(range.start).ok()?;
        let caret = self
            .layout
            .caret_on(&position, self.editor.session().page_hint)?;
        self.rect_bounds(
            caret.page,
            wordcraft_geom::Rect {
                x: caret.x,
                y: caret.top,
                w: caret.width.max(1. / self.scale),
                h: caret.height.max(1. / self.scale),
            },
        )
    }

    fn selection_bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.range_bounds(range)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let (_, position) = self.position_for_point(point)?;
        self.editor.utf16_at_position(&position)
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (range, _) = utf16_range(&self.editor.plain_text_snapshot(), range);
        if let Err(error) = self.editor.select_range(range) {
            self.error = Some(error.to_string());
            cx.notify();
            return;
        }
        self.marked = None;
        self.completion = None;
        self.changed(window, cx, false);
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.editor.plain_text_snapshot().encode_utf16().count())
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.editor.is_read_only() && self.editor.display_state().visible
    }
}

/// Normalize native UTF-16 ranges to complete scalar values, retaining UTF-8 byte boundaries.
pub(super) fn utf16_range(text: &str, requested: Range<usize>) -> (Range<usize>, Range<usize>) {
    let total = text.encode_utf16().count();
    let start = requested.start.min(requested.end).min(total);
    let end = requested.end.max(requested.start).min(total);
    let mut units = 0;
    let mut start_units = total;
    let mut start_byte = text.len();
    let mut end_units = total;
    let mut end_byte = text.len();
    let mut found_start = false;
    for (byte, ch) in text.char_indices() {
        let next = units + ch.len_utf16();
        if !found_start && (start == units || start < next) {
            start_units = units;
            start_byte = byte;
            found_start = true;
        }
        if end <= units {
            end_units = units;
            end_byte = byte;
            break;
        }
        if end < next {
            end_units = next;
            end_byte = byte + ch.len_utf8();
            break;
        }
        units = next;
    }
    (start_units..end_units, start_byte..end_byte)
}
