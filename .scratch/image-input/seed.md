# 种子材料：把图片交给模型

> **这不是 spec，也不是票。** 它是 2026-10-02 在一轮 `/ask-matt` 里记下的一句话意向：
> 「支持添加图片」。
> 还没被访谈、也没有票；想推进时走 `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
> **写就于 2026-10-02**；下面《现状》一节核实于同一天。

## 它要什么

给人一条把**图片**弄进这次会话的路。这一句至少能读成三件事，推进前得先说清是哪一件 ——
它们牵动的面几乎不重叠：

1. **输入侧**：用户把一张图**交给模型**（粘贴剪贴板里的图、给一个路径、拖进终端）。要的是
   多模态消息：图片进 provider 的请求，模型看得见它。
2. **显示侧**：终端里**真的画出**图片 —— kitty graphics / iTerm2 inline images / sixel。
3. **引用侧**：只把图片当成一个可提及的**路径/资源**（像 `@file` 那样），内容不进请求。

## 现状（2026-10-02 核实）

- **渲染侧已经有半条腿**：Markdown 的 `![alt](url)` 早在 `markdown-render` 那一轮就落成了
  `[图片] alt (url)`（[票 06](../markdown-render/issues/06-images.md)、
  [`wording::IMAGE_PLACEHOLDER`](../../src/render/wording.rs)、`src/render/markdown.rs` 的
  `close_image`）。那条票的《具体行为》第 4 条明确写了**不做真图渲染**，理由是「那需要终端图像
  协议（kitty graphics / iTerm2 inline images / sixel）」，并把它留给「单独一轮」——
  所以**显示侧这条线有前人明确留的口子**，不是新起炉灶。
- **输入侧什么都没有**：TUI 只认**文本**粘贴（括号粘贴 + 过大粘贴先问一句，
  `src/render/tui.rs` 的 `EnableBracketedPaste` 与那个阈值），而 `ConsoleRequest`、
  `RenderEvent`、消息 schema 里没有「附件」这一路。
- **请求的形状是纯文本**：`src/provider/mod.rs` 的三处 `content: String`，以及事件流里的
  `MessageCompleted.content: String`（[`src/events.rs:334`](../../src/events.rs)）。多模态
  要动的是这一层，也就是「参数即真相、零 schema 改动」那条设计的正面（与 ADR 0009 同类的判断）。
- **调研材料**：[`docs/research/notes/claude-code-amp.md`](../../docs/research/notes/claude-code-amp.md)
  里只出现过 kitty / iTerm2 / tmux 的**键盘**协议，**没有**终端图像协议的调研；那条路要新读一手
  材料（三种协议的降级矩阵、tmux 与 SSH 里各剩什么）。
- plain 逐行读 stdin、headless 没有键盘：这两条前端在「图片怎么进来」上必然和 TUI 不同。

## 待谈的分叉

1. 上面三条解读先做哪一条（或哪两条一起）—— 它们不是同一件事，别在一张票里混着做。
2. 图片**怎么进来**：括号粘贴只递字节、收不到图；要么读剪贴板（`wl-paste` / `xclip` 还是
   OSC 52？），要么只收路径，要么走终端的图片协议。
3. 图片**去哪里**：多模态 content block 会动 provider 请求与事件流 schema，代价要按
   「老流还读不读得回来」算清楚。
4. **显示侧要不要一起**：转录里画缩略图要挑终端协议，三套各自降级，tmux / SSH / plain /
   headless 又各剩一种行为。
5. **预算**：图片算 token 吗？`context` 的预算、截断与丢弃（`goal-loop` 的翻页）怎么处理一张
   比整段对话还大的 base64。
6. **权限**：粘贴一张截图算不算**区外读**？它走不走 `outside_read` 那条旋钮，还是要单独一条。
7. **落盘与重放**：图片进不进会话文件 —— 流里放 base64（文件会炸），还是落一份拷贝进
   `outputs/` 再从流里引用（`--continue` 才重放得出来）。
