//! 语义色板：颜色值集中一处，绘制代码只引用语义名
//! （`.scratch/tui-visual-language/spec.md` 实现决定 §1–§5）。
//!
//! 分两节，**域间允许撞值**（§3）：**界面域**回答「要不要注意 / 有没有被选中」，
//! **内容域**（markdown 与语法高亮）回答「这是什么」。代码块内部上下文明确，`Green` 既是
//! 「成功」又是「字符串」不会被读混 —— 这是本 spec 唯一的「两套规则并存」，代价显式接受。
//! 两节都在本文件里；[`super::markdown`] 与 [`super::highlight`] 反过来从这里取色。
//!
//! 命名是**扁平的英文大写**：已经在 `palette::` 里了，再加族前缀是冗余（§1）。

use ratatui::style::{Color, Style};

use super::severity::Severity;

// --- 界面域 -----------------------------------------------------------------

/// 正文档（终端自己的默认前景）：用户与助手正文、markdown 正文、问卷题面与选项、详情正文、
/// 菜单文字、输入区草稿、面板数值、`Notice` 那一类、分歧行（§6）。
pub const PLAIN: Color = Color::Reset;

/// 静音：**过程** —— 「系统在说它自己干了什么」。全屏只有这一档静音；再退一档只能靠**结构**
/// （缩进、线型、留白），`DIM` 不参与层级（§6、§7）。
pub const MUTED: Color = Color::DarkGray;

/// 框架线：只做装饰线 —— 外壳的竖线、页签条、浮层边框与空角、详情小节线。调研量过它对白底
/// 8.86:1、对 Alacritty 默认底 2.00:1，所以它**不承载信息**（§2）。
pub const CHROME: Color = Color::Rgb(0x4a, 0x4a, 0x4a);

/// 焦点：**被系统认出来的 / 当前聚焦的** —— 常驻选中（+ `BOLD`）或临时光标（+ `REVERSED`），
/// 以及新内容指示器与输入记号（§6、§8）。
pub const ACCENT: Color = Color::LightMagenta;

/// 警告：诊断、hook 反馈、`Severity::Warn`（§6）。
pub const WARN: Color = Color::Yellow;

/// 错误：工具失败、会话错误、`Severity::Bad`（§6）。要连 `BOLD` 一起用时由调用方加，色板
/// 不把字重烤进颜色。
pub const BAD: Color = Color::Red;

/// 上下文注入行的专色 —— 本 effort 里**唯一为某个表面保留的颜色**：轨迹页的三类前缀
/// （注入 / 用户 / 助手）要分得开（§2、用户故事 8）。
pub const INJECTED: Color = Color::LightBlue;

/// 用户消息那块**气泡**的底色（`.scratch/trace-tab/spec.md` §2 的补记）。
///
/// 它不是一个前景色：气泡靠一块等宽的底色与它右缘的留白说「这是我说的」，而不用边框字符 ——
/// 字符会被拖选复制带走，底色不会。值取得很暗，于是它在一块本来就暗的终端上是一层哑光，
/// 而不是一个抢注意力的色块。
pub const BUBBLE: Color = Color::Rgb(0x33, 0x33, 0x33);

/// **当前视口在时间轴上占的那一段**的底色（`.scratch/trace-ledger/spec.md` §12、票 22）。
///
/// 底色在界面上是**第四个通道**（前三个是前景、字重、反显），而这一档是它第一次用来回答
/// 「哪一段时间」。它只铺在刻度行上，与票 23 的**命中底色**（铺在两条泳道行上）语义不同、
/// 行也不同，所以两者不必互相区分到不可分辨 —— 但必须分辨得开：一个说「你在看哪儿」，
/// 一个说「哪儿有命中」。值取得比 [`BUBBLE`] 更暗、更偏蓝，于是它读起来是一层哑光，
/// 而不是一个抢注意力的色块，也不会与那块中性灰的气泡混成同一件事。
pub const VIEWPORT: Color = Color::Rgb(0x1f, 0x2d, 0x3d);

/// 草稿里一条**能兑现**的 `/` 命令的颜色。
///
/// 它与 [`INJECTED`] 撞值、[`TOKEN_REFERENCE`] 与 [`ACCENT`] 撞值，都是**刻意的**：两个记号
/// 是草稿里自己的一对（[`input-tokens` §4](../../../.scratch/input-tokens/spec.md) 把它们钉成
/// **两个不同的**颜色），值在这里、判据仍在那一份 spec 里。
pub const TOKEN_COMMAND: Color = Color::LightBlue;

/// 草稿里一个**能兑现**的 `@` 引用的颜色。
pub const TOKEN_REFERENCE: Color = Color::LightMagenta;

/// `/` 菜单里一条**技能**的名字（`.scratch/tui-feedback/spec.md` §11）。
///
/// 与 [`TOKEN_REFERENCE`] 撞值是刻意的：两者都是「用户自己装进来的东西」那一档品红。命令
/// 那一档**不在这里**——它直接用 [`TOKEN_COMMAND`]，于是同一个名字在草稿里与菜单里长得一样。
pub const MENU_SKILL: Color = Color::LightMagenta;

/// `/` 菜单里一条 **MCP 提示词模板**的名字。三类来源里它最弱（要一台 server 才可能出现），
/// 于是给最暗的一档青。
pub const MENU_TEMPLATE: Color = Color::Cyan;

// --- 冻结的角色五色 ----------------------------------------------------------
//
// `tui-ux` 冻结项 9，本 effort 一个字不动（§2、用户故事 9）。它们只回答「谁在说」。

/// 讨论者的两色，按名册槽位发出去。
pub const DEBATERS: [Color; 2] = [Color::LightCyan, Color::LightMagenta];

/// 一个执行者的回合。
pub const EXECUTOR: Color = Color::LightYellow;

/// 用户自己说的话。
pub const USER: Color = Color::LightGreen;

/// `System` 消息。
pub const SYSTEM: Color = Color::Gray;

// --- 品牌标记 ----------------------------------------------------------------
//
// 宽档左栏那块「衡」字标记的品红：两档基线，字亮、注暗；外加扫光峰值那一档白。它是**品牌
// 标记**，不参与语义体系 —— 但颜色值仍住在色板里，于是 `grep Color::` 的结果是干净的
// （`.scratch/tui-visual-language/issues/07` 决定 4）。

/// 亮的那一档：块字本身那二十二行。
pub const MARK_BRIGHT: Color = Color::LightMagenta;
/// 暗的那一档：字形上面标着拼音的那一行。
pub const MARK_DIM: Color = Color::Magenta;

/// 扫光的峰值：一次运行里那束从右下走到左上的反光，最亮的一格用的是白。
///
/// 洋红族里 [`MARK_BRIGHT`] 已经是最亮的一档，没有更亮的品红可用了 —— 要读起来像镜面反光，
/// 峰值只能**过曝**成白。它只在一帧里光带核心的那几格上出现，静止的标记从来不用这个颜色
/// （`.scratch/mark-sweep/spec.md` §2）。
pub const MARK_LIGHT: Color = Color::White;

// --- 内容域（markdown 与语法高亮） -------------------------------------------
//
// 分家规则见本文件开头（§3）；一句话记法：**这一节里没有 `WARN` / `BAD` / `ACCENT`** ——
// 内容域不预警、不表示选中。所以这里的值即使与界面域某一条相同，也是**刻意的撞值**，不是
// 漏改的引用。

/// 内容域的退后一档：行内代码、引用条、链接目标、分隔线、代码块的语言名、表格网格，以及
/// 语法里的注释与标点。值与界面域的 [`MUTED`] 相同是刻意的（§3）。
pub const CODE_QUIET: Color = Color::DarkGray;

/// markdown 标题，配 [`ratatui::style::Modifier::BOLD`]。
pub const CODE_HEADING: Color = Color::Cyan;

/// 语法关键字。
pub const CODE_KEYWORD: Color = Color::Magenta;

/// 语法函数名。
pub const CODE_FUNCTION: Color = Color::Blue;

/// 语法类型名。
pub const CODE_TYPE: Color = Color::Cyan;

/// 语法字符串。
pub const CODE_STRING: Color = Color::Green;

/// 语法数字与常量 —— `LightYellow` 而不是界面域的 `WARN`（`Yellow`）：代码里的数字不是警告。
pub const CODE_NUMBER: Color = Color::LightYellow;

// --- 补丁的三档（`.scratch/diff-page/spec.md` §6） ---------------------------
//
// 它们住进内容域而不是界面域，因为回答的是「这是什么」：这一行在补丁里算新增、删除，还是
// 一个 hunk 的头。**新增与删除给的是背景** —— 这样它们与语法层的前景是叠加的，而不是互相
// 打架（`highlight` 模块头那两层分家的同一个理由）。两个值取得很暗：一块背景色要读得出
// 「这一行不一样」，又不能把代码本身压下去。

/// 补丁里新增的一行。本文件里**仅有**的两个 24 位色之一：要暗到刚好看得出来，16 色那几档
/// 没有一个合适的绿。
pub const DIFF_ADDED: Color = Color::Rgb(0, 40, 0);

/// 补丁里删除的一行。另一个 24 位色，理由同上。
pub const DIFF_REMOVED: Color = Color::Rgb(50, 0, 0);

/// hunk 头（`@@ … @@`）。它是**前景**，不配背景 —— 它不是一行代码，是一条分隔；配粗体一起
/// 用，于是一眼分得开「这是头」与「这是内容」。
pub const DIFF_HUNK: Color = Color::Cyan;

// --- 严重度 -----------------------------------------------------------------

/// 一档严重度的 TUI 样式。
///
/// 语义归属仍只有 [`Severity::of`] 一处；plain 的 [`Severity::ansi`] 输出一个字节不动
/// （§4）。`Good` 与 `Note` 归静音 —— **正常完成不再抢注意力**，只有出问题时屏幕才亮。
pub fn style(severity: Severity) -> Style {
    match severity {
        Severity::Good | Severity::Note => Style::default().fg(MUTED),
        Severity::Warn => Style::default().fg(WARN),
        Severity::Bad => Style::default().fg(BAD),
    }
}

// --- 提示符的专色 ------------------------------------------------------------
//
// 这里曾经住着提示符 `❱` 的色相与呼吸（`PROMPT_HUE_PER_SECOND` 那一组常量、`prompt_colour`
// 与它下面那个 `hsv_to_rgb`）—— 这个界面里**唯一**一处 24 位色。2026-10-08 维护者点掉了输入框
// 那个字形，色相于是没有载体，整套随它一起退场（`.scratch/ui-trim/spec.md`；来源是
// `.scratch/tui-input-pulse/spec.md` §2b）。今天这个色板里除补丁那两个背景（[`DIFF_ADDED`] /
// [`DIFF_REMOVED`] —— 它们要暗得恰好，16 色里没有合适的档）之外，都是 16 色 ANSI 码。

#[cfg(test)]
mod tests {
    use super::*;

    /// `/` 菜单的三类来源各一色、互不相同 —— 「看得出是三类」就是这一条
    /// （`.scratch/tui-feedback/spec.md` §11）。撞成同一个值时，配色那一层就白改了。
    #[test]
    fn the_menu_kinds_are_three_distinct_colours() {
        let kinds = [TOKEN_COMMAND, MENU_SKILL, MENU_TEMPLATE];
        for (index, left) in kinds.iter().enumerate() {
            for right in &kinds[index + 1..] {
                assert_ne!(left, right, "菜单里两类来源撞了同一个颜色");
            }
        }
    }
}
