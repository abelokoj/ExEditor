//! Offline language-tool settings and bounded background check state.
use crate::{
    app::{Command, show_error},
    dialogs::Commands,
    proofing::{self, Issue, ProofingOptions, SpellingBackend},
    services::utf16_byte_offset,
    ui::{self, Palette, px},
};
use gpui_kit::component::{
    Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    checkbox::Checkbox,
    input::{Input, InputState},
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

struct PendingCheck {
    receiver: mpsc::Receiver<anyhow::Result<Vec<Issue>>>,
    text: Arc<str>,
    options: ProofingOptions,
}
pub struct CheckState {
    pub issues: Vec<Issue>,
    pub error: Option<String>,
    pub manual: bool,
    checked_text: Option<Arc<str>>,
    checked_options: Option<ProofingOptions>,
    pending: Option<PendingCheck>,
    changed: Instant,
}
impl Default for CheckState {
    fn default() -> Self {
        Self {
            issues: Vec::new(),
            error: None,
            manual: false,
            checked_text: None,
            checked_options: None,
            pending: None,
            changed: Instant::now(),
        }
    }
}
impl CheckState {
    pub fn invalidate(&mut self) {
        self.changed = Instant::now();
        self.checked_text = None;
        self.checked_options = None;
        self.issues.clear();
        self.error = None;
    }
    pub fn current(&self, text: &str, options: &ProofingOptions) -> bool {
        self.checked_text.as_deref() == Some(text) && self.checked_options.as_ref() == Some(options)
    }
    pub fn snapshot(&self) -> Option<Arc<str>> {
        self.checked_text.clone()
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn ready(&self) -> bool {
        self.manual || self.changed.elapsed() >= Duration::from_millis(700)
    }
    pub fn start(&mut self, text: Arc<str>, options: ProofingOptions) {
        if self.pending.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let job_text = text.clone();
        let job_options = options.clone();
        smol::spawn(async move {
            let result = smol::unblock(move || proofing::check(&job_text, &job_options)).await;
            let _ = sender.send(result);
        })
        .detach();
        self.pending = Some(PendingCheck {
            receiver,
            text,
            options,
        });
    }
    pub fn poll(&mut self, text: &str, options: &ProofingOptions) -> bool {
        let Some(job) = self.pending.as_ref() else {
            return false;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!(
                "The language check stopped. Check the document again."
            )),
        };
        let Some(job) = self.pending.take() else {
            return false;
        };
        if job.text.as_ref() != text || &job.options != options {
            return false;
        }
        self.checked_text = Some(job.text);
        self.checked_options = Some(job.options);
        match result {
            Ok(issues) => {
                self.issues = issues;
                self.error = None;
            }
            Err(error) => {
                self.issues.clear();
                self.error = Some(format!("{error:#}"));
            }
        }
        true
    }
}
/// Both the complete checked snapshot and exact UTF-16 range must still match.
pub fn correction_matches(
    current: &str,
    snapshot: &str,
    range: &std::ops::Range<usize>,
    original: &str,
) -> bool {
    if current != snapshot || range.start > range.end {
        return false;
    }
    let (Some(start), Some(end)) = (
        utf16_byte_offset(current, range.start),
        utf16_byte_offset(current, range.end),
    ) else {
        return false;
    };
    current.get(start..end) == Some(original)
}

struct LanguageTools {
    draft: ProofingOptions,
    language: Entity<InputState>,
    dictionary: Entity<InputState>,
    commands: Commands,
    colors: Palette,
    error: Option<String>,
}
pub fn open(options: ProofingOptions, commands: Commands, colors: Palette, cx: &mut App) {
    let window_options = WindowOptions {
        kind: WindowKind::Dialog,
        window_bounds: Some(WindowBounds::centered(size(px(610.), px(430.)), cx)),
        titlebar: None,
        app_owns_titlebar_drag: true,
        window_decorations: Some(WindowDecorations::Client),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let result = gpui_kit::open_window(window_options, cx, move |window, cx| {
        window.set_window_title("Spelling and Grammar — ExEditor");
        cx.new(|cx| {
            let language = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Spelling language, e.g. en-US")
                    .default_value(options.language.clone())
            });
            let dictionary = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Dictionary file or folder")
                    .default_value(
                        options
                            .dictionary_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    )
            });
            LanguageTools {
                draft: options,
                language,
                dictionary,
                commands,
                colors,
                error: None,
            }
        })
    });
    if let Err(error) = result {
        show_error(&error);
    }
}
impl LanguageTools {
    fn button(&self, id: &'static str, label: &'static str, cx: &App) -> Button {
        let c = self.colors;
        Button::new(id)
            .label(label)
            .with_size(Size::Small)
            .h(px(26.))
            .rounded(px(4.))
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(rgb(c.field).into())
                    .foreground(rgb(c.ink).into())
                    .hover(rgb(c.toolbar).into())
                    .shadow(false),
            )
    }
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = rfd::FileDialog::new().set_title("Choose spelling dictionary");
        let path = match self.draft.spelling {
            SpellingBackend::System => None,
            SpellingBackend::Hunspell => dialog
                .add_filter("Hunspell dictionary", &["aff", "dic"])
                .pick_file(),
            SpellingBackend::Divvunspell => dialog
                .add_filter("Divvun dictionary", &["bhfst", "zhfst"])
                .pick_file(),
            SpellingBackend::Voikko => dialog.pick_folder(),
        };
        if let Some(path) = path {
            self.dictionary.update(cx, |input, cx| {
                input.set_value(path.to_string_lossy().into_owned(), window, cx)
            });
            self.error = None;
            cx.notify();
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.dictionary.read(cx).value().trim().to_owned();
        let options = ProofingOptions {
            language: self.language.read(cx).value().trim().replace('_', "-"),
            dictionary_path: if self.draft.spelling == SpellingBackend::System || path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            },
            ..self.draft.clone()
        };
        match options.validate() {
            Ok(()) => {
                self.commands
                    .borrow_mut()
                    .push_back(Command::ApplyLanguageTools(options));
                window.remove_window();
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }
}
impl Render for LanguageTools {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(ui::rem_size());
        let c = self.colors;
        let hint = match self.draft.spelling {
            SpellingBackend::System => "Uses the Windows dictionary for the spelling language.",
            SpellingBackend::Hunspell => {
                "Choose an .aff or .dic file; its matching partner must be beside it."
            }
            SpellingBackend::Divvunspell => "Choose a Divvun .bhfst or .zhfst dictionary archive.",
            SpellingBackend::Voikko => {
                "Choose the folder containing your Voikko dictionary packages."
            }
        };
        let engines = [
            (SpellingBackend::System, "Windows", "engine-system"),
            (SpellingBackend::Hunspell, "Hunspell", "engine-hunspell"),
            (SpellingBackend::Divvunspell, "Divvunspell", "engine-divvun"),
            (SpellingBackend::Voikko, "Voikko", "engine-voikko"),
        ];
        div().id("language-tools").size_full().flex().flex_col().bg(rgb(c.chrome)).text_color(rgb(c.ink)).text_size(px(12.)).rounded(px(8.)).border_1().border_color(rgb(c.border)).overflow_hidden()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" { window.remove_window(); cx.stop_propagation(); }
                else if event.keystroke.key == "enter" && event.keystroke.modifiers.control { this.submit(window, cx); cx.stop_propagation(); }
            }))
            .child(div().h(px(35.)).flex_shrink_0().px(px(14.)).flex().items_center().window_control_area(WindowControlArea::Drag).border_b_1().border_color(rgb(c.border))
                .child(div().flex_1().child("Spelling and Grammar"))
                .child(self.button("close-language-tools", "Close", cx).on_click(|_, window, _| window.remove_window())))
            .child(div().id("language-tools-content").flex_1().min_h_0().overflow_y_scroll().p(px(18.)).flex().flex_col().gap(px(13.))
                .child(Checkbox::new("harper-grammar").label("Check English grammar with Harper").checked(self.draft.grammar).with_size(Size::Small).text_color(rgb(c.ink)).on_change(cx.listener(|this, value, _, cx| { this.draft.grammar = *value; this.error = None; cx.notify(); })))
                .child(div().text_color(rgb(c.muted)).child("Harper is built in and works offline. Turn it off for non-English documents."))
                .child(div().child("Spelling engine"))
                .child(div().flex().gap(px(6.)).children(engines.into_iter().map(|(engine, label, id)| {
                    self.button(id, label, cx).selected(self.draft.spelling == engine).on_click(cx.listener(move |this, _, _, cx| { this.draft.spelling = engine; this.error = None; cx.notify(); }))
                })))
                .child(div().flex().items_center().gap(px(10.)).child(div().w(px(125.)).child("Spelling language:"))
                    .child(div().flex_1().child(Input::new(&self.language).with_size(Size::Small))))
                .child(div().text_color(rgb(c.muted)).child(hint))
                .when(self.draft.spelling != SpellingBackend::System, |panel| panel.child(div().flex().items_center().gap(px(8.))
                    .child(div().flex_1().child(Input::new(&self.dictionary).with_size(Size::Small)))
                    .child(self.button("browse-dictionary", "Browse…", cx).on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))))))
                .when_some(self.error.clone(), |panel, error| panel.child(div().child(error))))
            .child(div().flex_shrink_0().border_t_1().border_color(rgb(c.border)).p(px(12.)).flex().items_center().justify_end().gap(px(8.))
                .child(self.button("cancel-language-tools", "Cancel", cx).on_click(|_, window, _| window.remove_window()))
                .child(self.button("apply-language-tools", "Apply", cx).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_results_are_discarded_after_text_or_options_change() {
        for changed_options in [false, true] {
            let options = ProofingOptions::default();
            let (sender, receiver) = mpsc::sync_channel(1);
            let mut state = CheckState {
                pending: Some(PendingCheck {
                    receiver,
                    text: Arc::from("old text"),
                    options: options.clone(),
                }),
                manual: true,
                ..CheckState::default()
            };
            sender.send(Ok(Vec::new())).expect("worker result");
            let mut current_options = options.clone();
            if changed_options {
                current_options.grammar = !current_options.grammar;
            }
            let text = if changed_options {
                "old text"
            } else {
                "new text"
            };
            assert!(!state.poll(text, &current_options));
            assert!(!state.current(text, &current_options));
            assert!(!state.pending());
            assert!(
                state.manual,
                "an explicit request can be retried on the new snapshot"
            );
        }
    }
    #[test]
    fn a_disconnected_worker_becomes_an_actionable_current_error() {
        let options = ProofingOptions::default();
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut state = CheckState {
            pending: Some(PendingCheck {
                receiver,
                text: Arc::from("text"),
                options: options.clone(),
            }),
            ..CheckState::default()
        };
        drop(sender);
        assert!(state.poll("text", &options));
        assert!(state.current("text", &options));
        assert!(
            state
                .error
                .as_deref()
                .is_some_and(|error| error.contains("stopped"))
        );
        assert!(!state.pending());
    }
    #[test]
    fn corrections_require_the_checked_document_and_complete_unicode_range() {
        let text = "😀 bad word";
        assert!(correction_matches(text, text, &(3..6), "bad"));
        assert!(!correction_matches(text, "😀 bad words", &(3..6), "bad"));
        assert!(!correction_matches(text, text, &(1..6), "bad"));
        assert!(!correction_matches(text, text, &(3..100), "bad"));
        assert!(!correction_matches(text, text, &(3..6), "word"));
    }
}
