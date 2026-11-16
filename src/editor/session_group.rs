//! One document and native undo history, with a selection and scroll counter per pane.
use super::overlays::Overlays;
use nsh_base::rich_editor::{
    extensions::history::history_key,
    state::{
        EditorState, EditorStateConfig, Plugin, PluginKey, PluginSpec, PluginState, StateError,
        Transaction, TransactionOrigin,
    },
    transform::Mapping,
    view::EditorSession,
};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
};

const MIRROR_META: &str = "exeditor.splitViewMirror";

#[derive(Default)]
struct HistorySlot {
    native: Option<Plugin>,
    value: Option<PluginState>,
}

struct SharedHistory {
    native: Plugin,
    slot: Arc<Mutex<HistorySlot>>,
}
impl PluginSpec for SharedHistory {
    fn key(&self) -> PluginKey {
        history_key()
    }
    fn init_state(&self, config: &EditorStateConfig, state: &EditorState) -> Option<PluginState> {
        let mut slot = self.slot.lock().expect("shared history lock poisoned");
        if slot.value.is_none() {
            slot.value = self.native.init_state(config, state);
        }
        slot.value.clone()
    }
    fn apply_state(
        &self,
        tr: &Transaction,
        value: &PluginState,
        old: &EditorState,
        new: &EditorState,
    ) -> PluginState {
        let mut slot = self.slot.lock().expect("shared history lock poisoned");
        let current = slot.value.as_ref().unwrap_or(value);
        if tr.get_meta(MIRROR_META) == Some(&Value::Bool(true)) {
            return current.clone();
        }
        // The original transaction retains native history's private typed undo/redo metadata.
        let next = self.native.apply_state(tr, current, old, new);
        slot.value = Some(next.clone());
        next
    }
}

/// Runs existing state updates and validation, but never repeats source append hooks.
struct StateOnly(Plugin);
impl PluginSpec for StateOnly {
    fn key(&self) -> PluginKey {
        self.0.key()
    }
    fn init_state(&self, config: &EditorStateConfig, state: &EditorState) -> Option<PluginState> {
        self.0.init_state(config, state)
    }
    fn apply_state(
        &self,
        tr: &Transaction,
        value: &PluginState,
        old: &EditorState,
        new: &EditorState,
    ) -> PluginState {
        self.0.apply_state(tr, value, old, new)
    }
    fn filter_transaction(&self, tr: &Transaction, state: &EditorState) -> bool {
        self.0.filter_transaction(tr, state)
    }
}

#[derive(Default)]
struct DispatchQueue {
    running: bool,
    pending: VecDeque<(EditorSession, Transaction)>,
}

/// Keeps a pane registered for its lifetime; the group retains only a weak reference.
pub(super) struct Member {
    session: EditorSession,
    group: Arc<SessionGroup>,
}

/// Synchronizes accepted native steps and history without copying another pane's view state.
pub(super) struct SessionGroup {
    members: Mutex<Vec<Weak<Member>>>,
    history: Arc<Mutex<HistorySlot>>,
    queue: Mutex<DispatchQueue>,
    overlays: Arc<Mutex<Overlays>>,
}
impl SessionGroup {
    pub(super) fn new(overlays: Arc<Mutex<Overlays>>) -> Arc<Self> {
        Arc::new(Self {
            members: Mutex::new(Vec::new()),
            history: Arc::new(Mutex::new(HistorySlot::default())),
            queue: Mutex::new(DispatchQueue::default()),
            overlays,
        })
    }

    /// Wrap plugins before creating/reconfiguring a native state; repeated wrapping is safe.
    pub(super) fn wrap_plugins(&self, plugins: Vec<Plugin>) -> Vec<Plugin> {
        plugins
            .into_iter()
            .map(|plugin| {
                if plugin.key() != history_key() {
                    return plugin;
                }
                let native = {
                    let mut slot = self.history.lock().expect("shared history lock poisoned");
                    slot.native.get_or_insert_with(|| plugin.clone()).clone()
                };
                Arc::new(SharedHistory {
                    native,
                    slot: self.history.clone(),
                }) as Plugin
            })
            .collect()
    }

    /// Joins a fresh pane. Retain the returned member for as long as that pane is open.
    pub(super) fn join(self: &Arc<Self>, session: EditorSession) -> Arc<Member> {
        // Initial construction may share a snapshot; edits below always use peer transactions.
        if let Some(first) = self.live_members().first() {
            session.update_state(first.session.state());
        }
        let member = Arc::new(Member {
            session: session.clone(),
            group: self.clone(),
        });
        self.members
            .lock()
            .expect("split member lock poisoned")
            .push(Arc::downgrade(&member));
        let weak = Arc::downgrade(&member);
        session.set_dispatch_transaction(Some(Arc::new(move |session, tr| {
            if let Some(member) = weak.upgrade() {
                member.group.enqueue(session.clone(), tr);
            }
        })));
        member
    }

    pub(super) fn reconfigure(&self, plugins: Vec<Plugin>) -> Result<(), StateError> {
        let plugins = self.wrap_plugins(plugins);
        let prepared = self
            .live_members()
            .into_iter()
            .map(|member| {
                member
                    .session
                    .state()
                    .reconfigure(plugins.clone())
                    .map(|state| (member, state))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (member, state) in prepared {
            member.session.update_state(state);
        }
        Ok(())
    }

    /// Loads a whole document into every pane and resets its shared history.
    /// The caller holds its loading flag while this method notifies the panes.
    pub(super) fn replace_state(&self, state: EditorState) -> Result<(), StateError> {
        let previous = self
            .history
            .lock()
            .expect("shared history lock poisoned")
            .value
            .take();
        let result = self.fresh_history(state);
        let state = match result {
            Ok(state) => state,
            Err(error) => {
                self.history
                    .lock()
                    .expect("shared history lock poisoned")
                    .value = previous;
                return Err(error);
            }
        };
        for member in self.live_members() {
            member.session.update_state(state.clone());
        }
        Ok(())
    }

    /// Clears undo and redo while keeping each pane's current selection and scroll counter.
    #[cfg(test)]
    pub(super) fn reset_history(&self) -> Result<(), StateError> {
        let previous = self
            .history
            .lock()
            .expect("shared history lock poisoned")
            .value
            .take();
        let prepared = self
            .live_members()
            .into_iter()
            .map(|member| {
                self.fresh_history(member.session.state())
                    .map(|state| (member, state))
            })
            .collect::<Result<Vec<_>, _>>();
        match prepared {
            Ok(prepared) => {
                for (member, state) in prepared {
                    member.session.update_state(state);
                }
                Ok(())
            }
            Err(error) => {
                self.history
                    .lock()
                    .expect("shared history lock poisoned")
                    .value = previous;
                Err(error)
            }
        }
    }

    fn fresh_history(&self, state: EditorState) -> Result<EditorState, StateError> {
        let plugins = self.wrap_plugins(state.plugins().to_vec());
        let without_history = plugins
            .iter()
            .filter(|plugin| plugin.key() != history_key())
            .cloned()
            .collect();
        state.reconfigure(without_history)?.reconfigure(plugins)
    }

    fn live_members(&self) -> Vec<Arc<Member>> {
        let mut members = self.members.lock().expect("split member lock poisoned");
        let mut live = Vec::with_capacity(members.len());
        members.retain(|member| match member.upgrade() {
            Some(member) => {
                live.push(member);
                true
            }
            None => false,
        });
        live
    }

    fn enqueue(&self, session: EditorSession, tr: Transaction) {
        {
            let mut queue = self.queue.lock().expect("split dispatch lock poisoned");
            queue.pending.push_back((session, tr));
            if queue.running {
                return;
            }
            queue.running = true;
        }
        loop {
            let work = {
                let mut queue = self.queue.lock().expect("split dispatch lock poisoned");
                match queue.pending.pop_front() {
                    Some(work) => work,
                    None => {
                        queue.running = false;
                        return;
                    }
                }
            };
            if let Err(error) = self.apply(work.0, work.1) {
                self.overlays
                    .lock()
                    .expect("editor overlay lock poisoned")
                    .error = Some(format!("The document edit could not be applied: {error}"));
            }
        }
    }

    fn apply(&self, source: EditorSession, tr: Transaction) -> Result<(), StateError> {
        let old = source.state();
        if tr.before() != old.doc() {
            return Err(StateError::MismatchedTransaction);
        }
        let previous = self
            .history
            .lock()
            .expect("shared history lock poisoned")
            .value
            .clone();
        let result = (|| {
            let (next, applied) = old.apply_transaction(tr)?;
            if applied.is_empty() {
                return Ok(None);
            }
            let mut peers = Vec::new();
            // Selection and stored marks belong to this pane, not to the shared document.
            if old.doc().ptr_eq(next.doc()) {
                return Ok(Some((next, applied, peers)));
            }
            for member in self.live_members() {
                if member.session == source {
                    continue;
                }
                let state = member.session.state();
                if state.doc() != old.doc() {
                    return Err(StateError::MismatchedTransaction);
                }
                let mut mirror = state.tr();
                let mut mapping = Mapping::new();
                for original in &applied {
                    for step in &original.steps {
                        mirror
                            .step(step.clone())
                            .map_err(|error| StateError::Invalid(error.to_string()))?;
                    }
                    mapping.append_mapping(&original.mapping);
                }
                mirror.mapping = mapping;
                mirror.set_meta(MIRROR_META, Value::Bool(true));
                mirror.set_add_to_history(false);
                mirror.set_origin(TransactionOrigin::Remote("split view".into()));
                let plugins = state.plugins().to_vec();
                let quiet = state.reconfigure(
                    plugins
                        .iter()
                        .map(|plugin| Arc::new(StateOnly(plugin.clone())) as Plugin)
                        .collect(),
                )?;
                let (mirrored, transactions) = quiet.apply_transaction(mirror)?;
                if transactions.is_empty() || mirrored.doc() != next.doc() {
                    return Err(StateError::Invalid(
                        "a split pane refused the synchronized edit".into(),
                    ));
                }
                peers.push((member, mirrored.reconfigure(plugins)?, transactions));
            }
            Ok(Some((next, applied, peers)))
        })();
        match result {
            Ok(Some((next, applied, peers))) => {
                // No group lock is held during callbacks; reentrant dispatch is queued.
                source.commit(next, &applied);
                for (member, state, transactions) in peers {
                    member.session.commit(state, &transactions);
                }
                Ok(())
            }
            Ok(None) => {
                self.history
                    .lock()
                    .expect("shared history lock poisoned")
                    .value = previous;
                Ok(())
            }
            Err(error) => {
                self.history
                    .lock()
                    .expect("shared history lock poisoned")
                    .value = previous;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_base::rich_editor::{
        commands::Dispatch,
        extensions::{
            history::{self, HistoryOptions},
            starter,
        },
        model::Fragment,
        state::TextSelection,
        view::NodeViews,
    };

    fn panes(
        text: &str,
    ) -> (
        Arc<SessionGroup>,
        EditorSession,
        Arc<Member>,
        EditorSession,
        Arc<Member>,
    ) {
        let group = SessionGroup::new(Arc::new(Mutex::new(Overlays::default())));
        let schema = starter::schema();
        let content = if text.is_empty() {
            Vec::new()
        } else {
            vec![schema.text(text, vec![])]
        };
        let paragraph = schema
            .node("paragraph", None, Fragment::from(content), vec![])
            .unwrap();
        let doc = schema
            .node("doc", None, Fragment::from(vec![paragraph]), vec![])
            .unwrap();
        let state = EditorState::create(
            EditorStateConfig::new(schema)
                .doc(doc)
                .plugins(group.wrap_plugins(vec![history::history(HistoryOptions::default())])),
        )
        .unwrap();
        let first = EditorSession::new(state, NodeViews::new());
        let first_member = group.join(first.clone());
        let second = EditorSession::new(first.state(), NodeViews::new());
        let second_member = group.join(second.clone());
        (group, first, first_member, second, second_member)
    }
    fn select(session: &EditorSession, at: usize, scroll: bool) {
        let state = session.state();
        let mut tr = state.tr();
        tr.set_selection(TextSelection::create(state.doc(), at, None));
        if scroll {
            tr.scroll_into_view();
        }
        session.dispatch(tr).unwrap();
    }
    fn type_text(session: &EditorSession, text: &str, time: u64) {
        let mut tr = session.state().tr();
        tr.insert_text(text, None, None);
        tr.set_time(time);
        tr.scroll_into_view();
        session.dispatch(tr).unwrap();
    }
    fn run(session: &EditorSession, command: fn(&EditorState, Dispatch<'_>) -> bool) {
        let mut transaction = None;
        assert!(command(
            &session.state(),
            Some(&mut |tr| transaction = Some(tr))
        ));
        session.dispatch(transaction.unwrap()).unwrap();
    }
    #[test]
    fn peer_steps_map_its_own_caret_without_copying_source_selection_or_scroll() {
        let (_group, first, _a, second, _b) = panes("abcdef");
        select(&second, 6, true);
        select(&first, 2, false);
        let peer_scroll = second.state().scroll_to_selection();
        type_text(&first, "X", 1000);
        assert_eq!(first.state().doc(), second.state().doc());
        assert_eq!(first.state().selection().head(), 3);
        assert_eq!(second.state().selection().head(), 7);
        assert_eq!(second.state().scroll_to_selection(), peer_scroll);
        let first_caret = first.state().selection().head();
        select(&second, 1, true);
        assert_eq!(first.state().selection().head(), first_caret);
        assert_eq!(first.state().scroll_to_selection(), 1);
    }
    #[test]
    fn either_pane_undoes_and_redoes_the_other_panes_edits() {
        let (_group, first, _a, second, _b) = panes("");
        type_text(&first, "first", 1000);
        type_text(&second, "second", 5000);
        assert_eq!(history::undo_depth(&first.state()), 2);
        assert_eq!(history::undo_depth(&second.state()), 2);
        run(&first, history::undo);
        assert_eq!(second.state().doc().text_content(), "first");
        run(&second, history::undo);
        assert_eq!(first.state().doc().text_content(), "");
        run(&first, history::redo);
        assert_eq!(second.state().doc().text_content(), "first");
        run(&second, history::redo);
        assert_eq!(first.state().doc().text_content(), "firstsecond");
        assert_eq!(
            history::undo_depth(&first.state()),
            history::undo_depth(&second.state())
        );
    }
    #[test]
    fn resetting_history_keeps_each_caret_and_closed_panes_are_not_retained() {
        let (group, first, _a, second, b) = panes("abcdef");
        type_text(&first, "X", 1000);
        select(&first, 2, true);
        select(&second, 6, true);
        let first_before = first.state();
        let second_before = second.state();
        group.reset_history().unwrap();
        assert_eq!(first.state().selection(), first_before.selection());
        assert_eq!(second.state().selection(), second_before.selection());
        assert_eq!(
            first.state().scroll_to_selection(),
            first_before.scroll_to_selection()
        );
        assert_eq!(
            second.state().scroll_to_selection(),
            second_before.scroll_to_selection()
        );
        assert_eq!(history::undo_depth(&first.state()), 0);
        assert_eq!(history::undo_depth(&second.state()), 0);
        let closed = Arc::downgrade(&b);
        drop(b);
        assert!(closed.upgrade().is_none());
        assert_eq!(group.live_members().len(), 1);
        type_text(&first, "Y", 5000);
        assert_ne!(first.state().doc(), second.state().doc());
    }
    struct AppendSuffix(Arc<std::sync::atomic::AtomicUsize>);
    impl PluginSpec for AppendSuffix {
        fn append_transaction(
            &self,
            transactions: &[Transaction],
            _old: &EditorState,
            new: &EditorState,
        ) -> Option<Transaction> {
            if transactions.iter().any(history::is_history_transaction)
                || new.doc().text_content() != "x"
            {
                return None;
            }
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut tr = new.tr();
            tr.insert_text("!", Some(new.doc().content().size() - 1), None);
            Some(tr)
        }
    }

    #[test]
    fn appended_source_steps_run_once_and_share_the_same_undo_event() {
        let group = SessionGroup::new(Arc::new(Mutex::new(Overlays::default())));
        let appends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let state = EditorState::create(EditorStateConfig::new(starter::schema()).plugins(
            group.wrap_plugins(vec![
                history::history(HistoryOptions::default()),
                Arc::new(AppendSuffix(appends.clone())),
            ]),
        ))
        .unwrap();
        let first = EditorSession::new(state, NodeViews::new());
        let _first_member = group.join(first.clone());
        let second = EditorSession::new(first.state(), NodeViews::new());
        let _second_member = group.join(second.clone());
        type_text(&first, "x", 1000);
        assert_eq!(first.state().doc().text_content(), "x!");
        assert_eq!(first.state().doc(), second.state().doc());
        assert_eq!(appends.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(history::undo_depth(&second.state()), 1);
        run(&second, history::undo);
        assert_eq!(first.state().doc().text_content(), "");
        assert_eq!(first.state().doc(), second.state().doc());
    }

    #[test]
    fn opening_another_pane_preserves_the_existing_native_undo_stack() {
        let (group, first, _a, second, _b) = panes("abc");
        type_text(&first, "X", 1000);
        let third = EditorSession::new(first.state(), NodeViews::new());
        let _third_member = group.join(third.clone());
        assert_eq!(history::undo_depth(&third.state()), 1);
        select(&third, 2, false);
        run(&third, history::undo);
        assert_eq!(first.state().doc().text_content(), "abc");
        assert_eq!(first.state().doc(), second.state().doc());
        assert_eq!(first.state().doc(), third.state().doc());
    }

    #[test]
    fn failed_source_application_leaves_documents_and_history_unchanged() {
        let (group, first, _a, second, _b) = panes("abc");
        let stale = first.state().tr();
        type_text(&first, "X", 1000);
        let before = first.state();
        let depth = history::undo_depth(&before);
        assert!(group.apply(second.clone(), stale).is_err());
        assert_eq!(first.state().doc(), before.doc());
        assert_eq!(second.state().doc(), before.doc());
        assert_eq!(history::undo_depth(&first.state()), depth);
        assert_eq!(history::undo_depth(&second.state()), depth);
    }
}
