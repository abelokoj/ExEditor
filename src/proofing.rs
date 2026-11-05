//! Offline English grammar and selectable dictionary-backed spelling engines.
//!
//! Harper parses plain English; source/Markdown masking is deliberately not applied here.
//! All ranges and replacement originals refer to the exact submitted UTF-16 document.

mod voikko;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{Read, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use anyhow::{Context, Result, bail, ensure};
use divvun_fst::{
    archive::SpellerArchive,
    speller::{Speller, SpellerConfig},
};
use harper_core::{
    Dialect, Document,
    linting::{Lint, LintGroup, LintKind, Linter, Suggestion},
    spell::FstDictionary,
};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use unicode_segmentation::UnicodeSegmentation as _;

pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_ISSUES: usize = 500;
const MAX_SUGGESTIONS: usize = 8;
const MAX_WORD_CHARS: usize = 255;
const MAX_DICTIONARY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_HUNSPELL_BYTES: u64 = 128 * 1024 * 1024;
const DIVVUN_SEARCH_BUDGET: u64 = 60_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpellingBackend {
    #[default]
    System,
    Hunspell,
    Divvunspell,
    Voikko,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProofingOptions {
    pub grammar: bool,
    pub spelling: SpellingBackend,
    pub language: String,
    pub dictionary_path: Option<PathBuf>,
}

impl Default for ProofingOptions {
    fn default() -> Self {
        Self {
            grammar: true,
            spelling: SpellingBackend::System,
            language: "en-US".into(),
            dictionary_path: None,
        }
    }
}

impl ProofingOptions {
    /// Validates configuration syntax without opening dictionaries or native libraries.
    pub fn validate(&self) -> Result<()> {
        let language = self.language.trim();
        ensure!(
            !language.is_empty() && language.len() <= 64,
            "Enter a language tag such as en-US, en-GB, fi, or se."
        );
        let mut segments = language.split(['-', '_']);
        let base = segments.next().unwrap_or("");
        ensure!(
            (2..=8).contains(&base.len()) && base.bytes().all(|byte| byte.is_ascii_alphabetic()),
            "The language must start with a two-to-eight-letter language code."
        );
        ensure!(
            segments.all(|segment| !segment.is_empty()
                && segment.len() <= 8
                && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())),
            "Use a valid language tag such as en-US or fi."
        );
        if let Some(path) = &self.dictionary_path {
            ensure!(
                !path.as_os_str().is_empty() && !path.to_string_lossy().contains('\0'),
                "Choose a valid dictionary file or folder."
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueKind {
    Grammar,
    Spelling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub range: Range<usize>,
    pub original: String,
    pub suggestions: Vec<String>,
    pub message: String,
    pub kind: IssueKind,
}

thread_local! {
    // Harper and Voikko handles are thread-confined; none enters the GUI or crosses worker threads.
    static GRAMMAR: RefCell<BTreeMap<u8, LintGroup>> = const { RefCell::new(BTreeMap::new()) };
    static DICTIONARY: RefCell<Option<CachedEngine>> = const { RefCell::new(None) };
}

/// Checks an exact document snapshot without network access or UI objects.
///
/// # Errors
/// Reports invalid input/configuration, absent dictionaries, unsupported dictionary data,
/// engine failures, or missing packaged native libraries. Harper supports English only.
pub fn check(text: &str, options: &ProofingOptions) -> Result<Vec<Issue>> {
    options.validate()?;
    ensure!(
        text.len() <= MAX_TEXT_BYTES,
        "Language tools check documents up to 1 MiB. Select a smaller document to keep checking responsive."
    );
    ensure!(
        !text.contains('\0'),
        "Language tools cannot check text containing NUL characters."
    );
    let language = options.language.trim().replace('_', "-");
    if options.grammar {
        ensure!(
            language
                .split('-')
                .next()
                .is_some_and(|base| base.eq_ignore_ascii_case("en")),
            "Harper grammar checking currently supports English. Turn off English grammar checking to use a non-English spelling dictionary."
        );
    }
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let index = TextIndex::new(text);
    let mut issues = if options.grammar {
        grammar_issues(text, &index, &language)?
    } else {
        Vec::new()
    };
    let spelling = if options.spelling == SpellingBackend::System {
        crate::services::check_spelling_for_language(text, &language)?
            .into_iter()
            .map(|issue| Issue {
                range: issue.range,
                message: format!("Check the spelling of “{}”.", issue.word),
                original: issue.word,
                suggestions: issue.suggestions,
                kind: IssueKind::Spelling,
            })
            .collect()
    } else {
        let key = dictionary_key(options, &language)?;
        DICTIONARY.with(|cache| -> Result<Vec<Issue>> {
            let mut cache = cache.borrow_mut();
            if cache.as_ref().is_none_or(|cached| cached.key != key) {
                let engine = Engine::open(&key)?;
                *cache = Some(CachedEngine { key, engine });
            }
            let engine = &mut cache
                .as_mut()
                .expect("the requested dictionary was initialized")
                .engine;
            spelling_issues(text, &index, |word| engine.word(word))
        })?
    };
    issues.extend(spelling);
    issues.sort_by(|left, right| {
        (left.range.start, left.range.end).cmp(&(right.range.start, right.range.end))
    });
    issues.dedup_by(|left, right| {
        left.range == right.range && left.kind == right.kind && left.message == right.message
    });
    issues.truncate(MAX_ISSUES);
    for issue in &mut issues {
        normalize_suggestions(&mut issue.suggestions, &issue.original);
    }
    Ok(issues)
}

struct TextIndex {
    chars: Vec<char>,
    bytes: Vec<usize>,
    utf16: Vec<usize>,
}
impl TextIndex {
    fn new(text: &str) -> Self {
        let mut chars = Vec::new();
        let mut bytes = Vec::new();
        let mut utf16 = Vec::new();
        let mut units = 0;
        for (offset, character) in text.char_indices() {
            chars.push(character);
            bytes.push(offset);
            utf16.push(units);
            units += character.len_utf16();
        }
        bytes.push(text.len());
        utf16.push(units);
        Self {
            chars,
            bytes,
            utf16,
        }
    }
    fn character_range(&self, range: Range<usize>) -> Result<Range<usize>> {
        ensure!(
            range.start <= range.end && range.end <= self.chars.len(),
            "The grammar engine returned an invalid character range."
        );
        Ok(self.utf16[range.start]..self.utf16[range.end])
    }
    fn byte_range(&self, range: Range<usize>) -> Result<Range<usize>> {
        let start = self.bytes.binary_search(&range.start).map_err(|_| {
            anyhow::anyhow!("The dictionary word starts inside a Unicode character")
        })?;
        let end = self
            .bytes
            .binary_search(&range.end)
            .map_err(|_| anyhow::anyhow!("The dictionary word ends inside a Unicode character"))?;
        self.character_range(start..end)
    }
}

fn dialect(language: &str) -> (u8, Dialect) {
    let region = language
        .split('-')
        .nth(1)
        .unwrap_or("")
        .to_ascii_uppercase();
    match region.as_str() {
        "GB" => (1, Dialect::British),
        "CA" => (2, Dialect::Canadian),
        "AU" | "NZ" => (3, Dialect::Australian),
        _ => (0, Dialect::American),
    }
}
fn grammar_issues(text: &str, index: &TextIndex, language: &str) -> Result<Vec<Issue>> {
    let (key, dialect) = dialect(language);
    let document = Document::new_plain_english_curated(text);
    let mut lints = GRAMMAR.with(|cache| {
        let mut cache = cache.borrow_mut();
        let linter = cache.entry(key).or_insert_with(|| {
            let mut group = LintGroup::new_curated(FstDictionary::curated(), dialect);
            group.config.set_rule_enabled("SpellCheck", false);
            group
        });
        linter.lint(&document)
    });
    lints.sort_by_key(|lint| (lint.span.start, lint.priority));
    lints
        .into_iter()
        .filter(|lint| lint.lint_kind != LintKind::Spelling)
        .take(MAX_ISSUES)
        .map(|lint| lint_issue(lint, index))
        .collect()
}
fn lint_issue(lint: Lint, index: &TextIndex) -> Result<Issue> {
    let range = index.character_range(lint.span.start..lint.span.end)?;
    let original: String = index.chars[lint.span.start..lint.span.end].iter().collect();
    let suggestions = lint
        .suggestions
        .iter()
        .map(|suggestion| match suggestion {
            Suggestion::ReplaceWith(characters) => characters.iter().collect(),
            Suggestion::Remove => String::new(),
            Suggestion::InsertAfter(characters) => {
                let mut replacement = original.clone();
                replacement.extend(characters);
                replacement
            }
        })
        .collect();
    Ok(Issue {
        range,
        original,
        suggestions,
        message: lint.message,
        kind: IssueKind::Grammar,
    })
}
fn normalize_suggestions(suggestions: &mut Vec<String>, original: &str) {
    let mut unique = Vec::new();
    for suggestion in suggestions.drain(..) {
        if suggestion != original && !suggestion.contains('\0') && !unique.contains(&suggestion) {
            unique.push(suggestion);
            if unique.len() == MAX_SUGGESTIONS {
                break;
            }
        }
    }
    *suggestions = unique;
}
fn spelling_issues(
    text: &str,
    index: &TextIndex,
    mut word: impl FnMut(&str) -> Result<Option<Vec<String>>>,
) -> Result<Vec<Issue>> {
    let mut issues = Vec::new();
    let mut checked: HashMap<&str, Option<Vec<String>>> = HashMap::new();
    for (start, original) in text.unicode_word_indices() {
        if !original.chars().any(char::is_alphabetic) || original.chars().count() > MAX_WORD_CHARS {
            continue;
        }
        let suggestions = if let Some(cached) = checked.get(original) {
            cached.clone()
        } else {
            let found = word(original)?;
            checked.insert(original, found.clone());
            found
        };
        if let Some(suggestions) = suggestions {
            issues.push(Issue {
                range: index.byte_range(start..start + original.len())?,
                original: original.into(),
                suggestions,
                message: format!("Check the spelling of “{original}”."),
                kind: IssueKind::Spelling,
            });
            if issues.len() == MAX_ISSUES {
                break;
            }
        }
    }
    Ok(issues)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct DictionaryKey {
    backend: SpellingBackend,
    language: String,
    root: PathBuf,
    files: Vec<FileStamp>,
}
struct CachedEngine {
    key: DictionaryKey,
    engine: Engine,
}
enum Engine {
    Hunspell(Box<spellbook::Dictionary>),
    Divvun(Box<Divvun>),
    Voikko(Box<voikko::Voikko>),
}
impl Engine {
    fn open(key: &DictionaryKey) -> Result<Self> {
        match key.backend {
            SpellingBackend::Hunspell => {
                let aff = read_bounded(&key.files[0].path, MAX_HUNSPELL_BYTES)?;
                let dic = read_bounded(&key.files[1].path, MAX_HUNSPELL_BYTES)?;
                let encoding = hunspell_encoding(&aff)?;
                let aff = decode_dictionary(&aff, encoding)?;
                let dic = decode_dictionary(&dic, encoding)?;
                let dictionary = spellbook::Dictionary::new(&aff, &dic).map_err(|error| {
                    anyhow::anyhow!("The Hunspell dictionary could not be parsed: {error}")
                })?;
                Ok(Self::Hunspell(Box::new(dictionary)))
            }
            SpellingBackend::Divvunspell => Ok(Self::Divvun(Box::new(Divvun::open(&key.root)?))),
            SpellingBackend::Voikko => Ok(Self::Voikko(Box::new(voikko::Voikko::open(
                &key.language,
                &key.root,
            )?))),
            SpellingBackend::System => bail!("System dictionaries are checked by Windows"),
        }
    }
    fn word(&mut self, word: &str) -> Result<Option<Vec<String>>> {
        match self {
            Self::Hunspell(dictionary) => {
                if dictionary.check(word) {
                    return Ok(None);
                }
                let mut suggestions = Vec::new();
                // Avoid the full dictionary scan used by ngram suggestions on every typed word.
                dictionary
                    .suggester()
                    .with_ngram_suggestions(false)
                    .suggest(word, &mut suggestions);
                normalize_suggestions(&mut suggestions, word);
                Ok(Some(suggestions))
            }
            Self::Divvun(dictionary) => dictionary.word(word),
            Self::Voikko(dictionary) => dictionary.word(word),
        }
    }
}

fn stamp(path: &Path, limit: u64) -> Result<FileStamp> {
    let path =
        fs::canonicalize(path).with_context(|| format!("Opening dictionary {}", path.display()))?;
    let metadata = fs::metadata(&path)?;
    ensure!(
        metadata.is_file(),
        "Choose a regular dictionary file: {}",
        path.display()
    );
    ensure!(
        metadata.len() > 0 && metadata.len() <= limit,
        "The dictionary file must be nonempty and no larger than {} MiB: {}",
        limit / (1024 * 1024),
        path.display()
    );
    Ok(FileStamp {
        path,
        length: metadata.len(),
        modified: metadata.modified()?,
    })
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file =
        File::open(path).with_context(|| format!("Reading dictionary {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= limit,
        "The dictionary exceeds its safety limit."
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "The dictionary grew beyond its safety limit while being read."
    );
    Ok(bytes)
}
fn dictionary_key(options: &ProofingOptions, language: &str) -> Result<DictionaryKey> {
    let selected = options.dictionary_path.as_deref().context(match options.spelling {
        SpellingBackend::Hunspell => "Choose a Hunspell .dic/.aff pair or the folder containing it in Language Tools.",
        SpellingBackend::Divvunspell => "Choose a Divvun .bhfst or .zhfst dictionary in Language Tools.",
        SpellingBackend::Voikko => "Choose a Voikko dictionary folder containing 5/mor-*/index.txt and mor.vfst in Language Tools.",
        SpellingBackend::System => "The system dictionary does not require a file.",
    })?;
    let selected = fs::canonicalize(selected).with_context(|| {
        format!(
            "The selected dictionary is unavailable: {}",
            selected.display()
        )
    })?;
    let (root, files) = match options.spelling {
        SpellingBackend::Hunspell => {
            let dictionary = hunspell_dictionary(&selected, language)?;
            let aff = stamp(&dictionary.with_extension("aff"), MAX_HUNSPELL_BYTES)?;
            let dic = stamp(&dictionary.with_extension("dic"), MAX_HUNSPELL_BYTES)?;
            (dic.path.clone(), vec![aff, dic])
        }
        SpellingBackend::Divvunspell => {
            ensure!(
                selected
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("bhfst")
                        || extension.eq_ignore_ascii_case("zhfst")),
                "Choose an actual .bhfst or .zhfst Divvun dictionary archive."
            );
            let file = stamp(&selected, MAX_DICTIONARY_BYTES)?;
            (selected, vec![file])
        }
        SpellingBackend::Voikko => {
            let root = if selected.join("5").is_dir() {
                selected
            } else if selected.file_name().is_some_and(|name| name == "5") {
                selected
                    .parent()
                    .context("The Voikko dictionary folder has no parent")?
                    .to_path_buf()
            } else if selected
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "5")
            {
                selected
                    .parent()
                    .and_then(Path::parent)
                    .context("The Voikko dictionary folder has no root")?
                    .to_path_buf()
            } else {
                bail!(
                    "Choose the Voikko dictionary root, its 5 folder, or a 5/mor-* dictionary folder."
                );
            };
            let mut files = Vec::new();
            collect_voikko(&root.join("5"), 0, &mut files)?;
            files.sort_by(|left, right| left.path.cmp(&right.path));
            ensure!(
                files.iter().any(|file| file
                    .path
                    .file_name()
                    .is_some_and(|name| name == "index.txt"))
                    && files
                        .iter()
                        .any(|file| file.path.file_name().is_some_and(|name| name == "mor.vfst")),
                "The selected Voikko folder is missing its index.txt or mor.vfst morphology dictionary."
            );
            let total = files.iter().try_fold(0_u64, |total, file| {
                total
                    .checked_add(file.length)
                    .context("The Voikko dictionary is too large")
            })?;
            ensure!(
                total <= MAX_DICTIONARY_BYTES,
                "The Voikko dictionary exceeds the 512 MiB safety limit."
            );
            (root, files)
        }
        SpellingBackend::System => bail!("Windows dictionaries do not use file-backed engines"),
    };
    Ok(DictionaryKey {
        backend: options.spelling,
        language: language.into(),
        root,
        files,
    })
}
fn hunspell_dictionary(selected: &Path, language: &str) -> Result<PathBuf> {
    if selected.is_file() {
        ensure!(
            selected
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dic")
                    || extension.eq_ignore_ascii_case("aff")),
            "Choose a Hunspell .dic or .aff file; the matching partner must be beside it."
        );
        return Ok(selected.with_extension("dic"));
    }
    ensure!(
        selected.is_dir(),
        "The Hunspell dictionary folder is unavailable."
    );
    for basename in [language.to_owned(), language.replace('-', "_")] {
        let candidate = selected.join(basename).with_extension("dic");
        if candidate.is_file() && candidate.with_extension("aff").is_file() {
            return Ok(candidate);
        }
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(selected)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dic"))
            && entry.path().with_extension("aff").is_file()
        {
            candidates.push(entry.path());
        }
        ensure!(
            candidates.len() <= 256,
            "This folder contains too many dictionaries. Select the desired .dic file instead."
        );
    }
    ensure!(
        candidates.len() == 1,
        "The folder does not contain exactly one matching Hunspell pair for {language}. Select the desired .dic file directly."
    );
    Ok(candidates.remove(0))
}
fn collect_voikko(folder: &Path, depth: usize, files: &mut Vec<FileStamp>) -> Result<()> {
    ensure!(
        depth <= 8,
        "The Voikko dictionary folder is nested too deeply."
    );
    for entry in fs::read_dir(folder).context("Reading the Voikko dictionary folder")? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "Voikko dictionary folders must not contain symbolic links."
        );
        if kind.is_dir() {
            collect_voikko(&entry.path(), depth + 1, files)?;
        } else if kind.is_file() {
            files.push(stamp(&entry.path(), MAX_DICTIONARY_BYTES)?);
            ensure!(
                files.len() <= 1024,
                "The Voikko dictionary folder contains too many files."
            );
        }
    }
    Ok(())
}
fn hunspell_encoding(aff: &[u8]) -> Result<&'static encoding_rs::Encoding> {
    for line in aff.split(|byte| *byte == b'\n') {
        let fields: Vec<&[u8]> = line
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty())
            .collect();
        if fields
            .first()
            .is_some_and(|field| field.eq_ignore_ascii_case(b"SET"))
        {
            let label = fields
                .get(1)
                .context("The Hunspell SET directive has no encoding")?;
            return encoding_rs::Encoding::for_label(label).context("The Hunspell dictionary uses an unsupported text encoding. Convert both .aff and .dic files to UTF-8 first.");
        }
    }
    Ok(encoding_rs::UTF_8)
}
fn decode_dictionary(bytes: &[u8], encoding: &'static encoding_rs::Encoding) -> Result<String> {
    let (text, _, errors) = encoding.decode(bytes);
    ensure!(
        !errors,
        "The dictionary is not valid in its declared encoding; no replacement characters were inserted."
    );
    Ok(text.into_owned())
}

struct Divvun {
    speller: Arc<dyn Speller + Send + Sync>,
    config: SpellerConfig,
    _archive: Arc<dyn SpellerArchive + Send + Sync>,
    _source: NamedTempFile,
}
impl Divvun {
    fn open(path: &Path) -> Result<Self> {
        let before = stamp(path, MAX_DICTIONARY_BYTES)?;
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut source = tempfile::Builder::new()
            .prefix("exeditor-speller-")
            .suffix(&format!(".{extension}"))
            .tempfile()
            .context("Making a private dictionary snapshot")?;
        let copied = std::io::copy(
            &mut File::open(path)?.take(MAX_DICTIONARY_BYTES + 1),
            &mut source,
        )?;
        ensure!(
            copied <= MAX_DICTIONARY_BYTES && stamp(path, MAX_DICTIONARY_BYTES)? == before,
            "The Divvun dictionary changed while opening. Select it again."
        );
        source.flush()?;
        if extension == "zhfst" {
            let mut archive = zip::ZipArchive::new(File::open(source.path())?)
                .context("The .zhfst dictionary is not a valid ZIP archive")?;
            ensure!(
                archive.len() <= 128,
                "The .zhfst dictionary contains too many entries."
            );
            let mut expanded = 0_u64;
            for index in 0..archive.len() {
                expanded = expanded
                    .checked_add(archive.by_index(index)?.size())
                    .context("The expanded dictionary is too large")?;
                ensure!(
                    expanded <= MAX_DICTIONARY_BYTES,
                    "The expanded .zhfst dictionary exceeds 512 MiB."
                );
            }
        }
        let archive = divvun_fst::archive::open(source.path()).map_err(|error| {
            anyhow::anyhow!("The Divvun dictionary could not be opened: {error}")
        })?;
        let mut config = archive.bundled_config().cloned().unwrap_or_default();
        config.search_budget = Some(
            config
                .search_budget
                .unwrap_or(DIVVUN_SEARCH_BUDGET)
                .min(DIVVUN_SEARCH_BUDGET),
        );
        config.n_best = Some(MAX_SUGGESTIONS + 1);
        Ok(Self {
            speller: archive.speller(),
            config,
            _archive: archive,
            _source: source,
        })
    }
    fn word(&self, word: &str) -> Result<Option<Vec<String>>> {
        if Arc::clone(&self.speller).is_correct_with_config(word, &self.config) {
            return Ok(None);
        }
        let mut suggestions = Vec::new();
        for suggestion in Arc::clone(&self.speller).suggest_with_config(word, &self.config) {
            let value = suggestion.value().to_owned();
            if value != word && !suggestions.contains(&value) {
                suggestions.push(value);
            }
            if suggestions.len() == MAX_SUGGESTIONS {
                break;
            }
        }
        Ok(Some(suggestions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_options_inherit_safe_offline_defaults() {
        let options: ProofingOptions = serde_json::from_str("{}").expect("older settings");
        assert_eq!(options, ProofingOptions::default());
        assert!(options.validate().is_ok());
    }
    #[test]
    fn language_tags_are_validated_without_touching_dictionary_paths() {
        let mut options = ProofingOptions {
            spelling: SpellingBackend::Hunspell,
            dictionary_path: Some(PathBuf::from("not-installed.dic")),
            ..ProofingOptions::default()
        };
        assert!(options.validate().is_ok());
        for language in ["", "../en", "en--US", "e", "en\0US"] {
            options.language = language.into();
            assert!(options.validate().is_err(), "language {language:?}");
        }
    }
    #[test]
    fn harper_character_offsets_map_to_exact_utf16_ranges() {
        let text = "🦀 e\u{301} teh";
        let index = TextIndex::new(text);
        assert_eq!(index.character_range(5..8).expect("word range"), 6..9);
        assert_eq!(
            index
                .byte_range(text.find("teh").expect("fixture word")..text.len())
                .expect("word byte range"),
            6..9
        );
        assert!(index.byte_range(1..2).is_err());
    }
    #[test]
    fn insert_after_and_remove_suggestions_replace_only_the_reported_original() {
        let index = TextIndex::new("🦀 word");
        let lint = Lint {
            span: harper_core::Span::new(2, 6),
            lint_kind: LintKind::Punctuation,
            suggestions: vec![Suggestion::InsertAfter(vec!['.']), Suggestion::Remove],
            message: "Add punctuation".into(),
            priority: 1,
        };
        let issue = lint_issue(lint, &index).expect("converted grammar finding");
        assert_eq!(issue.range, 3..7);
        assert_eq!(issue.original, "word");
        assert_eq!(issue.suggestions, ["word.", ""]);
    }
    #[test]
    fn real_hunspell_affix_rules_and_unicode_offsets_are_used() {
        let dictionary =
            spellbook::Dictionary::new("SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n", "2\ncat/S\ncafé\n")
                .expect("small real Hunspell dictionary");
        assert!(dictionary.check("cats"));
        let text = "🦀 café catz catz";
        let index = TextIndex::new(text);
        let mut calls = 0;
        let issues = spelling_issues(text, &index, |word| {
            calls += 1;
            Ok(if dictionary.check(word) {
                None
            } else {
                Some(vec!["cats".into()])
            })
        })
        .expect("dictionary findings");
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].range, 8..12);
        assert_eq!(issues[0].original, "catz");
        assert_eq!(calls, 2, "repeated words reuse their result");
    }
    #[test]
    fn real_divvun_archive_is_loaded_and_suggests_through_the_adapter() {
        use box_format::{
            BoxPath, Compression, CompressionConfig, HashMap as BoxHashMap, sync::BoxWriter,
        };
        let directory = tempfile::tempdir().expect("fixture directory");
        let path = directory.path().join("fixture.bhfst");
        let fixtures = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/proofing/fixtures/divvun"
        ));
        let mut writer = BoxWriter::create_with_alignment(&path, 8).expect("real Box archive");
        for (source, target) in [
            ("lexicon.thfst", "acceptor.default.thfst"),
            ("mutator.thfst", "errmodel.default.thfst"),
        ] {
            writer
                .mkdir(
                    BoxPath::new(target).expect("archive directory"),
                    BoxHashMap::new(),
                )
                .expect("create archive directory");
            for component in ["alphabet", "index", "transition"] {
                let file = File::open(fixtures.join(source).join(component))
                    .expect("unmodified upstream transducer fixture");
                writer
                    .insert(
                        &CompressionConfig::new(Compression::Stored),
                        BoxPath::new(Path::new(target).join(component)).expect("archive path"),
                        std::io::BufReader::new(file),
                        BoxHashMap::new(),
                    )
                    .expect("archive transducer component");
            }
        }
        let metadata = serde_json::json!({
            "info": {
                "locale": "se",
                "title": [{"lang": null, "$value": "Fixture speller"}],
                "description": "Fixture speller",
                "producer": "divvunspell tests"
            },
            "acceptor": {
                "type": "general", "id": "acceptor.default.thfst",
                "title": [{"lang": null, "$value": "Fixture lexicon"}],
                "description": "Fixture lexicon"
            },
            "errmodel": {
                "id": "errmodel.default.thfst",
                "title": [{"lang": null, "$value": "Fixture error model"}],
                "description": "Fixture error model"
            }
        });
        writer
            .insert(
                &CompressionConfig::new(Compression::Stored),
                BoxPath::new("meta.json").expect("metadata path"),
                std::io::Cursor::new(serde_json::to_vec(&metadata).expect("metadata JSON")),
                BoxHashMap::new(),
            )
            .expect("archive metadata");
        writer.finish().expect("finish real dictionary archive");
        let adapter = Divvun::open(&path).expect("open dictionary using the production adapter");
        assert!(
            adapter
                .word("cat")
                .expect("accepted fixture word")
                .is_none()
        );
        let suggestions = adapter
            .word("kat")
            .expect("fixture misspelling")
            .expect("kat is not a fixture word");
        assert!(
            suggestions.iter().any(|word| word == "cat"),
            "{suggestions:?}"
        );
    }

    #[test]
    fn dictionary_encoding_never_inserts_replacement_characters() {
        let encoding = hunspell_encoding(b"SET ISO8859-1\n").expect("legacy encoding");
        assert_eq!(
            decode_dictionary(b"caf\xe9", encoding).expect("legacy word"),
            "café"
        );
        assert!(decode_dictionary(&[0xff], encoding_rs::UTF_8).is_err());
    }
    #[test]
    fn missing_dictionaries_and_oversized_input_are_actionable_errors() {
        let options = ProofingOptions {
            grammar: false,
            spelling: SpellingBackend::Divvunspell,
            dictionary_path: None,
            ..ProofingOptions::default()
        };
        assert!(check("A word", &options).is_err());
        assert!(check(&"x".repeat(MAX_TEXT_BYTES + 1), &ProofingOptions::default()).is_err());
    }
    #[test]
    fn harper_finds_real_repeated_word_grammar_offline() {
        let text = "🦀 This is is a sentence.";
        let issues =
            grammar_issues(text, &TextIndex::new(text), "en-US").expect("offline Harper grammar");
        assert!(
            !issues.is_empty(),
            "a repeated word must produce a grammar finding"
        );
        let units: Vec<u16> = text.encode_utf16().collect();
        for issue in issues {
            assert_eq!(
                String::from_utf16(&units[issue.range]).expect("valid UTF-16 finding"),
                issue.original
            );
        }
    }
}
