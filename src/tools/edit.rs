//! The edit matching ladder (spec §8).
//!
//! A pure function over `(file content, old_string, new_string)`. The ladder is
//! an ordered list of match levels, first-success-wins, so a small formatting
//! deviation by the model does not fail the edit; the level that hit is reported
//! because a downgrade must never be silent. Three guardrails reject the
//! dangerous edits: a non-unique match, an over-large matched span, and a
//! placeholder phrase shaped like a comment.

use std::ops::Range;

/// The ordered ladder. First success wins; `Exact` is always tried first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchLevel {
    /// Byte-for-byte equality.
    Exact,
    /// Equal once trailing whitespace at end of line is ignored.
    LineEndWhitespace,
    /// Equal once every line is trimmed (indentation included).
    LineTrim,
}

impl MatchLevel {
    /// Every level, in ladder order.
    pub const LADDER: [MatchLevel; 3] = [
        MatchLevel::Exact,
        MatchLevel::LineEndWhitespace,
        MatchLevel::LineTrim,
    ];

    /// Stable name for diagnostics and the contract text in a tool result.
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchLevel::Exact => "exact",
            MatchLevel::LineEndWhitespace => "line-end-whitespace-insensitive",
            MatchLevel::LineTrim => "line-trim",
        }
    }
}

/// One accepted edit: where it lands, the bytes it replaces, and the replacement.
///
/// `old_text` is the **actual replaced region** from the file, not the caller's
/// `old_string`: a downgraded level matches different bytes, and that difference
/// is exactly what `/undo` needs to restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditMatch {
    pub level: MatchLevel,
    /// Byte range replaced in the original content.
    pub span: Range<usize>,
    /// The original bytes at `span`.
    pub old_text: String,
    /// The bytes that take `span`'s place.
    pub new_text: String,
}

/// Why a `/undo` could not reconstruct the content a snapshot came from.
///
/// Every variant is a refusal, never a guess: `/undo` either restores the exact
/// bytes the edit replaced or it leaves the workspace alone (spec §11).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RevertError {
    #[error("the file no longer contains the region this snapshot replaced")]
    Stale,
    #[error(
        "more than one region of the file could be the one this snapshot replaced; \
         the file has changed since the edit"
    )]
    Ambiguous,
    #[error(
        "this edit deleted the matched region, and the stream records no position for it; \
         it cannot be undone automatically"
    )]
    CannotLocateDeletion,
}

/// Why an edit was refused. Every variant carries the detail the model needs to
/// correct itself; none of them is silent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error(
        "no match found at any level of the ladder \
         (exact, line-end-whitespace-insensitive, line-trim)"
    )]
    NoMatch,
    #[error(
        "`old_string` matches {count} times; include more surrounding context to make it unique, \
         or pass replace_all to change every occurrence"
    )]
    NonUnique { count: usize, old_text: String },
    #[error(
        "matched region is {bytes} bytes / {lines} lines, too large for `old_string` \
         ({old_bytes} bytes); include the exact text you mean to replace"
    )]
    MatchTooLarge {
        bytes: usize,
        lines: usize,
        old_bytes: usize,
    },
    #[error(
        "`old_string` looks like a placeholder ({phrase:?}); it is a comment-shaped stand-in, \
         not text in the file. Pass the real text you want to replace"
    )]
    Placeholder { phrase: String },
}

/// Find the first successful level and return the one planned edit.
///
/// `old_string` and `new_string` are the caller's raw strings; `content` is the
/// file's current text.
pub fn find_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Result<EditMatch, EditError> {
    let mut edits = find_matches(content, old_string, new_string, false)?;
    Ok(edits.remove(0))
}

/// Plan every edit for one `old_string`/`new_string` pair.
///
/// With `replace_all` the match is looked up at the exact level only: a
/// downgraded level is there to absorb a formatting slip on a unique string, and
/// silently replacing every fuzzy hit would be exactly the "changed the wrong
/// thing" failure the guardrails exist to prevent. Without `replace_all`, more
/// than one hit is refused instead of guessed at.
pub fn find_matches(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<Vec<EditMatch>, EditError> {
    if let Some(phrase) = placeholder_phrase(old_string) {
        return Err(EditError::Placeholder { phrase });
    }
    if old_string.is_empty() {
        return Err(EditError::NoMatch);
    }

    if replace_all {
        let spans = find_all(content, old_string);
        if spans.is_empty() {
            return Err(EditError::NoMatch);
        }
        guard_span(content, &spans, old_string, true)?;
        return Ok(spans
            .into_iter()
            .map(|span| EditMatch {
                level: MatchLevel::Exact,
                old_text: content[span.clone()].to_owned(),
                span,
                new_text: new_string.to_owned(),
            })
            .collect());
    }

    for level in MatchLevel::LADDER {
        let spans = locate_all(content, old_string, level);
        if spans.is_empty() {
            continue;
        }
        guard_span(content, &spans, old_string, false)?;
        let span = spans[0].clone();
        return Ok(vec![EditMatch {
            level,
            old_text: content[span.clone()].to_owned(),
            span,
            new_text: new_string.to_owned(),
        }]);
    }
    Err(EditError::NoMatch)
}

/// Reconstruct the file content an edit started from: the inverse of the ladder.
///
/// `content` is the file as it is now, `before` is the **actual replaced bytes**
/// the edit recorded, and `old_string` / `new_string` / `replace_all` are the
/// edit's own arguments. The stream records no byte offset, so the region is
/// found by *verification*: a candidate restore is accepted only when replaying
/// the ladder over it reproduces the current content exactly. That makes the
/// answer a fact about the workspace rather than a guess, and it is what lets a
/// downgraded (line-trim) match be undone at all.
///
/// A pure deletion (`new_string` empty) carries no position the stream could
/// recover, so it is refused rather than guessed at.
pub fn revert(
    content: &str,
    before: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<String, RevertError> {
    if new_string.is_empty() {
        return Err(RevertError::CannotLocateDeletion);
    }
    if replace_all {
        return revert_all(content, before, old_string, new_string);
    }

    let mut restored: Option<String> = None;
    for span in find_all(content, new_string) {
        let candidate = splice(content, &span, before);
        if !replays(
            &candidate, old_string, new_string, span.start, before, content,
        ) {
            continue;
        }
        if restored.is_some() {
            return Err(RevertError::Ambiguous);
        }
        restored = Some(candidate);
    }
    restored.ok_or(RevertError::Stale)
}

/// Undo one `replace_all`: every occurrence the call replaced is an exact match,
/// so the snapshot is `old_string` repeated — which is what makes the regions
/// countable without a stored span.
fn revert_all(
    content: &str,
    before: &str,
    old_string: &str,
    new_string: &str,
) -> Result<String, RevertError> {
    // An empty search string makes `before` unsegmentable, and an empty search
    // is not a match any level would have produced.
    if old_string.is_empty() || before.is_empty() {
        return Err(RevertError::Stale);
    }
    if !before.len().is_multiple_of(old_string.len()) {
        return Err(RevertError::Stale);
    }
    let count = before.len() / old_string.len();
    if !before
        .as_bytes()
        .chunks(old_string.len())
        .all(|chunk| chunk == old_string.as_bytes())
    {
        return Err(RevertError::Stale);
    }

    // More or fewer `new_string`s than the call replaced means the file moved on
    // (or `new_string` also occurred elsewhere); either way the inverse is not
    // determined.
    if find_all(content, new_string).len() != count {
        return Err(RevertError::Stale);
    }

    let restored = content.replace(new_string, old_string);
    let edits = match find_matches(&restored, old_string, new_string, true) {
        Ok(edits)
            if edits.len() == count && edits.iter().all(|edit| edit.old_text == old_string) =>
        {
            edits
        }
        _ => return Err(RevertError::Stale),
    };
    let mut replay = restored.clone();
    for edit in edits.iter().rev() {
        replay.replace_range(edit.span.clone(), new_string);
    }
    if replay == content {
        Ok(restored)
    } else {
        Err(RevertError::Stale)
    }
}

/// Whether replaying the ladder over `candidate` really reproduces `content`,
/// with the match landing exactly on the bytes inserted at `expected_start`.
fn replays(
    candidate: &str,
    old_string: &str,
    new_string: &str,
    expected_start: usize,
    before: &str,
    content: &str,
) -> bool {
    let Ok(edits) = find_matches(candidate, old_string, new_string, false) else {
        return false;
    };
    let [edit] = edits.as_slice() else {
        return false;
    };
    edit.span.start == expected_start
        && edit.old_text == before
        && splice(candidate, &edit.span, new_string) == content
}

/// Replace one byte range of `text` with `replacement`.
fn splice(text: &str, span: &Range<usize>, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() - (span.end - span.start) + replacement.len());
    out.push_str(&text[..span.start]);
    out.push_str(replacement);
    out.push_str(&text[span.end..]);
    out
}

/// Every candidate span at one level, left to right.
///
/// The normalizing levels return spans into the **original** content: they walk
/// lines and map each normalized line back to its source offset, because the
/// matched bytes are what lands in `.before` and what the guardrails measure.
fn locate_all(content: &str, target: &str, level: MatchLevel) -> Vec<Range<usize>> {
    match level {
        MatchLevel::Exact => find_all(content, target),
        MatchLevel::LineEndWhitespace => walk_lines(content, target, LineShape::TrimEnd),
        MatchLevel::LineTrim => walk_lines(content, target, LineShape::Trim),
    }
}

/// Non-overlapping occurrences of `target`, left to right.
fn find_all(haystack: &str, target: &str) -> Vec<Range<usize>> {
    if target.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(found) = haystack[from..].find(target) {
        let start = from + found;
        let end = start + target.len();
        spans.push(start..end);
        from = end;
    }
    spans
}

/// Match line by line against `old_string`, returning spans into `content`.
///
/// The compared view of a line is decided by `shape`: exact lines compare whole,
/// trailing-whitespace-insensitive lines compare without their right blanks, and
/// line-trim lines compare their trimmed center. The returned span keeps the
/// line's leading blanks plus whatever the shape keeps, so the bytes that land
/// in `.before` are the bytes actually replaced.
fn walk_lines(content: &str, target: &str, shape: LineShape) -> Vec<Range<usize>> {
    if target.is_empty() {
        return Vec::new();
    }
    let target_lines: Vec<&str> = target.split_inclusive('\n').collect();

    let mut spans = Vec::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        // Every source line is a candidate start of the match.
        if let Some(end) = match_at(content, offset, &target_lines, shape) {
            spans.push(offset..end);
        }
        offset += line.len();
    }
    spans
}

/// Try to match `target_lines` starting at byte `offset`; return the span's end.
fn match_at(
    content: &str,
    offset: usize,
    target_lines: &[&str],
    shape: LineShape,
) -> Option<usize> {
    let mut end = offset;
    for (index, target_line) in target_lines.iter().enumerate() {
        let source_line = source_line_at(content, end)?;
        if !line_matches(source_line, target_line, shape) {
            return None;
        }
        let body = source_line.strip_suffix('\n').unwrap_or(source_line);
        let final_line = index + 1 == target_lines.len();
        // A target without a trailing newline stops the span before the source's
        // terminator; every other matched line keeps it.
        let keep_terminator = !final_line || target_line.ends_with('\n');
        end += kept_len(body, shape)
            + if keep_terminator {
                source_line.len() - body.len()
            } else {
                0
            };
    }
    Some(end)
}

/// The source line starting at `offset`, if any.
fn source_line_at(content: &str, offset: usize) -> Option<&str> {
    if offset >= content.len() {
        return None;
    }
    let rest = &content[offset..];
    let end = rest.find('\n').map(|index| index + 1).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Compare one line pair through `shape`, then decide whether the terminators
/// agree. A target line without a newline is always the final one, so it may
/// match a source line whose terminator lies just past the end of the span.
fn line_matches(source_line: &str, target_line: &str, shape: LineShape) -> bool {
    let source_body = source_line.strip_suffix('\n').unwrap_or(source_line);
    let target_body = target_line.strip_suffix('\n').unwrap_or(target_line);
    if line_view(source_body, shape) != line_view(target_body, shape) {
        return false;
    }
    // A target line that carries a newline needs a source line that carries one.
    !matches!(
        (source_line.ends_with('\n'), target_line.ends_with('\n')),
        (false, true)
    )
}

/// How a line is compared at a downgraded level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineShape {
    /// Trailing spaces and tabs at end of line are ignored.
    TrimEnd,
    /// The whole line is compared trimmed, indentation included.
    Trim,
}

/// The part of a line a shape compares.
fn line_view(body: &str, shape: LineShape) -> &str {
    match shape {
        LineShape::TrimEnd => trim_line_end_whitespace(body),
        LineShape::Trim => trim_line(body),
    }
}

/// How many bytes of a line the shape keeps, measured from the line's start.
///
/// Leading blanks always survive (a line-trim match still replaces the real
/// indentation); what the right-side trim removes does not.
fn kept_len(body: &str, shape: LineShape) -> usize {
    let viewed = line_view(body, shape);
    match shape {
        // `view` kept the leading blanks and dropped only the trailing ones, so
        // the kept span runs to the real end of the line body.
        LineShape::TrimEnd => body.len(),
        // `view` dropped the indentation; put the real indentation back.
        LineShape::Trim => leading_blank_len(body) + viewed.len(),
    }
}

/// The line-end-whitespace level keeps everything up to the line's trailing
/// blanks; `\r` counts as a blank because the CR of a CRLF is not content.
fn trim_line_end_whitespace(line: &str) -> &str {
    line.trim_end_matches([' ', '\t', '\r'])
}

/// The line-trim level drops indentation as well.
fn trim_line(line: &str) -> &str {
    line.trim_matches([' ', '\t', '\r'])
}

/// How many leading spaces or tabs a line-trim comparison drops.
fn leading_blank_len(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// The three guardrails, applied to every candidate span at the winning level.
///
/// They return the detail the model needs to correct itself rather than letting
/// a dangerous edit through silently.
fn guard_span(
    content: &str,
    spans: &[Range<usize>],
    old_string: &str,
    replace_all: bool,
) -> Result<(), EditError> {
    let first = spans.first().cloned().unwrap_or(0..0);
    let last = spans.last().cloned().unwrap_or(0..0);
    // The region a single edit would have to describe: from the first candidate
    // to the last, which is what "matched region too large" has to measure.
    let region = first.start..last.end;
    let bytes = region.end.saturating_sub(region.start);
    let lines = content[region].matches('\n').count();
    if bytes > MAX_MATCH_BYTES
        || lines > MAX_MATCH_LINES
        || bytes > old_string.len().saturating_mul(MAX_MATCH_RATIO)
    {
        return Err(EditError::MatchTooLarge {
            bytes,
            lines,
            old_bytes: old_string.len(),
        });
    }
    // Replacing every occurrence is a deliberate request, so it is not
    // ambiguous; without it, more than one candidate is refused rather than
    // guessed at.
    if !replace_all && spans.len() > 1 {
        return Err(EditError::NonUnique {
            count: spans.len(),
            old_text: old_string.to_owned(),
        });
    }
    Ok(())
}

/// A placeholder is a *comment-shaped* phrase, never a bare `..`, so Rust's
/// ranges (`..`, `..=`) are not mistaken for "and the rest of the file".
fn placeholder_phrase(old_string: &str) -> Option<String> {
    for line in old_string.lines() {
        let Some(body) = comment_body(line) else {
            continue;
        };
        let body = body.trim();
        if body.starts_with("...") {
            return Some("...".to_owned());
        }
        if body.starts_with('…') {
            return Some("…".to_owned());
        }
        // Only phrases that cannot be ordinary code: a bare "rest" (a variable,
        // a word in a sentence) is not evidence of a stand-in.
        for word in body.split_whitespace() {
            let word = word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_');
            if matches!(
                word.to_ascii_lowercase().as_str(),
                "existing" | "unchanged" | "elided" | "omitted"
            ) {
                return Some(word.to_owned());
            }
        }
        let lowercase = body.to_ascii_lowercase();
        if lowercase.starts_with("rest of") || lowercase.starts_with("the rest") {
            return Some("rest".to_owned());
        }
    }
    None
}

/// The text after a comment marker, for every comment shape the guardrail
/// recognises — doc comments (`///`, `//!`, `/** */`) included, since those are
/// the most likely place a model writes "and the rest of the file".
fn comment_body(line: &str) -> Option<&str> {
    let line = line.trim_start();
    for marker in ["///", "//!", "//", "/**", "/*", "*", "#"] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest);
        }
    }
    None
}

/// A matched span larger than this is not the edit the model meant.
const MAX_MATCH_BYTES: usize = 8 * 1024;
/// Nor is a matched span longer than this many lines.
const MAX_MATCH_LINES: usize = 200;
/// A matched span more than this many times the search length is suspicion
/// enough to refuse: a short search that hops across a region never meant it.
const MAX_MATCH_RATIO: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_match_is_found_at_the_exact_level() {
        let content = "fn main() {\n    run();\n}\n";
        let found = find_match(content, "    run();\n", "    run_twice();\n").unwrap();

        assert_eq!(found.level, MatchLevel::Exact);
        assert_eq!(found.old_text, "    run();\n");
        assert_eq!(found.span, 12..23);
    }

    #[test]
    fn trailing_whitespace_at_end_of_line_downgrades_to_the_second_level() {
        // The file has a trailing space after `run();` that the model did not
        // send. The edit still lands, and the level records the downgrade.
        let content = "fn main() {\n    run(); \n}\n";
        let found = find_match(content, "    run();\n", "    run_twice();\n").unwrap();

        assert_eq!(found.level, MatchLevel::LineEndWhitespace);
        assert_eq!(found.old_text, "    run(); \n");
        assert_eq!(found.span, 12..24);
    }

    #[test]
    fn indentation_differences_downgrade_to_the_third_level() {
        // The file indents with a tab where the model sent four spaces.
        let content = "fn main() {\n\trun();\n}\n";
        let found = find_match(content, "    run();", "    run_twice();").unwrap();

        assert_eq!(found.level, MatchLevel::LineTrim);
        assert_eq!(found.old_text, "\trun();");
    }

    #[test]
    fn the_ladder_is_first_success_wins_and_reports_the_highest_level() {
        let content = "alpha\n    beta\n";
        let found = find_match(content, "    beta\n", "    gamma\n").unwrap();

        assert_eq!(found.level, MatchLevel::Exact);
    }

    #[test]
    fn a_non_unique_old_string_is_refused_with_its_count() {
        let content = "let a = 1;\nlet b = 1;\n";
        let error = find_match(content, "= 1;", "= 2;").unwrap_err();

        match error {
            EditError::NonUnique { count, .. } => assert_eq!(count, 2),
            other => panic!("expected NonUnique, got {other}"),
        }
    }

    #[test]
    fn replace_all_is_how_a_non_unique_string_is_edited_on_purpose() {
        let content = "let a = 1;\nlet b = 1;\n";
        let edits = find_matches(content, "= 1;", "= 2;", true).unwrap();

        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].span, 6..10);
        assert_eq!(edits[1].span, 17..21);
    }

    #[test]
    fn a_match_that_spans_much_more_than_the_search_is_refused() {
        // `x` occurs throughout the file; the smallest region containing them
        // all is the whole file, which is not the edit the model meant.
        let content = format!("{}\n{}\n", "x".repeat(40), "x".repeat(40));
        let error = find_match(&content, "x", "y").unwrap_err();

        match error {
            EditError::MatchTooLarge {
                bytes,
                lines,
                old_bytes,
            } => {
                assert_eq!(bytes, 81, "the envelope from the first to the last hit");
                assert_eq!(lines, 1, "newlines inside the envelope, not lines touched");
                assert_eq!(old_bytes, 1);
            }
            other => panic!("expected MatchTooLarge, got {other}"),
        }
    }

    #[test]
    fn a_comment_shaped_placeholder_is_refused() {
        let error = find_match("fn main() {}\n", "// ...\n", "fn main() { run() }\n").unwrap_err();

        match error {
            EditError::Placeholder { phrase } => assert!(phrase.contains("..."), "{phrase}"),
            other => panic!("expected Placeholder, got {other}"),
        }
    }

    #[test]
    fn a_doc_comment_placeholder_is_refused() {
        for old_string in ["/// ...", "//! ...", "/** ... */", "# ..."] {
            let error = find_match("fn main() {}\n", old_string, "x").unwrap_err();
            assert!(
                matches!(error, EditError::Placeholder { .. }),
                "{old_string:?} should be a placeholder, got {error}"
            );
        }
    }

    #[test]
    fn ordinary_comments_are_not_placeholders() {
        // "rest" as an ordinary word, and a comment that happens to contain
        // "remaining", must not be refused: a false positive here makes a
        // legitimate edit impossible.
        let content = "// the rest is computed below\nlet x = 1;\n";
        assert!(find_match(content, "let x = 1;", "let x = 2;").is_ok());
        let content = "// keep the remaining bytes\n";
        assert!(find_match(content, "// keep the remaining bytes", "// done").is_ok());
    }

    #[test]
    fn rust_range_syntax_is_not_mistaken_for_a_placeholder() {
        // `..` and `..=` are language tokens, not "and the rest of the file".
        let content = "let tail = &items[1..];\nlet all = &items[..=9];\n";
        assert!(find_match(content, "items[1..]", "items[0..]").is_ok());
        assert!(find_match(content, "items[..=9]", "items[..=8]").is_ok());
    }

    #[test]
    fn revert_restores_an_exact_edit_from_its_snapshot() {
        let restored = revert("uno\ntwo\n", "one\n", "one\n", "uno\n", false).unwrap();

        assert_eq!(restored, "one\ntwo\n");
    }

    #[test]
    fn revert_restores_the_bytes_a_downgraded_match_replaced() {
        // The model sent four spaces, the file had a tab: `.before` holds the
        // tab, and the naive `old_string` would not match the restored file.
        let restored = revert(
            "fn main() {\n    run_twice();\n}\n",
            "\trun();",
            "    run();",
            "    run_twice();",
            false,
        )
        .unwrap();

        assert_eq!(restored, "fn main() {\n\trun();\n}\n");
    }

    #[test]
    fn revert_undoes_replace_all_from_the_repeated_snapshot() {
        let restored =
            revert("let a = 2;\nlet b = 2;\n", "= 1;= 1;", "= 1;", "= 2;", true).unwrap();

        assert_eq!(restored, "let a = 1;\nlet b = 1;\n");
    }

    #[test]
    fn revert_refuses_when_the_file_no_longer_holds_the_edit() {
        let error = revert("something else\n", "one\n", "one\n", "uno\n", false).unwrap_err();

        assert_eq!(error, RevertError::Stale);
    }

    #[test]
    fn revert_refuses_when_two_regions_could_be_the_one_replaced() {
        // Both "a b" and "b a" replay to "b b"; the snapshot cannot say which.
        let error = revert("b b", "a", "a", "b", false).unwrap_err();

        assert_eq!(error, RevertError::Ambiguous);
    }

    #[test]
    fn revert_refuses_a_pure_deletion() {
        let error = revert("two\n", "one\n", "one\n", "", false).unwrap_err();

        assert_eq!(error, RevertError::CannotLocateDeletion);
    }

    #[test]
    fn revert_refuses_when_the_replacement_also_occurs_outside_the_edit() {
        // The call replaced two occurrences, but the file now holds a third
        // "= 2;" that was already there: the inverse would corrupt it.
        let error = revert(
            "let a = 2;\nlet c = 2;\nlet b = 2;\n",
            "= 1;= 1;",
            "= 1;",
            "= 2;",
            true,
        )
        .unwrap_err();

        assert_eq!(error, RevertError::Stale);
    }
}
