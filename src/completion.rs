//! Bounded, offline word and language-keyword completion.
use std::{collections::BTreeMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation as _;

const MAX_SCAN_BYTES: usize = 256 * 1024;
const MAX_CANDIDATES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Completion {
    /// Entire word being completed, in the app's UTF-16 coordinate system.
    pub range: Range<usize>,
    pub prefix: String,
    pub candidates: Vec<String>,
}

/// Automatic completion starts at three graphemes; Ctrl+Space also accepts shorter prefixes.
pub(crate) fn suggest(
    text: &str,
    caret_utf16: usize,
    language: Option<&str>,
    manual: bool,
) -> Option<Completion> {
    let caret = byte_at_utf16(text, caret_utf16)?;
    let (start, word) = text
        .unicode_word_indices()
        .find(|(start, word)| *start < caret && caret <= start + word.len())?;
    let prefix = &text[start..caret];
    if prefix.graphemes(true).count() < if manual { 1 } else { 3 }
        || prefix.len() > 256
        || !prefix.chars().any(char::is_alphabetic)
    {
        return None;
    }
    let folded = prefix.to_lowercase();
    let begin = text.floor_char_boundary(caret.saturating_sub(MAX_SCAN_BYTES / 2));
    let end = text.floor_char_boundary(text.len().min(begin.saturating_add(MAX_SCAN_BYTES)));
    let mut counts = BTreeMap::<String, usize>::new();
    let mut consider = |candidate: &str| {
        if candidate == prefix
            || candidate == word
            || candidate.len() > 256
            || !candidate.chars().any(char::is_alphabetic)
            || !candidate.to_lowercase().starts_with(&folded)
        {
            return;
        }
        *counts.entry(candidate.to_owned()).or_default() += 1;
    };
    for (offset, candidate) in text[begin..end].unicode_word_indices() {
        // A bounded scan must not offer a word cut in half at either edge.
        if (begin != 0 && offset == 0)
            || (end != text.len() && offset + candidate.len() == end - begin)
        {
            continue;
        }
        consider(candidate);
    }
    for keyword in keywords(language).split_ascii_whitespace() {
        consider(keyword);
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_unstable_by(|(a, count_a), (b, count_b)| {
        b.starts_with(prefix)
            .cmp(&a.starts_with(prefix))
            .then_with(|| count_b.cmp(count_a))
            .then_with(|| a.cmp(b))
    });
    let candidates = ranked
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(word, _)| word)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let from = text[..start].encode_utf16().count();
    Some(Completion {
        range: from..from + word.encode_utf16().count(),
        prefix: prefix.into(),
        candidates,
    })
}

fn byte_at_utf16(text: &str, position: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == position {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > position {
            return None;
        }
    }
    (units == position).then_some(text.len())
}

fn keywords(language: Option<&str>) -> &'static str {
    let name = language.unwrap_or_default().to_ascii_lowercase();
    if name.contains("rust") {
        "Self async await as break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self static struct super trait true type union unsafe use where while Option Result Some None Ok Err String Vec println print format"
    } else if name.contains("python") {
        "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield print range len str list dict set tuple super isinstance enumerate zip"
    } else if name.contains("javascript")
        || name.contains("typescript")
        || name.contains("tsx")
        || name.contains("jsx")
    {
        "abstract any async await as boolean break case catch class const constructor continue debugger declare default delete do else enum export extends false finally for from function get if implements import in infer instanceof interface keyof let module namespace never new null number object of private protected public readonly require return set static string super switch symbol this throw true try type typeof undefined unknown var void while with yield console document window Promise Array Object String Number Map Set JSON"
    } else if name.contains("sql") {
        "SELECT FROM WHERE INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE ALTER DROP JOIN INNER LEFT RIGHT OUTER ON AS AND OR NOT NULL IS IN LIKE BETWEEN GROUP BY HAVING ORDER ASC DESC LIMIT OFFSET DISTINCT UNION ALL EXISTS CASE WHEN THEN ELSE END PRIMARY KEY FOREIGN REFERENCES INDEX COUNT SUM AVG MIN MAX"
    } else if name.contains("powershell") {
        "begin break catch class continue data default do dynamicparam else elseif end enum exit filter finally for foreach from function if in param process return switch throw trap try until using var while workflow Write-Host Write-Output Get-Content Set-Content Get-ChildItem Get-Item Set-Item Get-Location Set-Location Test-Path Where-Object ForEach-Object Select-Object"
    } else if name.contains("html") || name.contains("xml") {
        "html head body title meta link style script div span section article header footer main nav aside button input textarea label form select option table thead tbody tr th td caption ul ol li p a img figure figcaption class id href src alt type name value placeholder disabled checked selected width height charset content"
    } else if name.contains("css") || name.contains("sass") || name.contains("scss") {
        "align-items animation background border bottom box-shadow color content cursor display flex flex-direction flex-wrap font font-family font-size font-style font-weight gap grid height justify-content left line-height margin max-height max-width min-height min-width opacity overflow padding position right text-align text-decoration top transform transition vertical-align visibility white-space width z-index"
    } else if name.contains("json") {
        "true false null"
    } else if name.contains("java")
        || name.contains("c#")
        || name.contains("csharp")
        || name == "c"
        || name.contains("c++")
    {
        "abstract alignas alignof assert auto bool boolean break byte case catch char class const constexpr continue default delete do double else enum explicit export extends extern false final finally float for friend if implements import inline int interface internal long namespace native new null nullptr operator override package private protected public readonly ref register return sealed short signed sizeof static string struct super switch synchronized template this throw throws true try typedef typename union unsigned using virtual void volatile while"
    } else if name.contains("shell") || name.contains("bash") || name.contains("zsh") {
        "alias break case continue declare do done echo elif else esac eval exec exit export false fi for function getopts if in local printf read readonly return select set shift source test then trap true type typeset ulimit umask unalias unset until wait while"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_threshold_and_manual_completion_differ() {
        assert!(suggest("printing pr", 11, None, false).is_none());
        let result = suggest("printing pr", 11, None, true).unwrap();
        assert_eq!(result.range, 9..11);
        assert_eq!(result.candidates, ["printing"]);
    }

    #[test]
    fn completion_replaces_the_entire_word_at_an_interior_caret() {
        let result = suggest("println print", 11, Some("Rust"), false).unwrap();
        assert_eq!(result.prefix, "pri");
        assert_eq!(result.range, 8..13);
        assert!(result.candidates.iter().any(|word| word == "println"));
        assert!(!result.candidates.iter().any(|word| word == "print"));
    }

    #[test]
    fn unicode_prefixes_and_positions_survive_emoji_and_combining_marks() {
        let text = "😀 éclair écla";
        let result = suggest(text, text.encode_utf16().count(), None, false).unwrap();
        assert_eq!(result.prefix, "écla");
        assert_eq!(result.range, 10..14);
        assert_eq!(result.candidates, ["éclair"]);
        assert!(suggest("😀", 1, None, true).is_none());
        let text = "cafe\u{301}ine cafe\u{301}";
        let result = suggest(text, text.encode_utf16().count(), None, false).unwrap();
        assert_eq!(result.candidates, ["cafe\u{301}ine"]);
    }

    #[test]
    fn candidates_are_deduplicated_ranked_and_limited() {
        let text = "printer printer Printer priority price pride primary printf printa printb printc printd printe printf printg printh pri";
        let result = suggest(text, text.encode_utf16().count(), None, false).unwrap();
        assert_eq!(result.candidates[0], "printer");
        assert_eq!(result.candidates.len(), MAX_CANDIDATES);
        assert_eq!(
            result
                .candidates
                .iter()
                .filter(|word| *word == "printer")
                .count(),
            1
        );
    }

    #[test]
    fn keywords_follow_the_document_language() {
        let rust = suggest("ret", 3, Some("Rust"), false).unwrap();
        assert!(rust.candidates.iter().any(|word| word == "return"));
        assert!(suggest("ret", 3, None, false).is_none());
        assert!(suggest("word. ", 6, None, true).is_none());
    }
}
