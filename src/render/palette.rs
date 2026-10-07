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

/// 草稿里一条**能兑现**的 `/` 命令的颜色。
///
/// 它与 [`INJECTED`] 撞值、[`TOKEN_REFERENCE`] 与 [`ACCENT`] 撞值，都是**刻意的**：两个记号
/// 是草稿里自己的一对（[`input-tokens` §4](../../../.scratch/input-tokens/spec.md) 把它们钉成
/// **两个不同的**颜色），值在这里、判据仍在那一份 spec 里。
pub const TOKEN_COMMAND: Color = Color::LightBlue;

/// 草稿里一个**能兑现**的 `@` 引用的颜色。
pub const TOKEN_REFERENCE: Color = Color::LightMagenta;

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

/// 提示符的色相走色环有多快，单位是每秒圈数。维护者的脚本每 1/60 秒走 0.005，这三个常量就
/// 是那个：脚本的速率换成秒，这样换个帧长，样子还留得住。
pub const PROMPT_HUE_PER_SECOND: f64 = 0.3;

/// 提示符呼吸的中心饱和度、它摆多远，以及摆多快（每秒弧度：脚本里的每 1/60 秒 0.05）。
pub const PROMPT_SATURATION: f64 = 0.55;
pub const PROMPT_SATURATION_BREATH: f64 = 0.2;
pub const PROMPT_BREATH_PER_SECOND: f64 = 3.0;

/// 提示符保持的明度（亮度）：在暗色主题上够亮、读得清，在亮色主题上够暗、不刺眼。
pub const PROMPT_VALUE: f64 = 0.85;

/// 提示符的颜色，取 `seconds` 那一刻。
///
/// 它是 24 位色，这个界面里**唯一**不是 16 色 ANSI 码的地方 —— 提示符两边都坐在终端自己的
/// 背景上，而一个必须在十六个名字里挑一个的色相会看得见台阶。整个函数是时间的纯函数，所以
/// 测试不必有终端就能说出某一刻长什么样。
///
/// 它不属于上面任何一档：谁都不许拿它当自己的颜色（§2 的「提示符专色」）。
pub fn prompt_colour(seconds: f64) -> Color {
    let hue = (seconds * PROMPT_HUE_PER_SECOND) % 1.0;
    let saturation =
        PROMPT_SATURATION + PROMPT_SATURATION_BREATH * (seconds * PROMPT_BREATH_PER_SECOND).sin();
    let (red, green, blue) = hsv_to_rgb(hue, saturation, PROMPT_VALUE);
    Color::Rgb(red, green, blue)
}

/// HSV 转 RGB，按它来源那个脚本里 `colorsys.hsv_to_rgb` 的算法 —— 包括截到 8 位，这样同一刻
/// 给出的颜色与脚本当年给出的一样。
fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> (u8, u8, u8) {
    // 色环的每六分之一是一个色相升、下一个色相降；`sector` 是第几个六分之一，`offset` 是在
    // 里面走了多远。
    let scaled = (hue.fract() * 6.0).rem_euclid(6.0);
    let sector = scaled.floor();
    let offset = scaled - sector;
    let (rising, falling) = (
        value * (1.0 - saturation * (1.0 - offset)),
        value * (1.0 - saturation * offset),
    );
    let (red, green, blue) = match sector as u32 {
        0 => (value, rising, value * (1.0 - saturation)),
        1 => (falling, value, value * (1.0 - saturation)),
        2 => (value * (1.0 - saturation), value, rising),
        3 => (value * (1.0 - saturation), falling, value),
        4 => (rising, value * (1.0 - saturation), value),
        _ => (value, value * (1.0 - saturation), falling),
    };
    let byte = |channel: f64| (channel * 255.0).clamp(0.0, 255.0) as u8;
    (byte(red), byte(green), byte(blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 色环，钉在每个实现都同意的那六个点上 —— 分区算术里一个舍入错误最先显形的那几个角。
    #[test]
    fn hsv_to_rgb_matches_the_shortcut_table() {
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), (0, 0, 255));
        // 色环闭合：色相 1 就是色相 0，而越过它的色相会回绕而不是 panic。
        assert_eq!(hsv_to_rgb(1.0, 0.4, 0.8), hsv_to_rgb(0.0, 0.4, 0.8));
        assert_eq!(hsv_to_rgb(2.25, 0.4, 0.8), hsv_to_rgb(0.25, 0.4, 0.8));
    }
}
