//! 输入框里的**记号**：`/` 命令与 `@` 引用共用的那一套推导（`input-tokens` 票 02）。
//!
//! 一处纯函数把它从草稿文本里推出来，于是菜单、补全与提交解析三处看的是同一份判断 ——
//! 界面不会对循环说一套、自己又做另一套。边界规则**按前缀**参数化，不是两套代码：
//!
//! | 前缀 | 边界 |
//! | --- | --- |
//! | `@` | 草稿任意位置的记号开头，**前面是空白或草稿起点**；`src/@foo` 不触发 |
//! | `/` | 草稿**任意行、任意位置**的记号开头 |
//!
//! `@` 比 `/` 严，因为一个字面 `@` 在路径与邮箱里都很常见；`/` 松，是因为一条命令的判据
//! 本来就在命令表里，而位置规则不该与 `@` 各写一套（[ADR 0012] 记着那次取舍）。
//!
//! 名字（[`Token::query`]）延到第一个空白或草稿末尾，所以 `/ask-matt 优化这个` 的记号是
//! `ask-matt`，而 `看 /tmp/x` 的记号是 `tmp/x` —— 后者是不是一条命令，由调用方拿命令表去
//! 判。**推导本身不认识命令表，也不认识文件索引**：那是 `TuiState` 的事。
//!
//! [ADR 0012]: ../../../docs/adr/0012-input-tokens-are-atomic.md

/// 草稿里的一个记号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 开头的那个字符：`/` 或 `@`。
    pub prefix: char,
    /// 前缀字符的**字符**下标。
    pub start: usize,
    /// 名字刚过一格的字符下标（第一个空白或草稿末尾）。
    pub end: usize,
    /// 前缀之后、一直打到名字结束为止的东西。
    pub query: String,
}

/// 草稿里全部的记号，按位置从左到右。
pub fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < chars.len() {
        match token_here(&chars, at) {
            // 一个记号里面的字符不再另开记号：`@src/a.rs` 里的 `/` 属于那个 `@`。
            Some(token) => {
                at = token.end;
                out.push(token);
            }
            None => at += 1,
        }
    }
    out
}

/// 光标所在的那个记号，如果它落在某个记号里面的话。
pub fn token_at(text: &str, cursor: usize) -> Option<Token> {
    tokens(text)
        .into_iter()
        .find(|token| token.start <= cursor && cursor <= token.end)
}

/// 位置 `at` 上是不是一个记号的开头；是就把它整个读出来。
fn token_here(chars: &[char], at: usize) -> Option<Token> {
    let prefix = *chars.get(at)?;
    let opens = match prefix {
        '@' => at == 0 || chars[at - 1].is_whitespace(),
        '/' => true,
        _ => return None,
    };
    if !opens {
        return None;
    }
    // 名字一直延到出现的第一个空白 —— 换行结束这一行，空格开始任务 —— 或者到草稿末尾。
    let end = chars[at + 1..]
        .iter()
        .position(|ch| ch.is_whitespace())
        .map(|offset| at + 1 + offset)
        .unwrap_or(chars.len());
    Some(Token {
        prefix,
        start: at,
        end,
        query: chars[at + 1..end].iter().collect(),
    })
}
