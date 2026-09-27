//! RESEARCH PROTOTYPE (worktree `research-diff-aware-enricher`, not for
//! merge as-is) -- see /home/coder/ds-review/diff-aware-enricher-research.md.
//!
//! Classifies the change between the text an incident's stored extraction
//! was computed from and its current text, so the enricher can
//! (a) skip the LLM entirely for a *semantic no-op* (HTML/whitespace/
//! entity/case/punctuation-only change) by carrying the previous extraction
//! forward, and (b) label churn metrics by edit class so the decision on a
//! diff-aware prompt can be made on data.
//!
//! ## Semantic no-op normalisation
//!
//! For each of summary and description independently (moving text between
//! the two is NOT a no-op):
//!
//! 1. Replace every HTML tag (`<...>`) with a space.
//! 2. Decode numeric character references (`&#233;`, `&#xE9;`); replace any
//!    other `&name;` entity with a space (so `&nbsp;`/`&amp;` act as
//!    separators -- `&` is punctuation anyway).
//! 3. Split on Unicode whitespace.
//! 4. Map each word to its *key*: lowercase, alphanumeric characters only.
//!    Drop words whose key is empty (pure punctuation).
//!
//! Two texts are a semantic no-op iff both key sequences are identical.
//! Deliberately conservative: punctuation *inside* a word is dropped
//! ("King's" == "Kings", "09:55" == "0955"), but word boundaries are kept,
//! so "platforms 1, 2" != "platforms 12" and "09 ; 55" != "09:55". Anything
//! that changes a digit, a letter, or a word boundary is NOT a no-op and
//! gets a full extraction.

/// One whitespace-delimited word of an incident's text after tag stripping
/// and entity decoding: `key` is what's compared, `display` is what a
/// diff-aware prompt would show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub key: String,
    pub display: String,
}

/// Strips tags, decodes/blanks entities, splits on whitespace -- steps 1-4
/// of the module doc.
pub fn words(text: &str) -> Vec<Word> {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                // Skip to the closing '>'; an unterminated '<' swallows the
                // rest, which is fine for both sides of a comparison.
                for c in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                }
                plain.push(' ');
            }
            '&' => {
                let mut entity = String::new();
                let mut terminated = false;
                while let Some(&next) = chars.peek() {
                    if next == ';' {
                        chars.next();
                        terminated = true;
                        break;
                    }
                    if !(next.is_ascii_alphanumeric() || next == '#') || entity.len() > 10 {
                        break;
                    }
                    entity.push(next);
                    chars.next();
                }
                if !terminated {
                    plain.push('&');
                    plain.push_str(&entity);
                    continue;
                }
                let decoded = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32);
                match decoded {
                    Some(ch) => plain.push(ch),
                    None => plain.push(' '),
                }
            }
            other => plain.push(other),
        }
    }
    plain
        .split_whitespace()
        .filter_map(|w| {
            let key: String = w
                .chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            (!key.is_empty()).then(|| Word {
                key,
                display: w.to_string(),
            })
        })
        .collect()
}

fn keys(text: &str) -> Vec<String> {
    words(text).into_iter().map(|w| w.key).collect()
}

/// How an incident's text moved between the version its stored extraction
/// was computed from and now. `label` values are a fixed, low-cardinality
/// set suitable for a metric label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditClass {
    /// Identical under the module doc's normalisation.
    SemanticNoop,
    /// Every changed word is a number/time on both sides, one-for-one
    /// (ETA bumps, "delayed by up to 20" -> "15"). Semantically meaningful.
    NumericOnly,
    /// Pure addition at the end of the description (summary unchanged).
    Append,
    /// Word-level similarity >= 0.8.
    SmallEdit,
    /// Word-level similarity >= 0.5.
    PartialRewrite,
    /// Word-level similarity < 0.5.
    Rewrite,
}

impl EditClass {
    pub fn label(self) -> &'static str {
        match self {
            EditClass::SemanticNoop => "semantic_noop",
            EditClass::NumericOnly => "numeric_only",
            EditClass::Append => "append",
            EditClass::SmallEdit => "small_edit",
            EditClass::PartialRewrite => "partial_rewrite",
            EditClass::Rewrite => "rewrite",
        }
    }
}

/// One non-equal hunk of a word diff: `old[old_range]` became
/// `new[new_range]` (either side may be empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_range: std::ops::Range<usize>,
    pub new_range: std::ops::Range<usize>,
}

/// Upper bound on the LCS table (old_len * new_len cells). Incident text is
/// p99 ~5k chars (~900 words); 4M cells is ~2k x 2k words, 16 MB of u32.
/// Past this, `diff` returns one whole-text hunk (i.e. "treat as rewrite").
const MAX_LCS_CELLS: usize = 4_000_000;

/// Word-level diff by longest common subsequence over `key`s. Returns the
/// non-equal hunks and the number of matched words.
pub fn diff(old: &[String], new: &[String]) -> (Vec<Hunk>, usize) {
    let (n, m) = (old.len(), new.len());
    if n.saturating_mul(m) > MAX_LCS_CELLS {
        return (
            vec![Hunk {
                old_range: 0..n,
                new_range: 0..m,
            }],
            0,
        );
    }
    // lcs[i][j] = LCS length of old[i..] and new[j..], row-major.
    let width = m + 1;
    let mut lcs = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * width + j] = if old[i] == new[j] {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j, mut matched) = (0, 0, 0);
    let mut hunks = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            if let Some((oi, nj)) = open.take() {
                hunks.push(Hunk {
                    old_range: oi..i,
                    new_range: nj..j,
                });
            }
            i += 1;
            j += 1;
            matched += 1;
            continue;
        }
        open.get_or_insert((i, j));
        if j < m && (i == n || lcs[i * width + j + 1] >= lcs[(i + 1) * width + j]) {
            j += 1;
        } else {
            i += 1;
        }
    }
    if let Some((oi, nj)) = open {
        hunks.push(Hunk {
            old_range: oi..n,
            new_range: nj..m,
        });
    }
    (hunks, matched)
}

fn is_numeric_key(key: &str) -> bool {
    key.chars().all(|c| c.is_ascii_digit())
}

/// Classifies the change from (`old_summary`, `old_description`) to
/// (`new_summary`, `new_description`) -- see [`EditClass`].
pub fn classify(
    old_summary: &str,
    old_description: &str,
    new_summary: &str,
    new_description: &str,
) -> EditClass {
    let (os, od, ns, nd) = (
        keys(old_summary),
        keys(old_description),
        keys(new_summary),
        keys(new_description),
    );
    if os == ns && od == nd {
        return EditClass::SemanticNoop;
    }
    if os == ns && nd.len() > od.len() && nd.starts_with(&od) {
        return EditClass::Append;
    }
    // Summary and description diffed as one sequence with a sentinel
    // between them, so a hunk can't silently straddle the boundary.
    let sentinel = "\u{0}".to_string();
    let old_all: Vec<String> = os.into_iter().chain([sentinel.clone()]).chain(od).collect();
    let new_all: Vec<String> = ns.into_iter().chain([sentinel]).chain(nd).collect();
    let (hunks, matched) = diff(&old_all, &new_all);
    let numeric_only = !hunks.is_empty()
        && hunks.iter().all(|h| {
            h.old_range.len() == h.new_range.len()
                && old_all[h.old_range.clone()]
                    .iter()
                    .chain(&new_all[h.new_range.clone()])
                    .all(|k| is_numeric_key(k))
        });
    if numeric_only {
        return EditClass::NumericOnly;
    }
    let total = old_all.len() + new_all.len();
    let similarity = if total == 0 {
        1.0
    } else {
        2.0 * matched as f64 / total as f64
    };
    if similarity >= 0.8 {
        EditClass::SmallEdit
    } else if similarity >= 0.5 {
        EditClass::PartialRewrite
    } else {
        EditClass::Rewrite
    }
}

/// Renders a compact, human/LLM-readable word diff of the description
/// (`- removed` / `+ added` lines with a few words of context), for a
/// diff-aware prompt. Used only by the offline replay harness today.
#[cfg(test)]
pub fn render_diff(old_text: &str, new_text: &str, context_words: usize) -> String {
    let (old_words, new_words) = (words(old_text), words(new_text));
    let old_keys: Vec<String> = old_words.iter().map(|w| w.key.clone()).collect();
    let new_keys: Vec<String> = new_words.iter().map(|w| w.key.clone()).collect();
    let (hunks, _) = diff(&old_keys, &new_keys);
    let join = |ws: &[Word]| {
        ws.iter()
            .map(|w| w.display.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut out = String::new();
    for h in hunks {
        let ctx_start = h.old_range.start.saturating_sub(context_words);
        let ctx_end = (h.old_range.end + context_words).min(old_words.len());
        out.push_str(&format!(
            "@@ ...{} [CHANGE] {}...\n",
            join(&old_words[ctx_start..h.old_range.start]),
            join(&old_words[h.old_range.end..ctx_end]),
        ));
        if !h.old_range.is_empty() {
            out.push_str(&format!("- {}\n", join(&old_words[h.old_range.clone()])));
        }
        if !h.new_range.is_empty() {
            out.push_str(&format!("+ {}\n", join(&new_words[h.new_range.clone()])));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_whitespace_and_entities_are_a_noop() {
        assert_eq!(
            classify(
                "Signal failure at Crewe",
                "<p>Trains&nbsp;may be   delayed.</p>",
                "Signal failure at Crewe",
                "<h4>Trains may be delayed</h4>\n<p></p>",
            ),
            EditClass::SemanticNoop
        );
    }

    #[test]
    fn case_and_in_word_punctuation_are_a_noop() {
        assert_eq!(
            classify("London Kings Cross", "x", "London King's Cross", "x"),
            EditClass::SemanticNoop
        );
        assert_eq!(
            classify("a", "Line closed", "a", "line closed."),
            EditClass::SemanticNoop
        );
    }

    #[test]
    fn word_boundaries_digits_and_letters_are_never_a_noop() {
        assert_ne!(
            classify("a", "platforms 1, 2", "a", "platforms 12"),
            EditClass::SemanticNoop
        );
        assert_ne!(
            classify("a", "is not running", "a", "is now running"),
            EditClass::SemanticNoop
        );
    }

    #[test]
    fn moving_text_between_summary_and_description_is_not_a_noop() {
        assert_ne!(
            classify("Signal failure", "at Crewe", "Signal failure at", "Crewe"),
            EditClass::SemanticNoop
        );
    }

    #[test]
    fn eta_bump_is_numeric_only() {
        assert_eq!(
            classify(
                "Disruption expected until 17:00",
                "Delays of up to 20 minutes. Expected until 17:00.",
                "Disruption expected until 18:30",
                "Delays of up to 15 minutes. Expected until 18:30.",
            ),
            EditClass::NumericOnly
        );
        // A time replaced by words is not numeric-only.
        assert_ne!(
            classify(
                "Disruption expected until 17:00",
                "Delays.",
                "Disruption expected until the end of the day",
                "Delays.",
            ),
            EditClass::NumericOnly
        );
    }

    #[test]
    fn append_and_rewrite_are_detected() {
        assert_eq!(
            classify(
                "s",
                "Lines are closed.",
                "s",
                "Lines are closed. Tickets accepted on buses."
            ),
            EditClass::Append
        );
        // A typical "has now ended" status transition: short, heavily
        // rewritten text lands in one of the two rewrite classes.
        assert!(matches!(
            classify(
                "Disruption between A and B expected until 17:00",
                "A signal failure means lines are closed. Trains may be cancelled.",
                "Disruption between A and B has now ended",
                "Disruption caused by a signal failure has now ended.",
            ),
            EditClass::PartialRewrite | EditClass::Rewrite
        ));
        assert_eq!(
            classify("x y z", "one two three four", "p q", "five six seven"),
            EditClass::Rewrite
        );
    }

    #[test]
    fn diff_reports_minimal_hunks() {
        let a: Vec<String> = "a b c d".split(' ').map(String::from).collect();
        let b: Vec<String> = "a x c d e".split(' ').map(String::from).collect();
        let (hunks, matched) = diff(&a, &b);
        assert_eq!(matched, 3);
        assert_eq!(
            hunks,
            vec![
                Hunk {
                    old_range: 1..2,
                    new_range: 1..2
                },
                Hunk {
                    old_range: 4..4,
                    new_range: 4..5
                },
            ]
        );
    }

    #[test]
    fn render_diff_shows_removed_and_added_words() {
        let rendered = render_diff(
            "Expected until 17:00 today.",
            "Expected until 18:00 today.",
            2,
        );
        assert!(rendered.contains("- 17:00"), "{rendered}");
        assert!(rendered.contains("+ 18:00"), "{rendered}");
    }
}
