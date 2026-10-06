//! 可点链接的识别层：从**画出来的显示行**里认出 URL 与文件路径
//! （`.scratch/clickable-links/spec.md` §2）。
//!
//! 为什么在画出来的文本上认，而不是在 Markdown 解析那一层认：`/eli5` 的产物是一条**纯文本
//! 路径**（它连 `Tag::Link` 都不是），代码块里的 URL 也一样要认，而一条 URL 被窗格折成两片
//! 之后还得认得出。所以这一层收的是**显示行**，与**选区**同一层记账：每帧重算，指针事件读
//! 上一帧的账。
//!
//! 这一层**不做 IO**：一条路径候选真不真存在、在不在工作区里，是点击那一刻的事（§3）。
//! 它也因此是一条可以逐字断言的纯函数 —— 判据写在下面几张表里，不在文件系统里。

use std::ops::Range;
use std::path::Path;

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::width::{char_columns, text_columns};

/// 一个候选指向什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `http://` / `https://` 开头的地址，原样交给系统默认程序。
    Url(String),
    /// 一段**看起来像路径**的文本。存在性与区内外由点击那一刻判。
    Path(String),
}

/// 一条显示行里的一个候选。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotspot {
    /// 占了这一行的哪几列：从这一行的第 0 列起、按显示宽度数（`lead` 不在里面 —— 那是
    /// 区域自己的留白，由 [`crate::render::selection::TextRow`] 另记）。
    pub columns: Range<usize>,
    pub target: Target,
}

impl Hotspot {
    /// 这一列落在这个候选里吗（命中判据，规格 §3）。
    pub fn covers(&self, column: usize) -> bool {
        self.columns.contains(&column)
    }
}

impl Target {
    /// 点击那一刻把候选解析成真正要打开的东西（规格 §3）。
    ///
    /// URL 原样；路径要解析成绝对路径、**真存在**、而且**落在工作区里** —— 解不开、不存在、
    /// 区外都交回 `None`，也就是「什么都不发生」。
    ///
    /// 它与识别那一半有意不同：识别是纯函数、不碰文件系统（每帧都跑），这一条是点击那一刻
    /// 的一次 IO —— 一次点击换一次 `canonicalize`，而且只有真的点到候选上才会发生。
    /// `canonicalize` 顺手把 `..` 与符号链接一起解开，所以「区外」这条判据判的是**最终的**
    /// 落点，不是写出来的那串字。
    pub fn resolve(&self, cwd: &Path) -> Option<String> {
        match self {
            Target::Url(url) => Some(url.clone()),
            Target::Path(path) => {
                let root = cwd.canonicalize().ok()?;
                let full = root.join(path).canonicalize().ok()?;
                full.starts_with(&root)
                    .then(|| full.to_string_lossy().into_owned())
            }
        }
    }
}

/// 认出这一行文本里的候选，按列从左到右。
///
/// 判据是一张表，不是一组例外：URL 认协议头；路径认 `/`，或末段那个「扩展名」；
/// `path:line` 与带 scheme 的 token（`mailto:` 一类）都不算（规格 §2 的表）。
pub fn hotspots(text: &str) -> Vec<Hotspot> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    // 每个字符的起始列。扫描是顺序的，所以这里算一遍就够。
    let mut columns = Vec::with_capacity(chars.len() + 1);
    let mut column = 0usize;
    for (_, ch) in &chars {
        columns.push(column);
        column += char_columns(*ch);
    }
    columns.push(column);

    let mut found = Vec::new();
    let mut at = 0usize;
    while at < chars.len() {
        if separates(chars[at].1) {
            at += 1;
            continue;
        }
        let start = at;
        while at < chars.len() && !separates(chars[at].1) {
            at += 1;
        }
        let (from, to) = trim(&chars, start, at);
        if from >= to {
            continue;
        }
        let raw = slice(text, &chars, from, to);
        let Some(target) = classify(raw) else {
            continue;
        };
        let range = columns[from]..columns[to];
        if !range.is_empty() {
            found.push(Hotspot {
                columns: range,
                target,
            });
        }
    }
    found
}

/// 就地给这些行里的候选加上下划线，并交出**按行**的候选表（与 `rows` 平行）。
///
/// 软折的续行要拼回一条再认（`.scratch/tui-feedback/spec.md` §5 的拼回，与复制同源）：
/// 一条 URL 被折成两片之后，两片各自都认不出它。拼回只用来**认**，认出之后每个候选按列切回
/// 它真正落在的那几片 —— 命中还是按屏幕上的列算。
pub fn mark(rows: &mut [Line<'static>], folded: &[bool]) -> Vec<Vec<Hotspot>> {
    let mut found: Vec<Vec<Hotspot>> = vec![Vec::new(); rows.len()];
    let mut head = 0usize;
    while head < rows.len() {
        let mut tail = head + 1;
        while tail < rows.len() && folded.get(tail).copied().unwrap_or(false) {
            tail += 1;
        }
        let pieces: Vec<String> = (head..tail).map(|row| row_text(&rows[row])).collect();
        let widths: Vec<usize> = pieces.iter().map(|piece| text_columns(piece)).collect();
        let joined: String = pieces.concat();
        let mut offsets = Vec::with_capacity(pieces.len());
        let mut at = 0usize;
        for width in &widths {
            offsets.push(at);
            at += width;
        }
        for hot in hotspots(&joined) {
            for (index, offset) in offsets.iter().enumerate() {
                let lo = hot.columns.start.max(*offset);
                let hi = hot.columns.end.min(offset + widths[index]);
                if lo >= hi {
                    continue;
                }
                let row = head + index;
                let columns = (lo - offset)..(hi - offset);
                underline(&mut rows[row], &columns);
                found[row].push(Hotspot {
                    columns,
                    target: hot.target.clone(),
                });
            }
        }
        head = tail;
    }
    found
}

// ------------------------------------------------------------------ 扫描

/// 一个 token 到哪儿为止。
///
/// 中文标点与全角括号都在这里：它们是行文，不是地址的一部分，而它们与地址之间通常**没有**
/// 空白（`见 .scratch/x.html）` 这种形态），所以光按空白切是不够的。半角括号**不**在此列 ——
/// 它们可能是地址自己的一部分（`…/wiki/Foo_(bar)`），由 [`trim`] 按配对与否决定去留。
fn separates(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(
            ch,
            '"' | '\''
                | '`'
                | '<'
                | '>'
                | '['
                | ']'
                | '{'
                | '}'
                | '|'
                | '，'
                | '。'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '（'
                | '）'
                | '【'
                | '】'
                | '「'
                | '」'
                | '『'
                | '』'
                | '《'
                | '》'
                | '…'
                | '·'
        )
}

/// 贴着 token 两端的标点：剥掉，它们不属于候选。
fn trims(ch: char) -> bool {
    matches!(
        ch,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '。'
            | '，'
            | '、'
            | '；'
            | '：'
            | '！'
            | '？'
            | '·'
            | '…'
    )
}

/// 一对括号的另一半。
fn matching(ch: char) -> Option<char> {
    Some(match ch {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => return None,
    })
}

/// 剥掉 token 两端不属于它的东西：成对（或孤立）的括号、收尾标点。返回字符下标区间。
fn trim(chars: &[(usize, char)], mut start: usize, mut end: usize) -> (usize, usize) {
    loop {
        if start >= end {
            return (start, end);
        }
        let first = chars[start].1;
        let last = chars[end - 1].1;
        // 成对包裹：`(x.html)` 的两个括号一起走。
        if matching(first) == Some(last) {
            start += 1;
            end -= 1;
            continue;
        }
        // 孤立的闭括号：`(见 x.html)` 里那个 `)` 只有一边。
        if let Some(twin) = matching(last) {
            let left = chars[start..end]
                .iter()
                .filter(|(_, ch)| *ch == twin)
                .count();
            let right = chars[start..end]
                .iter()
                .filter(|(_, ch)| *ch == last)
                .count();
            if right > left {
                end -= 1;
                continue;
            }
        }
        if trims(last) {
            end -= 1;
            continue;
        }
        // 孤立的开括号：`(x.html` 只有一边。**头部只认括号**，不收尾标点 ——
        // `.scratch/x.html` 与 `./x` 那一个点不是标点，它是路径的一部分。
        if let Some(twin) = matching(first) {
            let left = chars[start..end]
                .iter()
                .filter(|(_, ch)| *ch == first)
                .count();
            let right = chars[start..end]
                .iter()
                .filter(|(_, ch)| *ch == twin)
                .count();
            if left > right {
                start += 1;
                continue;
            }
        }
        return (start, end);
    }
}

/// `chars[from..to]` 在原文里的那一段。
fn slice<'a>(text: &'a str, chars: &[(usize, char)], from: usize, to: usize) -> &'a str {
    let start = chars[from].0;
    let end = chars.get(to).map_or(text.len(), |(at, _)| *at);
    &text[start..end]
}

/// 一个 token 是什么（或者什么都不是）。
fn classify(token: &str) -> Option<Target> {
    if is_url(token) {
        return Some(Target::Url(token.to_owned()));
    }
    if looks_like_path(token) {
        return Some(Target::Path(token.to_owned()));
    }
    None
}

fn is_url(token: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        token
            .get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// 看起来像一条路径吗。
///
/// 三条判据按顺序：带 scheme 的（`mailto:`）不是；`path:line` 不是；剩下的要含 `/`，或者末段
/// 带一个像扩展名的东西。最后那条是 `README.md` 与 `x.html` 得以成立的原因，也是 `1.0.0`
/// 与 `e.g.` 被挡在外面的原因（扩展名必须**以字母开头**）。
fn looks_like_path(token: &str) -> bool {
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return false;
    }
    if has_scheme(token) || is_path_line(token) {
        return false;
    }
    if !token.chars().any(|ch| ch.is_alphanumeric() || ch == '_') {
        return false;
    }
    token.contains('/') || has_extension(token)
}

/// `scheme:` 开头 —— 出网那一类工具里的 `mailto:`、`ssh:` 都算。
fn has_scheme(token: &str) -> bool {
    let Some((head, _)) = token.split_once(':') else {
        return false;
    };
    let mut chars = head.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

/// `path:line` —— 那是编辑器那一档，不是浏览器（规格 §2）。
fn is_path_line(token: &str) -> bool {
    let Some((_, tail)) = token.rsplit_once(':') else {
        return false;
    };
    !tail.is_empty() && tail.chars().all(|ch| ch.is_ascii_digit())
}

/// 末段带一个像扩展名的东西。
fn has_extension(token: &str) -> bool {
    let last = token.rsplit('/').next().unwrap_or(token);
    let Some((stem, ext)) = last.rsplit_once('.') else {
        return false;
    };
    let mut chars = ext.chars();
    !stem.is_empty()
        && (1..=6).contains(&chars.clone().count())
        && matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric())
}

// ------------------------------------------------------------------ 画

/// 给一条行里的某几列加上下划线。
///
/// 按**字符**重建 span 列表，而不是去切 span 的字节：列与字符不是一一对应（一个宽字符两列），
/// 逐字符走是唯一能保证不切开一个宽字符的做法。一个候选的列区间总是落在字符边界上，所以
/// 「取字符的起始列」就是它要的那几格。
fn underline(line: &mut Line<'static>, columns: &Range<usize>) {
    if columns.is_empty() {
        return;
    }
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 2);
    let mut column = 0usize;
    for span in line.spans.drain(..) {
        let hot_style = span.style.patch(Modifier::UNDERLINED);
        let mut run = String::new();
        let mut run_hot = false;
        for ch in span.content.chars() {
            let hot = columns.contains(&column);
            if !run.is_empty() && hot != run_hot {
                let style = if run_hot { hot_style } else { span.style };
                spans.push(Span::styled(std::mem::take(&mut run), style));
            }
            run_hot = hot;
            run.push(ch);
            column += char_columns(ch);
        }
        if !run.is_empty() {
            let style = if run_hot { hot_style } else { span.style };
            spans.push(Span::styled(run, style));
        }
    }
    line.spans = spans;
}

/// 一条显示行的文本（拼接它的 span）。
pub(crate) fn row_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这一行里认出来的候选，按「列区间 + 目标」写出来。
    fn found(text: &str) -> Vec<(Range<usize>, Target)> {
        hotspots(text)
            .into_iter()
            .map(|hot| (hot.columns, hot.target))
            .collect()
    }

    fn url(text: &str) -> Target {
        Target::Url(text.to_owned())
    }

    fn path(text: &str) -> Target {
        Target::Path(text.to_owned())
    }

    #[test]
    fn a_bare_url_is_a_candidate() {
        assert_eq!(
            found("见 https://example.com/x"),
            vec![(3..24, url("https://example.com/x"))]
        );
    }

    #[test]
    fn a_chinese_full_stop_is_not_part_of_the_url() {
        assert_eq!(
            found("见 https://a.example/b。"),
            vec![(3..22, url("https://a.example/b"))]
        );
    }

    #[test]
    fn a_wrapped_path_loses_its_wrapping() {
        assert_eq!(
            found("（见 .scratch/sandbox/eli5-sandbox.html）"),
            vec![(5..39, path(".scratch/sandbox/eli5-sandbox.html"))]
        );
    }

    #[test]
    fn a_half_width_wrapped_path_loses_its_wrapping_too() {
        assert_eq!(found("(见 x.html)"), vec![(4..10, path("x.html"))]);
    }

    #[test]
    fn a_relative_path_is_a_candidate() {
        assert_eq!(
            found("prototype/embed-nvim/README.md"),
            vec![(0..30, path("prototype/embed-nvim/README.md"))]
        );
    }

    #[test]
    fn a_bare_file_name_with_an_extension_is_a_candidate() {
        assert_eq!(found("README.md"), vec![(0..9, path("README.md"))]);
    }

    #[test]
    fn a_path_with_a_line_number_is_not() {
        assert!(found("src/render/tui.rs:2867").is_empty());
    }

    #[test]
    fn a_bare_word_is_not() {
        assert!(found("已经写好了").is_empty());
    }

    #[test]
    fn another_scheme_is_not_a_path() {
        assert!(found("mailto:ada@example.com").is_empty());
    }

    #[test]
    fn a_version_number_is_not_a_path() {
        assert!(found("mermaid-text 1.0.0 能画").is_empty());
    }

    #[test]
    fn a_sentence_full_of_punctuation_only_yields_the_addresses() {
        assert_eq!(
            found("跑 /eli5 生成了：.scratch/eli5/x.html，见 https://example.com/y。"),
            vec![
                (3..8, path("/eli5")),
                (17..37, path(".scratch/eli5/x.html")),
                (42..63, url("https://example.com/y")),
            ]
        );
    }

    #[test]
    fn a_wrapped_url_is_recognised_across_pieces() {
        // 一条 URL 被折成两片：拼回之后认得出，两个候选各自按列落在自己那一片上。
        let mut rows = vec![
            Line::from("见 https://a.exa"),
            Line::from("mple/very/long。"),
        ];
        let found = mark(&mut rows, &[false, true]);
        assert_eq!(
            found[0],
            vec![Hotspot {
                columns: 3..16,
                target: url("https://a.example/very/long"),
            }]
        );
        assert_eq!(
            found[1],
            vec![Hotspot {
                columns: 0..14,
                target: url("https://a.example/very/long"),
            }]
        );
    }

    #[test]
    fn marking_underlines_exactly_the_hot_columns() {
        let mut rows = vec![Line::from("见 x.html 与 https://a.example/b")];
        let found = mark(&mut rows, &[false]);
        assert_eq!(found[0].len(), 2);
        assert_eq!(row_text(&rows[0]), "见 x.html 与 https://a.example/b");
        // 按**列**写出这一行：`^` 是这一格带下划线。`见` 与 `与` 各占两格，所以用
        // 显示宽度铺，而不是按字符数铺。
        let mut drawn = String::new();
        for span in &rows[0].spans {
            let mark = if span.style.add_modifier.contains(Modifier::UNDERLINED) {
                '^'
            } else {
                '.'
            };
            for ch in span.content.chars() {
                for _ in 0..char_columns(ch) {
                    drawn.push(mark);
                }
            }
        }
        assert_eq!(
            drawn,
            format!("...^^^^^^....{}", "^".repeat(19)),
            "只有 x.html 与那条 URL 该带下划线"
        );
    }

    #[test]
    fn a_line_without_candidates_grows_no_spans() {
        let mut rows = vec![Line::from("已经写好了")];
        let found = mark(&mut rows, &[false]);
        assert!(found[0].is_empty());
        assert_eq!(rows[0].spans.len(), 1);
    }

    #[test]
    fn a_url_resolves_to_itself() {
        let cwd = std::env::current_dir().expect("测试的工作目录");
        assert_eq!(
            url("https://example.com/x").resolve(&cwd),
            Some("https://example.com/x".to_owned())
        );
    }

    #[test]
    fn a_path_resolves_only_inside_the_workspace_and_only_when_it_exists() {
        let cwd = std::env::current_dir().expect("测试的工作目录");
        let root = cwd.canonicalize().expect("仓库根");
        assert_eq!(
            path("Cargo.toml").resolve(&cwd),
            Some(root.join("Cargo.toml").to_string_lossy().into_owned()),
            "真存在、又在区内：交回绝对路径"
        );
        assert_eq!(
            path("没有这个文件.html").resolve(&cwd),
            None,
            "不存在就不开"
        );
        assert_eq!(
            path("/etc/hostname").resolve(&cwd),
            None,
            "存在但在区外：与区外读那条地板同一个立场"
        );
        assert_eq!(
            path("target/../Cargo.toml").resolve(&cwd),
            Some(root.join("Cargo.toml").to_string_lossy().into_owned()),
            "`..` 先解开再判区内外"
        );
    }
}
