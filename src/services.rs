//! Windows spelling and speech, plus local typographic editing helpers.

use std::{collections::BTreeMap, marker::PhantomData, ops::Range, rc::Rc};

use anyhow::{Context as _, Result, bail, ensure};
use unicode_segmentation::UnicodeSegmentation as _;
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Globalization::{
            CORRECTIVE_ACTION_DELETE, CORRECTIVE_ACTION_GET_SUGGESTIONS, CORRECTIVE_ACTION_NONE,
            CORRECTIVE_ACTION_REPLACE, ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory,
        },
        Media::Speech::{ISpVoice, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SpVoice},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, IEnumString,
        },
    },
    core::{PCWSTR, PWSTR},
};

/// A Windows spelling finding with offsets in RichEdit's UTF-16 coordinate system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellingIssue {
    /// The affected UTF-16 code units, excluding any terminating NUL.
    pub range: Range<usize>,
    /// The original document text in that range.
    pub word: String,
    /// Suggested replacements in Windows' preferred order; empty text means delete.
    pub suggestions: Vec<String>,
}

/// Checks spelling with the Windows dictionary for the selected language.
///
/// # Errors
/// Returns an error for missing dictionaries, invalid input, or COM/provider failures.
pub fn check_spelling_for_language(text: &str, language: &str) -> Result<Vec<SpellingIssue>> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(
        !text.contains('\0'),
        "spelling input contains a NUL character"
    );
    let units: Vec<u16> = text.encode_utf16().collect();
    ensure!(
        units.len() <= u32::MAX as usize,
        "document is too long for Windows spellchecking"
    );
    let _apartment = ComApartment::initialize()?;
    ensure!(
        !language.is_empty()
            && language.len() < 85
            && language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "Choose a spelling language such as en-US or fi-FI"
    );
    let locale: Vec<u16> = language.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: COM is initialized on this thread and the factory has no outer object.
    let factory: ISpellCheckerFactory =
        unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER) }
            .context("Windows spellchecking is unavailable")?;
    // SAFETY: locale contains the NUL-terminated language name and remains alive.
    let supported = unsafe { factory.IsSupported(PCWSTR(locale.as_ptr())) }
        .context("checking the installed spelling dictionary")?;
    ensure!(
        supported.as_bool(),
        "no Windows spelling dictionary is installed for {language}"
    );
    // SAFETY: the validated language string remains valid throughout the COM call.
    let checker = unsafe { factory.CreateSpellChecker(PCWSTR(locale.as_ptr())) }
        .context("opening the Windows spelling dictionary")?;
    let mut terminated = units.clone();
    terminated.push(0);
    // SAFETY: terminated contains the complete NUL-terminated input for this call.
    let errors = unsafe { checker.ComprehensiveCheck(PCWSTR(terminated.as_ptr())) }
        .context("checking the document's spelling")?;
    let mut issues = Vec::new();
    loop {
        let mut error = None;
        // SAFETY: error is a valid writable COM out parameter and owns any result.
        unsafe { errors.Next(&mut error) }
            .ok()
            .context("reading the next spelling finding")?;
        let Some(error) = error else { break };
        // SAFETY: error is a live interface returned by the spelling enumerator.
        let action =
            unsafe { error.CorrectiveAction() }.context("reading a spelling correction")?;
        if action == CORRECTIVE_ACTION_NONE {
            continue;
        }
        // SAFETY: both properties are read from the same live COM finding.
        let start = unsafe { error.StartIndex() }.context("reading spelling offset")? as usize;
        // SAFETY: error remains alive and Length writes only its return value.
        let length = unsafe { error.Length() }.context("reading spelling length")? as usize;
        let end = start
            .checked_add(length)
            .context("spelling range overflow")?;
        let word_units = units
            .get(start..end)
            .context("Windows returned an invalid spelling range")?;
        let word =
            String::from_utf16(word_units).context("spelling range splits a Unicode character")?;
        let suggestions = if action == CORRECTIVE_ACTION_GET_SUGGESTIONS {
            spelling_suggestions(&checker, &word)?
        } else if action == CORRECTIVE_ACTION_REPLACE {
            // SAFETY: Replacement returns a CoTaskMem-allocated NUL-terminated string.
            let replacement = TaskString(
                unsafe { error.Replacement() }
                    .context("reading the suggested spelling replacement")?,
            );
            vec![replacement.into_string()?]
        } else if action == CORRECTIVE_ACTION_DELETE {
            vec![String::new()]
        } else {
            bail!(
                "Windows returned an unknown spelling correction: {}",
                action.0
            );
        };
        issues.push(SpellingIssue {
            range: start..end,
            word,
            suggestions,
        });
    }
    Ok(issues)
}

fn spelling_suggestions(checker: &ISpellChecker, word: &str) -> Result<Vec<String>> {
    let terminated: Vec<u16> = word.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: word is validated text from the document and remains NUL-terminated.
    let suggestions = unsafe { checker.Suggest(PCWSTR(terminated.as_ptr())) }
        .context("requesting spelling suggestions")?;
    read_com_strings(&suggestions)
}

fn read_com_strings(enumerator: &IEnumString) -> Result<Vec<String>> {
    let mut strings = Vec::new();
    loop {
        let mut slot = [PWSTR::null()];
        let mut fetched = 0;
        // SAFETY: slot holds one writable pointer; fetched is a live out parameter.
        let status = unsafe { enumerator.Next(&mut slot, Some(&mut fetched)) };
        let owned = TaskString(slot[0]);
        status.ok().context("reading a spelling suggestion")?;
        if fetched == 0 {
            break;
        }
        ensure!(
            fetched == 1,
            "Windows returned an invalid spelling suggestion count"
        );
        let value = owned.into_string()?;
        if !strings.contains(&value) {
            strings.push(value);
        }
    }
    Ok(strings)
}

#[derive(Debug)]
struct TaskString(PWSTR);

impl TaskString {
    fn into_string(self) -> Result<String> {
        ensure!(
            !self.0.is_null(),
            "Windows returned a missing spelling suggestion"
        );
        // SAFETY: this private wrapper owns a documented COM-allocated UTF-16 string.
        unsafe { self.0.to_string() }.context("Windows returned invalid Unicode text")
    }
}

impl Drop for TaskString {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer was allocated by COM and is released exactly once.
            unsafe { CoTaskMemFree(Some(self.0.as_ptr().cast())) };
        }
    }
}

#[derive(Debug)]
struct ComApartment {
    initialized: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl ComApartment {
    fn initialize() -> Result<Self> {
        // SAFETY: the reserved argument is null and this guard balances successful calls.
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let initialized = if status == RPC_E_CHANGED_MODE {
            false
        } else {
            status
                .ok()
                .context("initializing Windows language services")?;
            true
        };
        Ok(Self {
            initialized,
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: the guard cannot move between threads and owns one successful init.
            unsafe { CoUninitialize() };
        }
    }
}

/// A cancellable native Windows voice that keeps COM alive while speech is queued.
#[derive(Debug, Default)]
pub struct Speech {
    voice: Option<ISpVoice>,
    apartment: Option<ComApartment>,
}

impl Speech {
    /// Starts asynchronous speech, replacing anything previously queued by this voice.
    ///
    /// # Errors
    /// Returns errors from Windows speech initialization or queuing the utterance.
    pub fn start(&mut self, text: &str) -> Result<()> {
        ensure!(
            !text.contains('\0'),
            "speech input contains a NUL character"
        );
        if text.is_empty() {
            return self.stop();
        }
        if self.voice.is_none() {
            if self.apartment.is_none() {
                self.apartment = Some(ComApartment::initialize()?);
            }
            // SAFETY: the apartment field keeps COM initialized until after voice drops.
            self.voice = Some(
                unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_INPROC_SERVER) }
                    .context("Windows speech synthesis is unavailable")?,
            );
        }
        let terminated: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
        if let Some(voice) = &self.voice {
            // SAFETY: SAPI copies the terminated input when queuing asynchronous speech.
            unsafe { voice.Speak(PCWSTR(terminated.as_ptr()), flags, None) }
                .context("starting speech synthesis")?;
        }
        Ok(())
    }

    /// Stops any utterance queued by this voice without affecting other applications.
    ///
    /// # Errors
    /// Returns an error if Windows cannot cancel the current utterance.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(voice) = &self.voice {
            // SAFETY: a null string with PURGEBEFORESPEAK cancels this live SAPI voice.
            unsafe { voice.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None) }
                .context("stopping speech synthesis")?;
        }
        Ok(())
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// A local replacement with offsets in RichEdit's UTF-16 coordinate system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSubstitution {
    /// The exact code units to replace, leaving surrounding formatting intact.
    pub range: Range<usize>,
    /// The typographic quote or dash replacing the user's most recent insertion.
    pub replacement: String,
}

/// Suggests one smart quote or dash immediately before the supplied UTF-16 caret.
/// Returns no change for disabled substitutions or an invalid Unicode boundary.
pub fn substitution(
    text: &str,
    caret_utf16: usize,
    smart_quotes: bool,
    smart_dashes: bool,
) -> Option<TextSubstitution> {
    let caret_byte = utf16_byte_offset(text, caret_utf16)?;
    let preceding = &text[..caret_byte];
    if smart_dashes && preceding.ends_with("--") {
        return Some(TextSubstitution {
            range: caret_utf16 - 2..caret_utf16,
            replacement: "—".to_owned(),
        });
    }
    if !smart_quotes {
        return None;
    }
    let mut chars = preceding.chars().rev();
    let typed = chars.next()?;
    if typed != '\'' && typed != '"' {
        return None;
    }
    let previous = chars.next();
    let opening = previous.is_none_or(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '(' | '[' | '{' | '<' | '—' | '–' | ':' | ';' | '=' | '“' | '‘'
            )
    });
    let replacement = match (typed, opening) {
        ('\'', true) => "‘",
        ('\'', false) => "’",
        ('"', true) => "“",
        ('"', false) => "”",
        _ => return None,
    };
    Some(TextSubstitution {
        range: caret_utf16 - 1..caret_utf16,
        replacement: replacement.to_owned(),
    })
}

pub(crate) fn utf16_byte_offset(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

/// Returns document words extending a prefix, deduplicated and ordered ignoring case.
pub fn completions(text: &str, prefix: &str) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let prefix = prefix.to_lowercase();
    let mut candidates = BTreeMap::new();
    for word in text.unicode_words() {
        let key = word.to_lowercase();
        if key.starts_with(&prefix) && key != prefix {
            candidates.entry(key).or_insert_with(|| word.to_owned());
        }
    }
    candidates.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::{TextSubstitution, completions, substitution, utf16_byte_offset};

    #[test]
    fn smart_dashes_keep_utf16_offsets_after_emoji() {
        assert_eq!(
            substitution("😀--", 4, false, true),
            Some(TextSubstitution {
                range: 2..4,
                replacement: "—".to_owned()
            })
        );
    }

    #[test]
    fn smart_quotes_distinguish_opening_closing_and_apostrophes() {
        for (text, caret, expected) in [
            ("\"hello", 1, "“"),
            ("hello\"", 6, "”"),
            ("can\'t", 4, "’"),
            ("(\'hello", 2, "‘"),
            ("He said: \"here", 10, "“"),
        ] {
            assert_eq!(
                substitution(text, caret, true, false).map(|item| item.replacement),
                Some(expected.to_owned()),
                "input {text:?}"
            );
        }
    }

    #[test]
    fn substitutions_ignore_disabled_features_and_replacement_characters() {
        assert_eq!(substitution("--", 2, true, false), None);
        assert_eq!(substitution("\"", 1, false, true), None);
        assert_eq!(substitution("“", 1, true, true), None);
        assert_eq!(substitution("—", 1, true, true), None);
    }

    #[test]
    fn invalid_surrogate_offsets_are_rejected() {
        assert_eq!(utf16_byte_offset("😀\"", 1), None);
        assert_eq!(utf16_byte_offset("😀\"", 2), Some(4));
        assert_eq!(substitution("😀\"", 1, true, true), None);
        assert_eq!(substitution("😀\"", 4, true, true), None);
        assert_eq!(
            substitution("😀\"", 3, true, true).map(|item| item.range),
            Some(2..3)
        );
    }

    #[test]
    fn completions_are_case_insensitive_and_deterministic() {
        assert_eq!(
            completions("Alpha alphabet ALPHA alpine beta", "AL"),
            vec!["Alpha", "alphabet", "alpine"]
        );
        assert_eq!(completions("alpha alpha", "alpha"), Vec::<String>::new());
        assert_eq!(completions("alpha", ""), Vec::<String>::new());
    }

    #[test]
    fn completions_preserve_combining_marks_and_non_ascii_words() {
        assert_eq!(completions("e\u{301}clair École ÉCOLE", "é"), vec!["École"]);
        assert_eq!(completions("e\u{301}clair", "e"), vec!["e\u{301}clair"]);
    }
}
