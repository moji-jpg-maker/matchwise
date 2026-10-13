//! Turning one pasted corpus of raw text into the entries a bulk intake run works on.
//!
//! This is the only parser on the intake path: the app splits a paste or a loaded file into entries here, then
//! creates one profile and stores one intake note per entry. Entries are kept as the matchmaker wrote them — the
//! helper trims edges and normalizes line endings, nothing else. Raw text is never a profile field, so it can
//! never be scored, exported as field data, or reach a model through any path but the redacted intake note.

use serde::Serialize;

/// The longest text kept from a single entry. Anything beyond is dropped and the entry is flagged `truncated`,
/// so an accidentally pasted book cannot silently become a 40 MB profile.
pub const MAX_ENTRY_CHARS: usize = 20_000;

/// A separator line: nothing but dashes (`---`, `-----`) once trimmed.
fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.chars().all(|c| c == '-')
}

/// Why an entry was left out, in words the report can show the matchmaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// The entry had no text at all once trimmed.
    Empty,
}

/// One entry of a corpus, ready to become a profile plus an intake note.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntakeEntry {
    /// 1-based position in the corpus, before any drop.
    pub index: usize,
    /// A label the matchmaker or file gave the entry, or `Entry N` when there is none.
    pub label: String,
    /// The raw text, trimmed and capped at [`MAX_ENTRY_CHARS`].
    pub text: String,
    /// True when the text was longer than the cap and the tail was dropped.
    pub truncated: bool,
}

/// An entry that could not be used, and why. Kept apart from [`IntakeEntry`] so a caller can report both.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DroppedEntry {
    pub index: usize,
    pub label: String,
    pub reason: DropReason,
}

/// The outcome of parsing a corpus: what can be used, and what could not.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedCorpus {
    pub entries: Vec<IntakeEntry>,
    pub dropped: Vec<DroppedEntry>,
}

/// Turn a corpus of raw text into entries.
///
/// Entries are separated by a line of dashes (`---`), and by blank lines when the corpus contains no separator
/// line at all — a matchmaker pasting blocks still gets blocks. Line endings are normalized to `\n` and every
/// entry is trimmed; nothing else is changed, because the point of storing the entry is to keep the source text
/// as it was written. A leading heading line (`# Sara`) names the entry; otherwise it is `Entry N`. Empty
/// entries are dropped and reported rather than becoming an empty profile.
pub fn split_corpus(corpus: &str) -> ParsedCorpus {
    let normalized = corpus.replace("\r\n", "\n").replace('\r', "\n");
    let chunks = if normalized.lines().any(is_separator) {
        split_on_separators(&normalized)
    } else {
        split_on_blank_lines(&normalized)
    };

    let mut entries = Vec::new();
    let mut dropped = Vec::new();
    for (i, chunk) in chunks.into_iter().enumerate() {
        let index = i + 1;
        let (label, text) = split_label(&chunk);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            dropped.push(DroppedEntry {
                index,
                label: label.unwrap_or_else(|| format!("Entry {index}")),
                reason: DropReason::Empty,
            });
            continue;
        }
        let (text, truncated) = cap(trimmed, MAX_ENTRY_CHARS);
        entries.push(IntakeEntry {
            index,
            label: label.unwrap_or_else(|| format!("Entry {index}")),
            text,
            truncated,
        });
    }
    ParsedCorpus { entries, dropped }
}

/// Parse a `.json` file: an array of strings, one entry each, in order. A malformed document is a plain error the
/// caller can record against the run, never a silent empty corpus.
pub fn parse_json_corpus(raw: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("The file is not valid JSON: {e}"))?;
    let array = value
        .as_array()
        .ok_or_else(|| "The JSON file must be an array of strings, one per person.".to_string())?;
    let mut out = Vec::with_capacity(array.len());
    for (i, item) in array.iter().enumerate() {
        let text = item
            .as_str()
            .ok_or_else(|| format!("Item {} of the JSON array is not a string.", i + 1))?;
        out.push(text.to_string());
    }
    Ok(out)
}

/// Turn entries a caller has already separated — a `.json` array of strings, or one string per entry — into the
/// same [`ParsedCorpus`] shape `split_corpus` produces. Each string is one entry as written, so it is never split
/// further; it is only trimmed and capped, and an empty one is dropped rather than becoming an empty profile.
pub fn parse_entries(entries: &[String]) -> ParsedCorpus {
    let mut parsed = ParsedCorpus {
        entries: Vec::new(),
        dropped: Vec::new(),
    };
    for (i, raw) in entries.iter().enumerate() {
        let index = i + 1;
        let text = raw.replace("\r\n", "\n").replace('\r', "\n");
        let (label, body) = split_label(&text);
        let trimmed = body.trim();
        if trimmed.is_empty() {
            parsed.dropped.push(DroppedEntry {
                index,
                label: label.unwrap_or_else(|| format!("Entry {index}")),
                reason: DropReason::Empty,
            });
            continue;
        }
        let (text, truncated) = cap(trimmed, MAX_ENTRY_CHARS);
        parsed.entries.push(IntakeEntry {
            index,
            label: label.unwrap_or_else(|| format!("Entry {index}")),
            text,
            truncated,
        });
    }
    parsed
}

/// Split on separator lines: a line of three or more dashes separates entries and is itself removed.
fn split_on_separators(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if is_separator(line) {
            chunks.push(current.join("\n"));
            current.clear();
        } else {
            current.push(line);
        }
    }
    chunks.push(current.join("\n"));
    chunks
}

/// Split on runs of blank lines, the fallback when the corpus has no separator line.
fn split_on_blank_lines(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                chunks.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        chunks.push(current.join("\n"));
    }
    chunks
}

/// Read an optional label from the first line of a chunk: a leading `#` heading names the entry. Anything else
/// stays part of the text, so no line of somebody's own words is ever mistaken for a label.
fn split_label(chunk: &str) -> (Option<String>, String) {
    let mut lines = chunk.lines();
    let Some(first) = lines.next() else {
        return (None, String::new());
    };
    let first = first.trim();
    if first.is_empty() {
        return (None, lines.collect::<Vec<_>>().join("\n"));
    }
    if let Some(rest) = first.strip_prefix('#') {
        let label = rest
            .trim_matches(|c: char| c == '#' || c.is_whitespace())
            .trim();
        if !label.is_empty() && label.chars().count() <= 60 {
            let body = lines.collect::<Vec<_>>().join("\n");
            if !body.trim().is_empty() {
                return (Some(label.to_string()), body);
            }
        }
    }
    (None, chunk.to_string())
}

fn cap(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        (s.to_string(), false)
    } else {
        (s.chars().take(max).collect(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Profile, Provenance, Value};
    use crate::{default_registry, profile_text_for_ai};

    #[test]
    fn dashes_split_entries_and_the_separator_is_removed() {
        let parsed = split_corpus(
            "Sara lives in Shiraz.\n---\nReza lives in Tehran.\n---\nMina lives in Yazd.",
        );
        let texts: Vec<&str> = parsed.entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Sara lives in Shiraz.",
                "Reza lives in Tehran.",
                "Mina lives in Yazd."
            ]
        );
        assert!(parsed.dropped.is_empty());
        assert!(!texts.iter().any(|t| t.contains("---")));
    }

    #[test]
    fn blank_lines_split_when_there_is_no_separator() {
        let parsed =
            split_corpus("Sara lives in Shiraz.\n\n\nReza lives in Tehran.\n\nMina lives in Yazd.");
        assert_eq!(parsed.entries.len(), 3);
        assert_eq!(parsed.entries[1].text, "Reza lives in Tehran.");
    }

    #[test]
    fn a_separator_line_wins_and_internal_blank_lines_stay_inside_an_entry() {
        let parsed =
            split_corpus("Sara lives in Shiraz.\nShe likes walks.\n\n---\nReza lives in Tehran.");
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(
            parsed.entries[0].text,
            "Sara lives in Shiraz.\nShe likes walks."
        );
    }

    #[test]
    fn crlf_and_bare_carriage_returns_are_normalized() {
        let parsed =
            split_corpus("Sara lives in Shiraz.\r\n---\rReza lives in Tehran.\r\n---\r\nMina.");
        assert_eq!(parsed.entries.len(), 3);
        assert!(parsed.entries.iter().all(|e| !e.text.contains('\r')));
        assert_eq!(parsed.entries[0].text, "Sara lives in Shiraz.");
        assert_eq!(parsed.entries[1].text, "Reza lives in Tehran.");
    }

    #[test]
    fn blank_entries_are_dropped_and_reported_not_created() {
        let parsed =
            split_corpus("Sara lives in Shiraz.\n---\n   \n---\n\n---\nReza lives in Tehran.");
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].index, 1);
        assert_eq!(parsed.entries[1].index, 4);
        assert_eq!(parsed.dropped.len(), 2);
        assert!(parsed.dropped.iter().all(|d| d.reason == DropReason::Empty));
        assert_eq!(parsed.dropped[0].index, 2);
        assert_eq!(parsed.dropped[0].label, "Entry 2");
    }

    #[test]
    fn an_empty_corpus_is_no_entries_and_no_drops() {
        let parsed = split_corpus("");
        assert!(parsed.entries.is_empty());
        assert!(parsed.dropped.is_empty());
    }

    #[test]
    fn the_cap_is_exact_and_only_flags_what_it_cuts() {
        let at_limit = "a".repeat(MAX_ENTRY_CHARS);
        let parsed = split_corpus(&at_limit);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].text.chars().count(), MAX_ENTRY_CHARS);
        assert!(
            !parsed.entries[0].truncated,
            "exactly at the cap is not truncated"
        );

        let over = format!("{}b", "a".repeat(MAX_ENTRY_CHARS));
        let parsed = split_corpus(&over);
        assert_eq!(parsed.entries[0].text.chars().count(), MAX_ENTRY_CHARS);
        assert!(parsed.entries[0].truncated);
        assert!(
            !parsed.entries[0].text.ends_with('b'),
            "the tail past the cap is dropped"
        );
    }

    #[test]
    fn the_cap_counts_characters_not_bytes() {
        let parsed = split_corpus(&"é".repeat(MAX_ENTRY_CHARS + 10));
        assert_eq!(parsed.entries[0].text.chars().count(), MAX_ENTRY_CHARS);
        assert!(parsed.entries[0].truncated);
    }

    #[test]
    fn labels_fall_back_to_entry_number_and_headings_are_read() {
        let parsed = split_corpus("Sara lives in Shiraz.\n---\n# Reza\nReza lives in Tehran.\n---\n## Mina\nMina lives in Yazd.");
        assert_eq!(parsed.entries[0].label, "Entry 1");
        assert_eq!(parsed.entries[1].label, "Reza");
        assert_eq!(
            parsed.entries[1].text, "Reza lives in Tehran.",
            "the heading line is not part of the text"
        );
        assert_eq!(parsed.entries[2].label, "Mina");
    }

    #[test]
    fn a_line_of_somebodys_words_is_never_taken_as_a_label() {
        let parsed = split_corpus("Sara: lives in Shiraz and likes walks.");
        assert_eq!(parsed.entries[0].label, "Entry 1");
        assert_eq!(
            parsed.entries[0].text,
            "Sara: lives in Shiraz and likes walks."
        );
    }

    #[test]
    fn json_array_of_strings_is_read_in_order() {
        let items =
            parse_json_corpus(r#"["Sara lives in Shiraz.", "Reza lives in Tehran."]"#).unwrap();
        assert_eq!(
            items,
            vec!["Sara lives in Shiraz.", "Reza lives in Tehran."]
        );
    }

    #[test]
    fn entries_that_arrived_already_separated_are_not_split_again() {
        // A JSON entry is one person, even when it contains a blank line or a dash line of its own.
        let json = r#"["Sara lives in Shiraz.\n\n---\nReza lives in Tehran."]"#;
        let items = parse_json_corpus(json).unwrap();
        let parsed = parse_entries(&items);
        assert_eq!(
            parsed.entries.len(),
            1,
            "an entry's own blank line or dashes do not split it"
        );
        assert!(parsed.entries[0].text.contains("Reza"));

        // CRLF in a JSON string is normalized too.
        let parsed = parse_entries(&["Sara.\r\nLikes walks.".to_string()]);
        assert_eq!(parsed.entries[0].text, "Sara.\nLikes walks.");

        // An empty or whitespace-only string is dropped, not made into a profile.
        let parsed = parse_entries(&["ok".to_string(), "   ".to_string(), "two".to_string()]);
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[1].index, 3);
        assert_eq!(parsed.dropped.len(), 1);
        assert_eq!(parsed.dropped[0].reason, DropReason::Empty);

        // The cap applies the same way.
        let parsed = parse_entries(&["x".repeat(MAX_ENTRY_CHARS + 5)]);
        assert_eq!(parsed.entries[0].text.chars().count(), MAX_ENTRY_CHARS);
        assert!(parsed.entries[0].truncated);
    }

    #[test]
    fn malformed_json_is_an_error_not_an_empty_corpus() {
        assert!(parse_json_corpus("[{\"oops\": true}").is_err());
        assert!(parse_json_corpus("{\"not\": \"an array\"}").is_err());
        assert!(parse_json_corpus("[1, 2]").is_err());
    }

    /// The raw corpus reaches a model only through the redacted `extra` path of `profile_text_for_ai`; it is never
    /// a profile field, so it cannot be exported, scored, or sent through the field text.
    #[test]
    fn raw_intake_text_is_not_a_profile_field() {
        let corpus =
            "Sara Karimi lives in Shiraz, phone +98 912 345 6789, and loves mountain walks.";
        let parsed = split_corpus(corpus);
        let entry = &parsed.entries[0];
        let reg = default_registry();
        let mut profile = Profile::new("p1");
        assert!(profile.set(
            "full_name",
            Value::Text("Sara Karimi".into()),
            Provenance::User
        ));
        assert!(profile.set("city", Value::Text("Shiraz".into()), Provenance::Matchmaker));

        // The corpus only ever became fields the caller chose to set; the raw sentence is nowhere in them.
        let keys: Vec<String> = profile.fields.keys().cloned().collect();
        assert_eq!(keys, vec!["city".to_string(), "full_name".to_string()]);
        assert!(profile
            .fields
            .values()
            .all(|e| !format!("{:?}", e.value).contains("mountain")));

        // Without the note, none of the raw words are in the profile text.
        let without = profile_text_for_ai(&profile, &reg, None, false);
        assert!(!without.contains("mountain"));
        assert!(!without.contains("+98"));

        // With the note, it appears scrubbed, under the intake heading only.
        let with_note = profile_text_for_ai(&profile, &reg, Some(&entry.text), false);
        assert!(with_note.contains("Intake notes:"));
        assert!(
            with_note.contains("mountain walks"),
            "ordinary words survive the scrubber"
        );
        assert!(
            !with_note.contains("Karimi"),
            "the person's name is scrubbed"
        );
        assert!(
            !with_note.contains("+98 912 345 6789"),
            "phone numbers are scrubbed"
        );
    }
}
