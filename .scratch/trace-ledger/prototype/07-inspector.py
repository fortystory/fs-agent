#!/usr/bin/env python3
"""一次性原型：检视器分面的形态 —— 字符格里怎么表达「这个覆盖层里有几个面、现在在第几个」。

票：`.scratch/trace-ledger/issues/07-prototype-inspector.md`
跑法：
    python3 .scratch/trace-ledger/prototype/07-inspector.py > .scratch/trace-ledger/prototype/07-inspector.txt

**throwaway**：不接渲染器，只把候选画出来给眼睛判一次。

尺寸全部照今天的实现（`src/render/layout.rs`）：

  · 屏幕 100×30 → `detail_width()` = min(100 − 2×`MODAL_MARGIN`(4), `DETAIL_MAX_WIDTH`(135)) = **92**
  · 覆盖层高 = 30 − (`BORDER_COLUMNS`(2) + `DETAIL_MARGIN_ROWS`(2)) = **26**，框内 24
  · `detail_padding`：框内够宽够高 → 上下左右各让 1 列 → 文字区 **88 列 × 22 行**
  · 今天的三段：标题 1 行（被点那一行自己的文字，`ACCENT+BOLD`）+ 主体 **20 行** + 页脚 1 行
    （`wording::detail_footer` = `↕ {pos}/{total} · esc 关闭`）
  · 边框 `chrome_block()`：横 `┄`、竖 `┆`、四角是空格

主体高度固定 20 行（真终端那么多），帧里没内容的地方留空 —— 这样「滚到哪」是有意义的，
滚动位置的代价（帧 2b、帧 1b）才看得见。

数据是**造的一条会话**，与 `prototype/03-ledger-skeleton.txt` / `05-timeline.txt` 那场对齐：
主角是 09:12:11→09:12:12 的那次 `grep`（参数 JSON 有六个键、输出落盘 138 行、模型那次调用
2.0 s / 首 token 1.4 s）。每一个量都只取数据面证过的来源（`issues/01-research-data-surface.md`
的 `Usage` 五个桶与 [ADR 0020](../../../docs/adr/0020-first-token-time-rides-the-completion.md)
的 `first_token_ms`）。**没有的就不画**：错误 code、历史 system prompt 的差异都没有位置。

帧里的颜色一律用文字说明（沿用 05 票的做法）：行选中 `ACCENT+BOLD`、命中高亮用**底色**、
失败 `BAD`、其余 `MUTED` / `CHROME`。

**这一票只回答形状**。哪些面、默认落在哪个面、键位是什么、原文面给什么，都在
[检视器的面与内容](08-grilling-inspector.md) 拍；帧里出现的面名是**假设**，让读者有东西可批。
"""

import unicodedata

# --- 尺寸（全部来自 src/render/layout.rs 的那几个常量） ----------------------------
SCREEN_W, SCREEN_H = 100, 30
MODAL_MARGIN = 4
DETAIL_MAX_WIDTH = 135
DETAIL_W = min(SCREEN_W - 2 * MODAL_MARGIN, DETAIL_MAX_WIDTH)  # 92
INNER_W = DETAIL_W - 2  # 90：扣掉两列边框
TEXT_W = INNER_W - 2  # 88：再扣掉左右各一列内边距
DETAIL_H = SCREEN_H - 4  # 26：扣掉边框两行 + 上下各一行转录
INNER_H = DETAIL_H - 2  # 24
TEXT_H = INNER_H - 2  # 22：再扣掉上下各一行内边距
TITLE_H, FOOTER_H = 1, 1
BODY_H = TEXT_H - TITLE_H - FOOTER_H  # 20

# --- 这一趟里主角那一次调用的数据 -------------------------------------------------
CALL_STAMP = "09:12:12"
TITLE = f"{CALL_STAMP} [kimi] ▸ 调用 grep TRUNCATED_MARKER"

ARGS_PRETTY = """{
  "pattern": "TRUNCATED_MARKER",
  "path": ".scratch",
  "output_mode": "content",
  "head_limit": 20,
  "line_numbers": true,
  "multiline": false
}"""

# 落盘全文 138 行，详情读的是它；帧里给开头（top = 0）与中段（top = 600）两处。
OUTPUT_HEAD = [
    '.scratch/grep-tool/issues/07-long-output-truncation.md:12:  const TRUNCATED_MARKER = "… （省略 128 行）…";',
    ".scratch/grep-tool/issues/07-long-output-truncation.md:31: fn take(head: usize, tail: usize, lines: &[String]) -> Vec<String> {",
    ".scratch/grep-tool/issues/07-long-output-truncation.md:44: // 头尾各留若干行，中间写一行标记，不静默丢。",
    ".scratch/grep-tool/spec.md:44: 只留头尾，中间写一行 `TRUNCATED_MARKER`。",
    ".scratch/grep-tool/spec.md:51: 计数要给的是**读的人看到的行数**，不是原结果的行数。",
]
OUTPUT_MID = [
    ".scratch/trace-ledger/prototype/05-timeline.py:29: TRACE_W = 79  # 轨迹内容宽",
    ".scratch/trace-ledger/prototype/05-timeline.py:36: def w(text: str) -> int:",
    ".scratch/trace-ledger/prototype/05-timeline.py:41:     return text + \" \" * max(0, width - w(text))",
    ".scratch/trace-ledger/prototype/05-timeline.py:44: def stamp(sec: float) -> str:",
    "… （这一份落盘全文共 138 行，这里是第 601 行起） …",
]

# 帧 4 的两份读法：同一条消息，渲染的那一档与原文的那一档。
MESSAGE_STAMP = "09:12:52"
MESSAGE_TITLE = f"{MESSAGE_STAMP} [kimi] ▸ 头尾都留住了，省掉多少也写清楚了…"
MESSAGE_RENDERED = (
    "头尾都留住了，省掉多少也写清楚了。中间那行标记现在带上了原结果的行数，读的人不会被 "
    "`head_limit` 骗——头尾各留多少、中间省了多少，一眼能对上。\n"
    "剩下那半边是失败：那次调用只有一个字符串，code 拿不到，所以详情里只能说「失败」。"
)
MESSAGE_RAW = """{
  "role": "assistant",
  "speaker": "kimi",
  "at": "2026-10-09T09:12:52.184+08:00",
  "text": "头尾都留住了，省掉多少也写清楚了。中间那行标记现在带上了原结果的行数，读的人不会被 head_limit 骗——头尾各留多少、中间省了多少，一眼能对上。\\n剩下那半边是失败：那次调用只有一个字符串，code 拿不到，所以详情里只能说「失败」。"
}"""

# 这次调用所属的那次模型调用（kimi 迭代 2：09:12:11 → 09:12:13，2.0 s）
MODEL_SECONDS = 2.0
FIRST_TOKEN_S = 1.4
GEN_SECONDS = round(MODEL_SECONDS - FIRST_TOKEN_S, 1)  # 0.6，浮点尾巴不要漏进帧里
OUT_TOKENS = 204

# 工具的墙钟：0.4 s；其中等审批 0.2 s（`PermissionAsked.at` → `PermissionDecided.at`）
TOOL_SECONDS = 0.4
APPROVAL_SECONDS = 0.2

# 用量（`events::Usage` 的五个桶：`cached` + `miss` = `input`，`reasoning` 含在 `output` 里）
INPUT, CACHED, MISS, OUTPUT = 18310, 15000, 3310, 204
TOTAL_INPUT, TOTAL_CACHED, TOTAL_MISS, TOTAL_OUTPUT = 36544, 29880, 6664, 516


def w(text: str) -> int:
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def pad(text: str, width: int = TEXT_W) -> str:
    return text + " " * max(0, width - w(text))


def clip(text: str, width: int = TEXT_W) -> str:
    """超宽收尾成 `…`：正文按文字区宽排版，多的部分被框裁掉（与实现一致）。"""
    return text if w(text) <= width else text[: max(0, width - 1)] + "…"


def wrapped(text: str, width: int = TEXT_W, indent: str = "") -> list[str]:
    """按显示列折行（与 `folded_text` 同序：先按 `\\n` 切逻辑行，再各自折行）。"""
    out: list[str] = []
    budget = width - w(indent)
    for para in text.split("\n"):
        current, used = "", 0
        for ch in para:
            cw = 2 if unicodedata.east_asian_width(ch) in "WF" else 1
            if used + cw > budget:
                out.append(indent + current)
                current, used = "", 0
            current += ch
            used += cw
        out.append(indent + current)
    return out


def section(name: str) -> str:
    """面内的小节标题：今天详情里的既有画法（`wording::detail_section` = `── 名称 ──`，MUTED）。"""
    return f"── {name} ──"


def meter(value: int, whole: int, cells: int = 20) -> str:
    """用量那一条占比条：沿用面板那套 `▓`（满）与 `░`（空），上限 20 格。"""
    filled = 0 if whole <= 0 else min(cells, max(1, round(value / whole * cells)))
    return "▓" * filled + "░" * (cells - filled)


# ---------------------------------------------------------------------------
# 覆盖层的骨架：顶框 / 标题 / 可选的面标签条 / 主体 / 页脚 / 底框
# ---------------------------------------------------------------------------


def overlay(rows: list[str], *, title: str = TITLE, bar: list[str] | None = None,
            footer: str = "↕ 1/1 · esc 关闭", backdrop: list[str] | None = None) -> list[str]:
    """一块详情覆盖层。`bar` 是面标签条（候选一），它从主体里借走一行。"""
    out: list[str] = []
    if backdrop:
        out.extend(backdrop)
    out.append(" " + "┄" * INNER_W + " ")  # chrome_block：横线是 ┄，四角是空格
    body_rows = BODY_H - (len(bar) if bar else 0)
    out.append("┆" + pad(clip(title), INNER_W) + "┆")
    if bar:
        out.extend("┆" + pad(line, INNER_W) + "┆" for line in bar)
    for index in range(body_rows):
        out.append("┆" + pad(rows[index] if index < len(rows) else "", INNER_W) + "┆")
    out.append("┆" + pad(clip(footer), INNER_W) + "┆")
    out.append(" " + "┄" * INNER_W + " ")
    # 每一行都得是 `DETAIL_W` 列 —— 字符格里一处错位，「88 列正文」这个前提就悄悄变成假的。
    for line in out:
        assert w(line) == DETAIL_W, f"宽度 {w(line)} ≠ {DETAIL_W}：{line!r}"
    return out


def bar(labels: list[str], active: int, *, rule: str = "┄") -> list[str]:
    """候选一的面标签条：照主列页签条那套画法（`draw_label_bar` —— 标签 + `┆` + 线填满）。

    选中那个是 `ACCENT+BOLD`；这里用 `▐…▌` 把那一格圈出来，只为在纯文本里能看出位置。
    """
    out = ""
    for index, label in enumerate(labels):
        out += ("▐" if index == active else " ") + label + ("▌" if index == active else " ")
        out += "┆"
    return [out[:-1] + rule * max(0, TEXT_W - w(out) + 1)]


FACES = ["参数", "输出", "用量", "计时"]


# ---------------------------------------------------------------------------
# 帧
# ---------------------------------------------------------------------------


def frame(name: str, note: str, cost: str, lines: list[str]) -> list[str]:
    return [
        "=" * 92,
        f"帧 {name}",
        note,
        f"代价：{cost}",
        "-" * 92,
        *lines,
        "",
    ]


def trace_line_above() -> list[str]:
    """覆盖层上方留的那一行转录（`DETAIL_MARGIN_ROWS`），也就是[账本的两级结构]定下的行尾读数。"""
    return [
        " " * 92,
        pad(f"{CALL_STAMP} [kimi] ▸ 调用 grep TRUNCATED_MARKER · 0.4 s   in=18310 out=204", DETAIL_W),
        " " * 92,
    ]


def frame0() -> list[str]:
    rows = [section("参数")] + wrapped(ARGS_PRETTY)
    # 落盘全文与流上正文走同一条 `folded_text`：按 88 列折行，续行**不带任何标记**
    rows += [section("输出")] + wrapped("\n".join(OUTPUT_HEAD)) + ["已截断"]
    return frame(
        "0 · 今天的样子（基线）",
        "一个面 = 一块正文，参数与输出是**两节**而不是两面：只有一块正文可滚，页脚只报行号"
        "（`↕ 3/47 · esc 关闭`，那一对数是这块正文的行号）。标题是被点那一行自己的文字。"
        "帧里滚到第 3 行 —— 参数那节已经读完了。",
        "零改动可比的地面。要分面，先得回答「那两块节标题去哪」，下面每一帧都在回答它。",
        overlay(rows[2:2 + BODY_H], footer="↕ 3/47 · esc 关闭"),
    )


def frame1a() -> list[str]:
    return frame(
        "1a · 候选一：顶部一条面标签条（当前面 = 参数）",
        "标签条照主列页签条的画法：标签 + `┆` + 线填满，选中那个 `ACCENT+BOLD`，每个标签在画出时"
        "记一个命中矩形（点它切面；**键位留给 08 票** —— 轨迹页今天不是键盘归属的一层）。"
        "面里的小节标题**全部收掉**，它在标签条上已经有了。",
        "标签条从主体借走一行（20 → 19）；画法直接复用 `draw_label_bar`。面名挤不下时的降级见帧 1c。"
        "页脚那对数分面后**只能是当前面**的行数（这一条留给 08 票第二问）。",
        overlay(wrapped(ARGS_PRETTY), bar=bar(FACES, 0), footer="↕ 1/9 · esc 关闭"),
    )


def frame1b() -> list[str]:
    return frame(
        "1b · 候选一的另一个取舍：小节标题留着",
        "同一块标签条，面里**仍**保留 `── 参数 ──`。看着是重复，好处是长正文滚到中段时屏幕上还认得出"
        "自己在哪一面 —— 标签条那一行是固定的、不会滚走，可**读者滚到中段时眼睛会离开它**"
        "（帧 2b 就是这件事的证据）。",
        "多一行、一处重复，换「滚到哪都认得」。留不留就是 08 票第一问的一个岔口。",
        overlay(
            [section("参数")] + wrapped(ARGS_PRETTY),
            bar=bar(FACES, 0),
            footer="↕ 2/10 · esc 关闭",
        ),
    )


def frame1c() -> list[str]:
    rows = [
        "工具          grep",
        "参数模式      content",
        "路径          .scratch",
        "头尾上限      20",
        "行号          是",
        "多行          否",
        "pattern       TRUNCATED_MARKER",
        "",
        "这一面是**摊成键值对**的参数（候选六的另一档），不是缩进的 JSON。",
    ]
    return frame(
        "1c · 候选一在窄终端的降级（当前面 = 计时，左边的面名挤不下）",
        "标签从当前面往右画，画不下的从左边丢，用 `…` 说清丢过。读者知道**左边还有面**，但看不到"
        "它们叫什么 —— 到了这一档，它与候选三只剩排版上的差别。",
        "窄终端里退化成候选三的形状。若这一档连名字也不给，就得改成计数（`3/4`），那它就是候选三。",
        overlay(rows, bar=bar(["…", "计时"], 1), footer="↕ 1/9 · esc 关闭"),
    )


def frame2a() -> list[str]:
    rows = [section("参数")] + wrapped(ARGS_PRETTY)
    # 落盘全文与流上正文走同一条 `folded_text`：按 88 列折行，续行**不带任何标记**
    rows += [section("输出")] + wrapped("\n".join(OUTPUT_HEAD)) + ["已截断"]
    rows += [section("用量"), f"输入 {INPUT:,} · 缓存读 {CACHED:,} · 未命中 {MISS:,} · 输出 {OUTPUT}"]
    rows += [section("计时"), f"开始 09:12:11 · 工具 {TOOL_SECONDS} s · 模型 {MODEL_SECONDS} s"]
    return frame(
        "2a · 候选二：面在流里分段堆叠",
        "不切面，滚到底就是下一面，段与段之间是既有的 `── 小节 ──`。**它就是今天的详情再加两节**，"
        "零新概念：想对照读参数与输出时不用来回切。",
        "成本全落在「读到哪一节」上：页脚那个行号对四节来说是同一个数，没有「第几面」这个位置。",
        overlay(rows[:BODY_H], footer="↕ 3/47 · esc 关闭"),
    )


def frame2b() -> list[str]:
    rows = wrapped("\n".join(OUTPUT_MID)) + ["已截断", section("用量"), "…", section("计时"), "…"]
    return frame(
        "2b · 候选二在中段：屏幕上没有「第 2 面」这回事",
        "滚到输出那 138 行的中段（top = 600 行）。看得见的只有一段正文；下面还有两节，而**页脚只说**"
        "`↕ 601/701`。这就是票面那句「长正文里的第二面几乎没人滚到」的证据。",
        "要么小节标题给每节计数（`── 输出 (2/4) ──`），要么放弃 —— 后者就不是候选二了。",
        overlay(rows, footer="↕ 601/701 · esc 关闭", backdrop=trace_line_above()),
    )


def frame3() -> list[str]:
    rows = wrapped("\n".join(OUTPUT_HEAD[:3] + ["… 6 行 …"] + OUTPUT_MID[2:4] + ["… 7 行 …"])) + ["已截断"]
    return frame(
        "3 · 候选三：没有可见标签，只有页脚提示",
        "键位循环切面，页脚写 `↹ 面 2/4`。当前面是什么，**翻页前只能知道编号**：面名与内容之间"
        "没有任何标题级的连接（帧里那圈 `▐▌` 是选中底色的示意，纯文本才画得出来）。",
        "主体零代价（省下的两列全给内容）；代价是发现性 —— 读者事先不知道有几个面、叫什么。"
        "键位本身归 08 票（`tab` 今天归输入框换行）。",
        overlay(rows, footer="↹ 面 2/4 · ↕ 8/47 · esc 关闭"),
    )


def frame4a() -> list[str]:
    return frame(
        "4a · 候选四 · 渲染版：一条消息的全文（今天的样子）",
        "轨迹里那条消息只画首行 + `…`，全文住在这里。今天**只有**这一档：markdown 按文字区 88 列"
        "折行，行内样式（行内代码、粗体）由 markdown 那层给。折行标记 `↕ 1/6`。",
        "零改动。原文那一档给什么、拿不到原文的那些详情怎么办，是 08 票第三问（帧 4b 给出它能给的"
        "那一类）。",
        overlay(
            wrapped(MESSAGE_RENDERED),
            title=MESSAGE_TITLE,
            footer="↕ 1/6 · esc 关闭",
        ),
    )


def frame4b() -> list[str]:
    rows = wrapped(MESSAGE_RAW)
    rows += [
        "",
        "（事件的原始 JSON —— `at` 的精度、`speaker` 的原名、可选字段有没有来过都在这儿，",
        "  渲染那一档把它们全丢了。`first_token_ms` 落地后这里是 `null` 或一个数。）",
    ]
    return frame(
        "4b · 候选四 · 原文版：同一份内容的另一种读法",
        "原文面给**事件的原始 JSON**（今天拿得到：事件信封里就有）。它对**没有原文可拿**的详情"
        "不适用 —— 工具输出走落盘文件、注入正文是拼出来的、文件与 diff 是打开那一刻读盘来的。",
        "拿不到原文的面该写「拿不到」，**不能拿渲染结果冒充原文**（那一栏就变成同一份东西画两遍）。"
        "值得问的是：消息这一类，值的还是不值的？",
        overlay(rows, title=MESSAGE_TITLE, footer="↕ 1/9 · esc 关闭"),
    )


def frame5a() -> list[str]:
    rows = [section("本次调用")] + [
        f"输入        {INPUT:,}   {meter(CACHED + MISS, INPUT + OUTPUT, 20)}",
        f"缓存读      {CACHED:,}   {meter(CACHED, INPUT + OUTPUT, 20)}",
        f"未命中       {MISS:,}   {meter(MISS, INPUT + OUTPUT, 20)}",
        f"输出         {OUTPUT}   {meter(OUTPUT, INPUT + OUTPUT, 20)}",
        "推理          —      （供应商未报）",
        "",
        section("这一趟会话（到这条记录为止）")] + [
        f"输入        {TOTAL_INPUT:,}   {meter(TOTAL_CACHED + TOTAL_MISS, TOTAL_INPUT + TOTAL_OUTPUT, 20)}",
        f"缓存读      {TOTAL_CACHED:,}   {meter(TOTAL_CACHED, TOTAL_INPUT + TOTAL_OUTPUT, 20)}",
        f"未命中       {TOTAL_MISS:,}   {meter(TOTAL_MISS, TOTAL_INPUT + TOTAL_OUTPUT, 20)}",
        f"输出         {TOTAL_OUTPUT}   {meter(TOTAL_OUTPUT, TOTAL_INPUT + TOTAL_OUTPUT, 20)}",
        "推理          —      （供应商未报）",
        "",
        "输入 = 缓存读 + 未命中；推理已含在输出里，这两条不能再相加。",
    ]
    return frame(
        "5a · 候选五：用量面（本次 + 这一趟）",
        "桶照 `events::Usage` 的五个，与左栏面板同一套算法（`Panel::observe` 用 `Usage::accumulate`），"
        "所以这一面不会与面板漂移。条形沿用面板那套 `▓`/`░`（今天唯一的条形先例）。"
        "注意 `缓存读 + 未命中 = 输入`，所以**输入那一行的条是另外两行的和** —— 画成三条就是把它数两遍。",
        "同一个词会在三个地方出现（行尾 / 这里 / 左栏），所以必须分节 + 写清「本次」还是「这一趟」。"
        "推理为 `—` 时不画条：没有分母的条形就是骗人。",
        overlay(rows, footer="↕ 1/18 · esc 关闭"),
    )


def frame5b() -> list[str]:
    rows = [section("这一行（框外那一行上那两个数）"), "in=18310 out=204", ""]
    rows += [section("这次调用")] + [
        f"输入        {INPUT:,}   {meter(CACHED + MISS, INPUT + OUTPUT, 20)}",
        f"缓存读      {CACHED:,}   {meter(CACHED, INPUT + OUTPUT, 20)}",
        f"未命中       {MISS:,}   {meter(MISS, INPUT + OUTPUT, 20)}",
        f"输出         {OUTPUT}   {meter(OUTPUT, INPUT + OUTPUT, 20)}",
    ]
    return frame(
        "5b · 候选五的读重现场：行尾那两个数就在框外一行",
        "覆盖层上方留的那一行转录就是行尾读数（ADR 0016，`in=… out=…` 留在产生它的那次调用上）。"
        "框内第一节的 `18,310 / 204` 与它是**同一组数**的紧凑与分桶两种读法。",
        "同数两现不犯规（同一口径、同一来源），但**这一趟**那一组若与它们同屏出现就会读重："
        "小节标题必须写清「本次」还是「这一趟」，别让读者自己猜。",
        overlay(rows, footer="↕ 1/14 · esc 关闭", backdrop=trace_line_above()),
    )


def frame6a() -> list[str]:
    return frame(
        "6a · 候选六 · 参数面：照原文缩进（今天）",
        "`serde_json::to_string_pretty` 之后按 88 列折行，今天就是这样。缩进保住**类型的形状**"
        "（字符串带引号、数字不带），嵌套看得见。",
        "长字符串（`edit_file` 的 `new_string`）折出来的续行要靠拖选拼回去，复制仍然可用（OSC 52）。",
        overlay(wrapped(ARGS_PRETTY), bar=bar(FACES, 0), footer="↕ 1/9 · esc 关闭"),
    )


def frame6b() -> list[str]:
    rows = [
        "items",
        "├─ ▸ [x] 读 07 票与 research/02 的键位占用表",
        "│        已定：裸 j/k、n/N、r、f、?、{ }、数字键都还空着",
        "├─ ▸ [ ] 造检视器分面的帧",
        "│        本次；产物 prototype/07-inspector.*",
        "└─ ▸ [ ] 回填 parity.md C 节",
        "",
        "（树：一节点一行，折叠状态在行首。DSH 对完整 JSON 走 JsonTree）",
    ]
    return frame(
        "6b · 候选六 · 结果面：完整 JSON 走树",
        "结果是 `todo` 那份 items 数组时的形状。树的价值在**折叠**：只读第一层的那些节点不必滚到底。",
        "树要键入交互才划算（展开 / 折叠），而折叠键位今天空着（02 票：`{` / `}` 未绑，右键与双击"
        "连识别机制都没有）。只给静态树的话，长数组仍然要在竖向滚到底（帧 6c）。",
        overlay(rows, bar=bar(FACES, 1), footer="↕ 1/8 · esc 关闭"),
    )


def frame6c() -> list[str]:
    rows = [
        '  "items": [',
        '    { "done": true, "content": "读 07 票与 research/02 的键位占用表" },',
        '    { "done": false, "content": "造检视器分面的帧" },',
        '    { "done": false, "content": "回填 parity.md C 节" }',
        "  ]",
        "",
        "已截断",
    ]
    return frame(
        "6c · 候选六 · 结果面：原文 + 缩进（今天）",
        "不做树的版本：落盘全文（`outputs/<id>.txt`）原样排。今天的行为，零改动 —— 今天详情读的是"
        "**落盘那份**，不是流上的预览（票 02 §4）。",
        "长数组读起来要数括号；截断落在中间时最后一行是 JSON 的一半（那行 `已截断`）。",
        overlay(rows, bar=bar(FACES, 1), footer="↕ 1/6 · esc 关闭"),
    )


def frame7a() -> list[str]:
    rows = [section("这次模型调用（09:12:11 → 09:12:13）")] + [
        "开始时刻   09:12:11.204",
        f"总时长     {MODEL_SECONDS} s",
        f"首 token   {FIRST_TOKEN_S} s",
        f"生成       {GEN_SECONDS} s",
        f"吞吐       {round(OUT_TOKENS / GEN_SECONDS)} token/s（输出 {OUT_TOKENS} ÷ 0.6 s）",
        "计时来源   first_token_ms（ADR 0020）+ 事件信封的 at",
    ]
    rows += [section("这次工具调用")] + [
        "开始时刻   09:12:11.610",
        f"总时长     {TOOL_SECONDS} s",
        f"其中等审批 {APPROVAL_SECONDS} s   ← 两个口径，不是两个可相加的数",
        "计时来源   ToolCallStarted.at → ToolCallCompleted.at",
    ]
    return frame(
        "7a · 候选七 · 计时面（数据齐）",
        "[时间轴与计时](06-grilling-timeline.md)的分工是「轴给形状、这一面给精确值」。六个量照 DSH 的 "
        "Timing 面，来源写清是哪两个时刻相减。工具的**两个口径**（`duration_ms` 与等审批切分）只在"
        "这一面出现，轴上不画（06 票 §7）。",
        "一次工具调用的详情里有两个来源不同的时间块，所以要两节小标题；`首 token` 一族在老流上"
        "根本没有（帧 7b）。",
        overlay(rows, bar=bar(FACES, 3), footer="↕ 1/14 · esc 关闭"),
    )


def frame7b() -> list[str]:
    rows = [section("这次模型调用（--continue 重放的老流）")] + [
        "开始时刻   09:12:11.204",
        f"总时长     {MODEL_SECONDS} s",
        "首 token   不可用",
        "生成       不可用",
        "吞吐       不可用",
        "计时来源   事件信封的 at（老流没有 first_token_ms）",
    ]
    rows += [section("这次工具调用")] + [
        "开始时刻   09:12:11.610",
        f"总时长     {TOOL_SECONDS} s",
        f"其中等审批 {APPROVAL_SECONDS} s",
        "计时来源   ToolCallStarted.at → ToolCallCompleted.at",
    ]
    return frame(
        "7b · 候选七 · 计时面（缺数据）",
        "ADR 0020 之前写的流没有 `first_token_ms`，那三个量**不存在**。DSH 的写法是「不可用」，照抄；"
        "填 0、拿总时长顶上去、或者干脆不列，都是编。",
        "同一种详情在两场会话里长得不一样（一场三行有数、一场三行「不可用」）是预期形态 —— 这是"
        "「不编」的代价，06 票已认下。要不要写清「这场流早于 ADR 0020」，08 票第五问拍。",
        overlay(rows, bar=bar(FACES, 3), footer="↕ 1/14 · esc 关闭"),
    )


# --- 补帧（维护者要的）：来源 / 差异 / Schema / 分派表 ---------------------------------


def frame8() -> list[str]:
    rows = [
        "这一行在账本里的位置",
        "",
        "  第 342 行 · 回合 1 · 第三次迭代",
        "  09:12:12 [kimi] ▸ 调用 grep TRUNCATED_MARKER",
        "",
        "  账本现在滚到第 118 行（往下 224 行）",
        "",
        "回车 跳过去",
    ]
    return frame(
        "8 · 补帧 · 「来源」面：这一面唯一不可替代的能力是**跳回那一行**",
        "`parity.md` C 节把来源列为补齐。字符格里它比 DSH 弱一大截：那行文字**标题行已经给了**，"
        "所以这一面不能重复它 —— 它唯一多出来的是「回得去」（读者滚到覆盖层底部时，账本已经离那"
        "一行很远了）。帧里给的是位置 + 那一行的首行 + 一个回车。",
        "要跳回去就得有**稳定行键**：今天的行身份是位置型下标（`CAP` 裁剪让全体左移、宽度变化整批"
        "重放），这正是 [长历史与行选择](10-grilling-history-and-selection.md) 要补的那一层 —— "
        "所以这个面能不能做，取决于 10 票；08 票只须拍「它配不配有一个面」。",
        overlay(rows, bar=bar(["正文", "来源"], 1), footer="↕ 1/9 · esc 关闭"),
    )


def frame8b() -> list[str]:
    rows = [
        "这一次调用长在哪儿",
        "",
        "  本行  ←  09:12:11 [kimi] 迭代 2 的回复",
        "          ←  09:12:04 回合 1",
        "          ←  09:12:03 用户「帮我把 grep 的输出截断修一下…」",
        "",
        "（每一环都是账本里的一行；回车跳过去，esc 关掉）",
    ]
    return frame(
        "8b · 补帧 · 「来源」的第二种读法：把祖先链列出来，不必跳",
        "DSH 是「跳到父消息 / 所属请求」—— 一次跳转。字符格里有更省事的一档：把链**直接列出来**，"
        "读者读完就知道自己读的这一行挂在哪儿。链的每一环仍然可跳。",
        "链的深度不封顶（一回合最多 557 次迭代），所以要么按固定三环截断并写「…」，要么只列到回合。"
        "取哪一档归 08 票第八问与 10 票 —— 这也是 map「尚未明确」里那两条 fog 之一。",
        overlay(rows, bar=bar(["正文", "来源"], 1), footer="↕ 1/9 · esc 关闭"),
    )


def frame9a() -> list[str]:
    rows = [
        "这一次请求比上一次多了什么",
        "",
        "  + AGENTS.md 的「语言」一节              +12 / −3",
        "  + 技能：wayfinder、writing-for-agents",
        "  + tools：mcp_resources",
        "  − CONTEXT.md「票」那一条的旧措辞         −3 / +1",
        "",
        "上一次请求的 system prompt 不进事件流 —— 这一面是",
        "replay 从头重算每个请求的 messages 得到的；它只在",
        "`--continue` 重放或有完整落盘流时在。",
    ]
    return frame(
        "9a · 补帧 · 「差异」面：这一轮比上一次多了什么（单栏 hunk）",
        "DSH 是 system prompt 全文 + 前后 diff。衡的历史 system prompt **不进流**（数据面 §C），"
        "所以只能靠 `replay::replay` 从流重算每个请求的 `messages` 再逐块比。字符格里两列放不下，"
        "单栏 hunk 是能读的形状。",
        "重算是 O(会话长度)，而它每来一次模型调用就得重做一次 —— 这一条要么按回合边界缓存，要么"
        "接受它只在打开详情时算（帧里那个「+12 / −3」就是 diff 的行数）。",
        overlay(rows, bar=bar(["正文", "差异"], 1), footer="↕ 1/13 · esc 关闭"),
    )


def frame9b() -> list[str]:
    rows = [
        "这一次请求比上一次多了什么",
        "",
        "  不可用",
        "",
        "这一场会话的流没有落盘（或者太新，重放还差最后一段），",
        "差异只能等它可算。你仍然能看到完整的上下文注入块 ——",
        "在「正文」那一面。",
    ]
    return frame(
        "9b · 补帧 · 「差异」面拿不到时的写法",
        "拿不到就说拿不到，并说清**退路是什么**（去「正文」面看完整的注入块）。编一个"
        "「无变化」比不给差得多 —— 前者会被当成一个结论。",
        "这一档出现的条件要写清：流未落盘 / 重放失败 / 这一轮之前没有可比的那次请求（第一回合）。",
        overlay(rows, bar=bar(["正文", "差异"], 1), footer="↕ 1/9 · esc 关闭"),
    )


def frame10() -> list[str]:
    rows = [
        "grep 的参数 schema",
        "",
        '  { "type": "object",',
        '    "properties": {',
        '      "pattern": { "type": "string",',
        '        "description": "要搜的正则（rg 语法）。大小写这类需求用内联语法解决…" },',
        '      "glob":   { "type": "string", "description": "可选，只搜匹配这个 glob 的文件…" },',
        '      "count":  { "type": "boolean", "description": "可选，默认 false…" }',
        "    },",
        '    "required": ["pattern"] }',
        "",
        "这一份在渲染层的进程里：registry 里那个 spec 的 parameters",
    ]
    return frame(
        "10 · 补帧 · 「Schema」面：拿得到的那一半",
        "内建工具的 `spec().parameters` 就是一份 JSON Schema，同进程里拿得到（`src/tools/grep.rs`）。"
        "自定义工具的 schema 在 `declaration.parameters` 里，同样拿得到。**这次调用真正传进去的**"
        "那几个键，可以在这一面标出来（帧里用光标/底色示意）。",
        "它对读的人有用：他要自己发一条消息时，能用同一份形状。代价是 schema 可能很长，"
        "88 列要折好几屏 —— 所以它更像是**查一格**而不是通读。",
        overlay(rows, bar=bar(["参数", "Schema"], 1), footer="↕ 1/15 · esc 关闭"),
    )


def frame10b() -> list[str]:
    rows = [
        "mcp_call 转发出去的那个工具的参数 schema",
        "",
        "  不可用",
        "",
        "衡本地只有一个工具：`mcp_call`，它的 arguments 原样转给 server，",
        "我们不校验（`src/tools/mcp_call.rs`）。那个工具自己的 schema 住在 server",
        "那边；要它就得跑一次 tools/list。",
        "",
        "替代读法：这次调用原样转过去的 `arguments` 在「参数」面。",
    ]
    return frame(
        "10b · 补帧 · 「Schema」面拿不到的那一半",
        "外部工具的 schema 本地没有。这一格写「不可用」并给出**替代读法**（参数面有原样转发的实参），"
        "别拿 `mcp_call` 自己的 schema 冒充 —— 那是另一件东西的形状。",
        "要不要为它加一次 `tools/list` 的往返（把 schema 缓存到会话里）由 08 票第一问拍；默认不做。",
        overlay(rows, bar=bar(["参数", "Schema"], 1), footer="↕ 1/11 · esc 关闭"),
    )


def frame11() -> list[str]:
    """分派表：七种 `DetailKind` × 今天的形态 × 分面（假设）× 默认面。列宽加起来正好 88。"""
    cols = (18, 26, 34, 10)
    header = ["── 详情种类 ──", "今天（唯一的一面）", "分面（假设，待 08 票拍）", "默认面"]
    rows = [pad2("".join(pad(h, c) for h, c in zip(header, cols)))]
    rows.append("─" * TEXT_W)
    table = [
        ("消息 Message", "正文一块，首行在账本上", "概述 / 原文 / 来源", "概述"),
        ("思考 Thinking", "全文，或「未记录」", "正文 / 来源", "正文"),
        ("工具 Tool", "参数 + 输出两节", "参数/输出/用量/计时/来源", "参数"),
        ("注入 Context", "来源小节 + 正文", "正文 / 原文 / 来源", "正文"),
        ("文件 File", "读盘，行号 + 语法高亮", "正文 / 来源 / 路径", "正文"),
        ("改动 Diff", "补丁，正文带补丁高亮", "正文 / 来源 / 路径", "正文"),
        ("待办 Todo", "快照列表", "正文 / 来源", "正文"),
    ]
    for name, today, faces, default in table:
        rows.append(pad2("".join(pad(x, c) for x, c in zip((name, today, faces, default), cols))))
    rows.append("")
    rows.append("七种里只有「工具」用得上用量面与计时面 —— 模型调用数不对着这一行。")
    return frame(
        "11 · 补帧 · 分派表：哪些面出现在哪种详情里",
        "DSH 就是按记录类型分派面的（Markdown 记录一套、工具一套、system 一套、请求一套）。"
        "衡的七种 `DetailKind` 各有一个**穷尽的 match**（`src/render/tui.rs`），所以这张表可以"
        "在代码里穷举测，而不是靠约定。表是**假设**，08 票第一问逐条批。",
        "只有工具那一行用得上用量与计时两面；给别的详情挂上它们等于说谎（消息详情的开始时刻"
        "不是「那次调用」）。所以分派表不是一张「所有面都能进」的表。",
        overlay(rows, title="分派表（不是一台覆盖层，是七种详情的对照）", footer="↕ 1/13 · esc 关闭"),
    )


def frame12() -> list[str]:
    rows = [
        "  842▕pub fn stamp(sec: float) -> str {",
        "  843▕    (BASE + timedelta(seconds=sec)).strftime(\"%H:%M:%S\")",
        "  844▕}",
        "  845▕",
        "  846▕",
        "  847▕BASE = datetime(2026, 10, 9, 9, 12, 4)",
    ]
    return frame(
        "12 · 补帧 · 非工具详情的对照：文件今天长什么样（行号 + 语法高亮）",
        "文件详情不是一块普通正文：它带行号那一列、按扩展名认语言做语法高亮，而**正文是打开那一刻"
        "由渲染器直接读盘**的（不进事件流、不进模型上下文、不打码）。改动的详情是同一个形状加上"
        "补丁的语法前景与背景（ADR 0018）。",
        "它们今天只有一面是有道理的：读者要的是「看这一份东西」，不是「在几份读法之间挑」。"
        "所以分面表里这两行最短 —— 一面 + 一个跳回账本的来源。",
        overlay(rows, title="src/render/…/prototype/05-timeline.py", footer="↕ 1/9 · esc 关闭"),
    )


def pad2(text: str) -> str:
    return text + " " * max(0, TEXT_W - w(text))


def main() -> None:
    print("一次性原型：检视器分面的形态")
    print("票：.scratch/trace-ledger/issues/07-prototype-inspector.md")
    print(f"屏幕 {SCREEN_W}×{SCREEN_H} → 覆盖层 {DETAIL_W}×{DETAIL_H}，框内 {INNER_W}×{INNER_H}，"
          f"文字区 {TEXT_W}×{TEXT_H}（标题 1 + 主体 {BODY_H} + 页脚 1）")
    print("颜色不在帧里：行选中 `ACCENT+BOLD`、命中高亮用底色、失败 `BAD`、其余 `MUTED`/`CHROME`")
    print("面名是**假设**，供 08 票批；键位与默认面不在这一票。")
    for group in (
        frame0, frame1a, frame1b, frame1c, frame2a, frame2b, frame3,
        frame4a, frame4b, frame5a, frame5b, frame6a, frame6b, frame6c,
        frame7a, frame7b,
        frame8, frame8b, frame9a, frame9b, frame10, frame10b, frame11, frame12,
    ):
        print("\n".join(group()) + "\n")


if __name__ == "__main__":
    main()