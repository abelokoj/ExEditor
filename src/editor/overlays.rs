//! Display-only native decorations and clipboard validation, outside saved document history.
use super::{core_positions::TextMap, validate_native_resources, validate_text};
use nsh_base::rich_editor::{
    model::Node,
    state::{Decoration, DecorationSet, EditorState, KeyEvent, PluginSpec, Transaction},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    ops::Range,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub(super) struct Overlays {
    pub doc: Option<Node>,
    pub syntax: Vec<crate::syntax::Span>,
    pub proofing: Vec<Range<usize>>,
    pub error: Option<String>,
    pub read_only: bool,
}
pub(super) struct OverlayPlugin(pub Arc<Mutex<Overlays>>);
impl PluginSpec for OverlayPlugin {
    fn filter_transaction(&self, tr: &Transaction, _state: &EditorState) -> bool {
        if let Err(error) = validate_native_resources(tr.doc()) {
            self.0.lock().expect("editor overlay lock poisoned").error = Some(error.to_string());
            false
        } else {
            true
        }
    }
    fn handle_key_down(
        &self,
        state: &EditorState,
        event: &KeyEvent,
        dispatch: &mut dyn FnMut(Transaction),
    ) -> bool {
        if !event.mod_key
            || event.shift
            || event.alt
            || !matches!(event.key.as_str(), "c" | "C" | "x" | "X")
        {
            return false;
        }
        let cut = event.key.eq_ignore_ascii_case("x");
        let read_only = self
            .0
            .lock()
            .expect("editor overlay lock poisoned")
            .read_only;
        let result = if cut && read_only {
            Err(anyhow::anyhow!("the document is locked"))
        } else {
            super::clipboard::copy_state(state, cut, dispatch)
        };
        if let Err(error) = result {
            self.0.lock().expect("editor overlay lock poisoned").error = Some(error.to_string());
        }
        true
    }

    fn transform_pasted_html(&self, _state: &EditorState, html: String) -> String {
        match crate::formats::safe_native_html(&html) {
            Ok(html) => html,
            Err(error) => {
                self.0.lock().expect("editor overlay lock poisoned").error =
                    Some(error.to_string());
                String::new()
            }
        }
    }
    fn transform_pasted_text(&self, _state: &EditorState, text: String, _plain: bool) -> String {
        if let Err(error) = validate_text(&text) {
            self.0.lock().expect("editor overlay lock poisoned").error = Some(error.to_string());
            String::new()
        } else {
            text
        }
    }
    fn decorations(&self, state: &EditorState) -> Option<DecorationSet> {
        let overlay = self.0.lock().expect("editor overlay lock poisoned");
        if overlay.doc.as_ref() != Some(state.doc()) {
            return None;
        }
        let map = TextMap::new(state.doc());
        let mut decorations = Vec::new();
        let mut add = |range: Range<usize>, class: String| {
            if let (Ok(from), Ok(to)) = (map.position(range.start), map.position(range.end)) {
                if from < to {
                    decorations.push(Decoration::inline(
                        from,
                        to,
                        BTreeMap::from([("class".into(), json!(class))]),
                    ));
                }
            }
        };
        for span in &overlay.syntax {
            let mut class = format!("text-[#{:06x}]", span.color & 0xffffff);
            if span.bold {
                class.push_str(" font-bold");
            }
            if span.italic {
                class.push_str(" italic");
            }
            add(span.start..span.start + span.len, class);
        }
        for range in &overlay.proofing {
            add(
                range.clone(),
                "underline decoration-red-500 decoration-wavy".into(),
            );
        }
        Some(DecorationSet::create(state.doc(), decorations))
    }
}
