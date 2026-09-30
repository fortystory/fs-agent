//! 编辑匹配阶梯（spec §8）。
//!
//! 一个作用在 `(文件内容, old_string, new_string)` 上的纯函数。阶梯是一张有序的匹配等级表，先
//! 成功者胜，于是模型一点小小的格式偏差不会让这次编辑失败；而命中的那一档会被报出来，因为降档
//! 绝不允许无声。三条护栏拒掉危险的编辑：不唯一的匹配、过大的匹配区段，以及一条注释形状的占位
//! 短语。

use std::ops::Range;

/// 那张有序的阶梯。先成功者胜；`Exact` 永远最先试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchLevel {
    /// 逐字节相等。
    Exact,
    /// 忽略行尾空白后相等。
    LineEndWhitespace,
    /// 每一行都 trim 后相等（含缩进）。
    LineTrim,
}

impl MatchLevel {
    /// 每一档，按阶梯顺序。
    pub const LADDER: [MatchLevel; 3] = [
        MatchLevel::Exact,
        MatchLevel::LineEndWhitespace,
        MatchLevel::LineTrim,
    ];

    /// 给诊断、以及工具结果里那段契约文本用的稳定名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchLevel::Exact => "exact",
            MatchLevel::LineEndWhitespace => "line-end-whitespace-insensitive",
            MatchLevel::LineTrim => "line-trim",
        }
    }
}

/// 一处被接受的编辑：它落在哪、替换掉哪些字节，以及替换成什么。
///
/// `old_text` 是文件里**实际被替换的那段区域**，不是调用方给的 `old_string`：降档后的匹配会匹配
/// 上不同的字节，而那个差异正是 `/undo` 需要用来还原的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditMatch {
    pub level: MatchLevel,
    /// 在原内容里被替换掉的字节区间。
    pub span: Range<usize>,
    /// `span` 处原来的那些字节。
    pub old_text: String,
    /// 取代 `span` 的那些字节。
    pub new_text: String,
}

/// 一次 `/undo` 为什么重建不出快照来自的那份内容。
///
/// 每个变体都是一次拒绝，绝不是一次猜测：`/undo` 要么还原这次编辑替换掉的精确字节，要么就不碰
/// 工作区（spec §11）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RevertError {
    #[error("文件里已经找不到这份快照替换掉的那段区域")]
    Stale,
    #[error("文件里不止一段区域可能是这份快照替换掉的那处；这次编辑之后文件又变过了")]
    Ambiguous,
    #[error("这次编辑删掉了匹配上的那段区域，而流上没有记下它的位置；它没法自动撤销")]
    CannotLocateDeletion,
}

/// 一次编辑为什么被拒。每个变体都带着模型据以自我纠正的细节；没有一个变体是无声的。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error(
        "整条匹配阶梯上都没有找到匹配（\
         exact、line-end-whitespace-insensitive、line-trim）"
    )]
    NoMatch,
    #[error(
        "`old_string` 匹配上了 {count} 处；多带上一些上下文让它唯一，或者传 \
         `replace_all` 把每一处都替换掉"
    )]
    NonUnique { count: usize, old_text: String },
    #[error(
        "匹配上的区域有 {bytes} 字节 / {lines} 行，对 `old_string`（{old_bytes} 字节）来说\
         太大了；请把你要替换的原文一字不差地写上"
    )]
    MatchTooLarge {
        bytes: usize,
        lines: usize,
        old_bytes: usize,
    },
    #[error(
        "`old_string` 看起来是个占位符（{phrase:?}）；它是一条注释形状的替身，不是文件里的\
         文本。请把你要替换的真实文本传进来"
    )]
    Placeholder { phrase: String },
}

/// 找到第一个成功的等级，并返回那一处已规划的编辑。
///
/// `old_string` 与 `new_string` 是调用方给的原始字符串；`content` 是文件当前的文本。
pub fn find_match(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Result<EditMatch, EditError> {
    let mut edits = find_matches(content, old_string, new_string, false)?;
    Ok(edits.remove(0))
}

/// 为一对 `old_string`/`new_string` 规划出每一处编辑。
///
/// 带 `replace_all` 时，匹配只在精确那一档找：降档的存在是为了吸收一个唯一字符串上的格式偏差，
/// 而悄悄替换每一处模糊命中，正是护栏存在要防的那种「改错了东西」的失败。不带 `replace_all` 时，
/// 命中多于一处会被拒掉，而不是去猜。
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

/// 重建一次编辑开始前的那份文件内容：阶梯的逆。
///
/// `content` 是文件现在的样子，`before` 是这次编辑记录下来的**实际被替换字节**，而 `old_string` /
/// `new_string` / `replace_all` 是这次编辑自己的参数。流上没有记字节偏移，所以那段区域是靠*验证*
/// 找出来的：只有当在一个候选还原结果上重放阶梯能一字不差地重现当前内容时，这个候选才被接受。这
/// 让答案成为关于工作区的一个事实、而不是一次猜测，也正是它让一次降档（line-trim）的匹配也能被
/// 撤销。
///
/// 一次纯删除（`new_string` 为空）不携带流能恢复的位置，所以它被拒掉，而不是去猜。
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

/// 撤销一次 `replace_all`：这次调用替换掉的每一处都是精确匹配，所以快照就是 `old_string` 重复
/// 出来的样子 —— 正是这一条让那些区域不必存 span 也能数清楚。
fn revert_all(
    content: &str,
    before: &str,
    old_string: &str,
    new_string: &str,
) -> Result<String, RevertError> {
    // 空的搜索串会让 `before` 分不了段，而空搜索也不是任何一档会产出的匹配。
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

    // 比这次调用替换的处数更多或更少的 `new_string`，意味着文件又往前走过了（或者 `new_string`
    // 也在别处出现过）；无论哪种，逆都不唯一确定。
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

/// 在 `candidate` 上重放阶梯是否真的重现了 `content`，且匹配恰好落在 `expected_start` 处插入
/// 的那些字节上。
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

/// 把 `text` 的一个字节区间换成 `replacement`。
fn splice(text: &str, span: &Range<usize>, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() - (span.end - span.start) + replacement.len());
    out.push_str(&text[..span.start]);
    out.push_str(replacement);
    out.push_str(&text[span.end..]);
    out
}

/// 某一档上的全部候选区段，从左到右。
///
/// 那两个归一化档返回的是指向**原内容**的区段：它们逐行走，把每个归一化后的行映射回它的源偏移，
/// 因为落进 `.before` 的、以及护栏所度量的，是那些被匹配上的字节。
fn locate_all(content: &str, target: &str, level: MatchLevel) -> Vec<Range<usize>> {
    match level {
        MatchLevel::Exact => find_all(content, target),
        MatchLevel::LineEndWhitespace => walk_lines(content, target, LineShape::TrimEnd),
        MatchLevel::LineTrim => walk_lines(content, target, LineShape::Trim),
    }
}

/// `target` 互不重叠的出现，从左到右。
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

/// 逐行对着 `old_string` 匹配，返回指向 `content` 的区段。
///
/// 一行被比较的视图由 `shape` 决定：精确的行整行比较，忽略行尾空白的行去掉右侧空白再比较，
/// line-trim 的行比较它们 trim 之后的中段。返回的区段保留该行的前导空白、加上 shape 保留的那些，
/// 所以落进 `.before` 的字节就是实际被替换的字节。
fn walk_lines(content: &str, target: &str, shape: LineShape) -> Vec<Range<usize>> {
    if target.is_empty() {
        return Vec::new();
    }
    let target_lines: Vec<&str> = target.split_inclusive('\n').collect();

    let mut spans = Vec::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        // 每一行源文本都是一个候选的匹配起点。
        if let Some(end) = match_at(content, offset, &target_lines, shape) {
            spans.push(offset..end);
        }
        offset += line.len();
    }
    spans
}

/// 试着从字节 `offset` 处开始匹配 `target_lines`；返回区段的结束位置。
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
        // 末尾不带换行的目标让区段停在源文本那个终止符之前；其他被匹配上的行都保留它。
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

/// 从 `offset` 开始的那一行源文本，如果有的话。
fn source_line_at(content: &str, offset: usize) -> Option<&str> {
    if offset >= content.len() {
        return None;
    }
    let rest = &content[offset..];
    let end = rest.find('\n').map(|index| index + 1).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// 通过 `shape` 比较一对行，然后判定两个终止符是否一致。一条不带换行的目标行永远是最后一行，所以
/// 它可以匹配上一条终止符恰好落在区段末尾之外的源行。
fn line_matches(source_line: &str, target_line: &str, shape: LineShape) -> bool {
    let source_body = source_line.strip_suffix('\n').unwrap_or(source_line);
    let target_body = target_line.strip_suffix('\n').unwrap_or(target_line);
    if line_view(source_body, shape) != line_view(target_body, shape) {
        return false;
    }
    // 一条带换行的目标行需要一条也带换行的源行。
    !matches!(
        (source_line.ends_with('\n'), target_line.ends_with('\n')),
        (false, true)
    )
}

/// 在降档的等级上，一行是怎么被比较的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineShape {
    /// 行尾的空格与制表符被忽略。
    TrimEnd,
    /// 整行 trim 后比较，含缩进。
    Trim,
}

/// 一个 shape 所比较的那部分行。
fn line_view(body: &str, shape: LineShape) -> &str {
    match shape {
        LineShape::TrimEnd => trim_line_end_whitespace(body),
        LineShape::Trim => trim_line(body),
    }
}

/// 一个 shape 保留一行的多少个字节，从行首算起。
///
/// 前导空白永远留下（一次 line-trim 匹配仍然替换真实的缩进）；右侧 trim 去掉的那些不留。
fn kept_len(body: &str, shape: LineShape) -> usize {
    let viewed = line_view(body, shape);
    match shape {
        // `view` 保住了前导空白、只丢掉尾部空白，所以保留下来的区段一直延伸到行正文的真实末尾。
        LineShape::TrimEnd => body.len(),
        // `view` 丢掉了缩进；把真实缩进放回去。
        LineShape::Trim => leading_blank_len(body) + viewed.len(),
    }
}

/// line-end-whitespace 那一档保留到该行尾部空白之前的一切；`\r` 算作空白，因为 CRLF 里的 CR 不是
/// 内容。
fn trim_line_end_whitespace(line: &str) -> &str {
    line.trim_end_matches([' ', '\t', '\r'])
}

/// line-trim 那一档连缩进一起丢掉。
fn trim_line(line: &str) -> &str {
    line.trim_matches([' ', '\t', '\r'])
}

/// 一次 line-trim 比较丢掉多少个前导空格或制表符。
fn leading_blank_len(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// 那三条护栏，施加在胜出那一档的每一个候选区段上。
///
/// 它们返回的是模型据以自我纠正的细节，而不是让一次危险的编辑无声通过。
fn guard_span(
    content: &str,
    spans: &[Range<usize>],
    old_string: &str,
    replace_all: bool,
) -> Result<(), EditError> {
    let first = spans.first().cloned().unwrap_or(0..0);
    let last = spans.last().cloned().unwrap_or(0..0);
    // 单处编辑将不得不描述的那段区域：从第一个候选到最后一个候选，这正是「匹配区域过大」必须
    // 度量的东西。
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
    // 替换每一处是一个刻意的请求，所以它不歧义；没有它时，候选多于一处会被拒掉，而不是去猜。
    if !replace_all && spans.len() > 1 {
        return Err(EditError::NonUnique {
            count: spans.len(),
            old_text: old_string.to_owned(),
        });
    }
    Ok(())
}

/// 占位符是一条*注释形状*的短语，绝不是光秃秃的 `..`，所以 Rust 的范围语法（`..`、`..=`）不会
/// 被误当成「以及文件其余部分」。
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
        // 只认那些不可能出现在普通代码里的短语：一个光秃秃的 "rest"（一个变量、句子里的一个词）
        // 不是占位符的证据。
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

/// 注释标记之后的那段文本，覆盖这条护栏认得的每一种注释形状 —— 含文档注释（`///`、`//!`、
/// `/** */`），因为那正是模型最可能写下「以及文件其余部分」的地方。
fn comment_body(line: &str) -> Option<&str> {
    let line = line.trim_start();
    for marker in ["///", "//!", "//", "/**", "/*", "*", "#"] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest);
        }
    }
    None
}

/// 比这个还大的匹配区段，不是模型想要的那次编辑。
const MAX_MATCH_BYTES: usize = 8 * 1024;
/// 比这么多行还长的匹配区段同样不是。
const MAX_MATCH_LINES: usize = 200;
/// 匹配区段超过搜索长度这么多倍就足以起疑而拒掉：一段跨过从来不想匹配的区域的很短的搜索，不是
/// 本意。
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
        // 文件里 `run();` 后面有一个模型没发的行尾空格。编辑仍然落地，而那一档记录下了这次降档。
        let content = "fn main() {\n    run(); \n}\n";
        let found = find_match(content, "    run();\n", "    run_twice();\n").unwrap();

        assert_eq!(found.level, MatchLevel::LineEndWhitespace);
        assert_eq!(found.old_text, "    run(); \n");
        assert_eq!(found.span, 12..24);
    }

    #[test]
    fn indentation_differences_downgrade_to_the_third_level() {
        // 文件缩进用的是制表符，而模型发的是四个空格。
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
            other => panic!("期望 NonUnique，得到 {other}"),
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
        // `x` 遍布整个文件；把它们全包住的最小区域就是整个文件，那不是模型想要的那次编辑。
        let content = format!("{}\n{}\n", "x".repeat(40), "x".repeat(40));
        let error = find_match(&content, "x", "y").unwrap_err();

        match error {
            EditError::MatchTooLarge {
                bytes,
                lines,
                old_bytes,
            } => {
                assert_eq!(bytes, 81, "从第一个命中到最后一个命中的包络");
                assert_eq!(lines, 1, "包络内部的换行数，不是被触及的行数");
                assert_eq!(old_bytes, 1);
            }
            other => panic!("期望 MatchTooLarge，得到 {other}"),
        }
    }

    #[test]
    fn a_comment_shaped_placeholder_is_refused() {
        let error = find_match("fn main() {}\n", "// ...\n", "fn main() { run() }\n").unwrap_err();

        match error {
            EditError::Placeholder { phrase } => assert!(phrase.contains("..."), "{phrase}"),
            other => panic!("期望 Placeholder，得到 {other}"),
        }
    }

    #[test]
    fn a_doc_comment_placeholder_is_refused() {
        for old_string in ["/// ...", "//! ...", "/** ... */", "# ..."] {
            let error = find_match("fn main() {}\n", old_string, "x").unwrap_err();
            assert!(
                matches!(error, EditError::Placeholder { .. }),
                "{old_string:?} 应该是个占位符，得到 {error}"
            );
        }
    }

    #[test]
    fn ordinary_comments_are_not_placeholders() {
        // 作为一个普通词的 "rest"，以及一条恰好含 "remaining" 的注释，都不许被拒：这里的假阳性会
        // 让一次正当的编辑变得不可能。
        let content = "// the rest is computed below\nlet x = 1;\n";
        assert!(find_match(content, "let x = 1;", "let x = 2;").is_ok());
        let content = "// keep the remaining bytes\n";
        assert!(find_match(content, "// keep the remaining bytes", "// done").is_ok());
    }

    #[test]
    fn rust_range_syntax_is_not_mistaken_for_a_placeholder() {
        // `..` 与 `..=` 是语言记号，不是「以及文件其余部分」。
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
        // 模型发的是四个空格，文件里是制表符：`.before` 里存的是那个制表符，而光用 `old_string`
        // 对不上还原后的文件。
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
        // "a b" 与 "b a" 重放后都得到 "b b"；快照说不出是哪一个。
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
        // 那次调用替换了两处，但文件现在多出一条本来就在那儿的 "= 2;"：这条逆会把它改坏。
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
