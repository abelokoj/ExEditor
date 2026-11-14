//! Offline language-tool settings and bounded background check state.
use crate::{
    app::Command,
    dialogs::Commands,
    proofing::{self, Issue, ProofingOptions, SpellingBackend},
    services::utf16_byte_offset,
    ui::{self, Palette, px},
};
use nsh_core::{desktop::WindowKind, prelude::*};
use std::{
    path::PathBuf,
    rc::Rc,
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

#[derive(Clone, PartialEq)]
enum DictionaryStatus {
    Checking,
    Missing,
    Downloading,
    Ready(PathBuf),
    Failed(String),
}
struct LanguageTools {
    draft: Signal<ProofingOptions>,
    language: Signal<String>,
    dictionary: Signal<String>,
    downloads: Signal<Vec<DictionaryStatus>>,
    error: Signal<String>,
    commands: Commands,
    colors: Palette,
}
pub fn open(options: ProofingOptions, commands: Commands, colors: Palette) {
    ui::show_panel(
        "language-tools",
        "Spelling and Grammar — ExEditor",
        620.,
        490.,
        WindowKind::Dialog,
        move || {
            let state = Rc::new(LanguageTools {
                language: signal(options.language.clone()),
                dictionary: signal(
                    options
                        .dictionary_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
                draft: signal(options.clone()),
                downloads: signal(vec![
                    DictionaryStatus::Checking;
                    crate::dictionaries::manifest().packs.len()
                ]),
                error: signal(String::new()),
                commands: commands.clone(),
                colors,
            });
            let downloads = state.downloads;
            spawn(async move {
                let statuses = smol::unblock(|| {
                    crate::dictionaries::manifest()
                        .packs
                        .iter()
                        .map(|pack| match crate::dictionaries::installed_path(pack) {
                            Ok(Some(path)) => DictionaryStatus::Ready(path),
                            Ok(None) => DictionaryStatus::Missing,
                            Err(error) => DictionaryStatus::Failed(format!("{error:#}")),
                        })
                        .collect()
                })
                .await;
                downloads.set(statuses);
            });
            let draft = state.draft;
            let error = state.error;
            let content = state.clone();
            let apply = state.clone();
            let shortcut = state.clone();
            on_key("mod+enter", move |_| shortcut.submit());
            ui::surface(colors).child(ui::heading("Spelling and Grammar"))
            .child(scroll().flex_1().min_h(px(0.)).child(view().gap(px(13.))
                .child(ui::checkbox("harper-grammar", "Check English grammar with Harper",
                    move || draft.with(|options| options.grammar), colors,
                    move |grammar| { draft.update(|options| options.grammar = grammar); error.set(String::new()); }))
                .child(ui::hint("Harper is built in and works offline. Turn it off for non-English documents.", colors))
                .child(ui::heading("Spelling engine"))
                .child(ui::radio_group("Spelling engine").flex_row().gap(px(12.))
                    .children([
                        (SpellingBackend::System,"System"), (SpellingBackend::Hunspell,"Hunspell"),
                        (SpellingBackend::Divvunspell,"Divvunspell"), (SpellingBackend::Voikko,"Voikko"),
                    ].into_iter().enumerate().map(|(index,(engine,label))| {
                        ui::radio(format!("spelling-engine-{index}"), label,
                            move || draft.with(|options| options.spelling == engine), colors,
                            move |_| { draft.update(|options| options.spelling = engine); error.set(String::new()); })
                    })))
                .child(ui::row("Spelling language:", ui::input("Spelling language, for example en-US", state.language, colors)))
                .child(dynamic(move || content.dictionary_controls()))
                .child(ui::error(state.error, colors))))
            .child(view().flex_row().justify_end().gap(px(8.))
                .child(ui::control("cancel-language-tools", "Cancel", 80., false, colors)
                    .on_press(|| ui::close_panel("language-tools")))
                .child(ui::control("apply-language-tools", "Apply", 80., true, colors)
                    .on_press(move || apply.submit())))
            .into_element()
        },
    );
}
impl LanguageTools {
    fn dictionary_controls(self: &Rc<Self>) -> AnyElement {
        let engine = self.draft.with(|options| options.spelling);
        let hint = match engine {
            SpellingBackend::System => {
                "Uses the operating system's spelling dictionary for this language."
            }
            SpellingBackend::Hunspell => {
                "Choose an .aff or .dic file; its matching partner must be beside it."
            }
            SpellingBackend::Divvunspell => "Choose a Divvun .bhfst or .zhfst dictionary archive.",
            SpellingBackend::Voikko => {
                "Choose the folder containing your Voikko dictionary packages."
            }
        };
        let mut content = view().gap(px(10.)).child(ui::hint(hint, self.colors));
        if engine != SpellingBackend::System {
            let state = self.clone();
            content = content.child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::input("Dictionary file or folder", self.dictionary, self.colors)
                            .flex_1(),
                    )
                    .child(
                        ui::button("browse-dictionary", "Browse…", 12.)
                            .on_press(move || state.browse()),
                    ),
            );
            content = content
                .child(ui::heading("Download dictionaries"))
                .child(ui::hint(
                    "Downloads come from ExEditor’s GitHub repository and include source and license notices. Nothing downloads until you choose it.",
                    self.colors,
                ))
                .child(ui::hint(
                    "Choose Use after downloading, then Apply. Selecting a non-English dictionary turns off English grammar checking.",
                    self.colors,
                ));
            for (index, pack) in crate::dictionaries::manifest()
                .packs
                .iter()
                .enumerate()
                .filter(|(_, pack)| pack.backend == engine)
            {
                let state = self.clone();
                content = content.child(dynamic(move || state.dictionary_row(index, pack)));
            }
        }
        content.into_element()
    }
    fn dictionary_row(
        self: &Rc<Self>,
        index: usize,
        pack: &'static crate::dictionaries::DictionaryPack,
    ) -> AnyElement {
        let current = self.downloads.with(|states| states[index].clone());
        let path = match &current {
            DictionaryStatus::Ready(path) => Some(path.clone()),
            _ => None,
        };
        let selected = path.as_ref().is_some_and(|path| {
            self.dictionary.get() == path.to_string_lossy()
                && self.language.get() == pack.language
                && self.draft.with(|options| options.spelling == pack.backend)
        });
        let label = match &current {
            DictionaryStatus::Checking => "Checking…",
            DictionaryStatus::Downloading => "Downloading…",
            DictionaryStatus::Ready(_) if selected => "Selected",
            DictionaryStatus::Ready(_) => "Use",
            DictionaryStatus::Missing => "Download",
            DictionaryStatus::Failed(_) => "Retry",
        };
        let disabled = selected
            || matches!(
                current,
                DictionaryStatus::Checking | DictionaryStatus::Downloading
            );
        let message = match &current {
            DictionaryStatus::Checking => "Checking downloaded files…".to_owned(),
            DictionaryStatus::Downloading => "Downloading and verifying files…".to_owned(),
            DictionaryStatus::Ready(_) if selected => {
                "Selected. Apply to use this dictionary.".to_owned()
            }
            DictionaryStatus::Ready(_) => "Downloaded and verified; available offline.".to_owned(),
            DictionaryStatus::Missing => String::new(),
            DictionaryStatus::Failed(error) => error.clone(),
        };
        let state = self.clone();
        let preview = if pack.pre_release { " · Preview" } else { "" };
        let failure = matches!(current, DictionaryStatus::Failed(_));
        view()
            .key(pack.id.clone())
            .gap(px(5.))
            .py(px(7.))
            .child(ui::heading(format!("{}{}", pack.name, preview)))
            .child(ui::hint(
                format!(
                    "{:.1} MB · {}",
                    pack.download_bytes() as f64 / 1_000_000.,
                    pack.license
                ),
                self.colors,
            ))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        ui::button(format!("dictionary-{}", pack.id), label, 12.)
                            .accessibility_label(format!("{label} {} dictionary", pack.name))
                            .disabled(disabled)
                            .on_press(move || {
                                if let Some(path) = &path {
                                    state.use_dictionary(pack, path);
                                } else {
                                    state.download_dictionary(index, pack);
                                }
                            }),
                    )
                    .child(
                        nsh_base::link()
                            .href(pack.source_url.clone())
                            .accessibility_label(format!("{} source and license", pack.name))
                            .text_color(ui::rgb(self.colors.accent))
                            .focus_visible(move |s| s.outline(px(2.)))
                            .child(text("Source and license").text_size(px(11.))),
                    ),
            )
            .child(
                text(message)
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(ui::rgb(if failure {
                        self.colors.danger
                    } else {
                        self.colors.muted
                    })),
            )
            .into_element()
    }
    fn download_dictionary(
        &self,
        index: usize,
        pack: &'static crate::dictionaries::DictionaryPack,
    ) {
        if self.downloads.with_untracked(|states| {
            matches!(
                states[index],
                DictionaryStatus::Checking
                    | DictionaryStatus::Downloading
                    | DictionaryStatus::Ready(_)
            )
        }) {
            return;
        }
        let downloads = self.downloads;
        downloads.update(|states| states[index] = DictionaryStatus::Downloading);
        spawn(async move {
            let result = smol::unblock(move || crate::dictionaries::download(pack)).await;
            downloads.update(|states| {
                states[index] = match result {
                    Ok(path) => DictionaryStatus::Ready(path),
                    Err(error) => DictionaryStatus::Failed(format!("{error:#}")),
                }
            });
        });
    }
    fn use_dictionary(&self, pack: &crate::dictionaries::DictionaryPack, path: &std::path::Path) {
        self.draft
            .update(|options| select_dictionary(options, pack.backend, &pack.language, path));
        self.language.set(pack.language.clone());
        self.dictionary.set(path.to_string_lossy().into_owned());
        self.error.set(String::new());
    }
    fn browse(&self) {
        let engine = self.draft.with_untracked(|options| options.spelling);
        let dictionary = self.dictionary;
        let error = self.error;
        spawn(async move {
            let dialog = rfd::AsyncFileDialog::new().set_title("Choose spelling dictionary");
            let selected = match engine {
                SpellingBackend::System => None,
                SpellingBackend::Hunspell => {
                    dialog
                        .add_filter("Hunspell dictionary", &["aff", "dic"])
                        .pick_file()
                        .await
                }
                SpellingBackend::Divvunspell => {
                    dialog
                        .add_filter("Divvun dictionary", &["bhfst", "zhfst"])
                        .pick_file()
                        .await
                }
                SpellingBackend::Voikko => dialog.pick_folder().await,
            };
            if let Some(selected) = selected {
                dictionary.set(selected.path().to_string_lossy().into_owned());
                error.set(String::new());
            }
        });
    }
    fn submit(&self) {
        let path = self.dictionary.get_untracked();
        let draft = self.draft.get_untracked();
        let options = ProofingOptions {
            language: self.language.get_untracked().trim().replace('_', "-"),
            dictionary_path: if draft.spelling == SpellingBackend::System || path.trim().is_empty()
            {
                None
            } else {
                Some(PathBuf::from(path.trim()))
            },
            ..draft
        };
        match options.validate() {
            Ok(()) => {
                self.commands
                    .borrow_mut()
                    .push_back(Command::ApplyLanguageTools(options));
                ui::close_panel("language-tools");
            }
            Err(error) => self.error.set(error.to_string()),
        }
    }
}
fn select_dictionary(
    options: &mut ProofingOptions,
    backend: SpellingBackend,
    language: &str,
    path: &std::path::Path,
) {
    options.spelling = backend;
    options.language = language.to_owned();
    options.dictionary_path = Some(path.to_path_buf());
    if !language
        .split(['-', '_'])
        .next()
        .is_some_and(|base| base.eq_ignore_ascii_case("en"))
    {
        options.grammar = false;
    }
}
#[cfg(test)]
mod tests {
    use super::{CheckState, PendingCheck, correction_matches};
    use crate::proofing::ProofingOptions;
    use std::sync::{Arc, mpsc};
    #[test]
    fn dictionary_selection_preserves_english_grammar_choice_and_disables_it_for_other_languages() {
        let mut options = ProofingOptions::default();
        let path = std::path::Path::new("downloaded/en_US.dic");
        super::select_dictionary(
            &mut options,
            crate::proofing::SpellingBackend::Hunspell,
            "en-US",
            path,
        );
        assert!(options.grammar);
        assert_eq!(options.dictionary_path.as_deref(), Some(path));
        options.grammar = false;
        super::select_dictionary(
            &mut options,
            crate::proofing::SpellingBackend::Hunspell,
            "en-GB",
            path,
        );
        assert!(!options.grammar, "an explicit grammar opt-out stays off");
        options.grammar = true;
        super::select_dictionary(
            &mut options,
            crate::proofing::SpellingBackend::Voikko,
            "fi",
            path,
        );
        assert!(!options.grammar);
        assert_eq!(options.language, "fi");
        assert_eq!(options.spelling, crate::proofing::SpellingBackend::Voikko);
    }
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
