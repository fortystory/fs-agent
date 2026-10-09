#!/usr/bin/env python3
"""一次性原型：把账本骨架的几种候选画成真帧。

票：`.scratch/trace-ledger/issues/03-prototype-ledger-skeleton.md`
跑法：`python3 .scratch/trace-ledger/prototype/03-ledger-skeleton.py > .scratch/trace-ledger/prototype/03-ledger-skeleton.txt`

这是 **throwaway**：措辞与宽度照今天的实现抄（9 列时间戳 `HH:MM:SS `、`[名字] ▸ ` 前缀、
`调用 <tool> <描述>`、行尾 `in=… out=…`、单位之间 79 列 `┄`），但没有接渲染器 —— 目的只是让
「分组 / 序号 / 耗时 / 失败 / 折叠」这几种骨架在真宽度下被眼睛判一次。宽度按显示宽度算，
中文两列。
"""

import unicodedata

TRACE_W = 79  # 主列内容宽（120 列终端带左栏）
STAMP_W = 9  # HH:MM:SS + 一个空格
RULE = "┄" * TRACE_W


def w(text: str) -> int:
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def fit(text: str, width: int) -> str:
    if w(text) <= width:
        return text
    out, used = "", 0
    for ch in text:
        cw = 2 if unicodedata.east_asian_width(ch) in "WF" else 1
        if used + cw > width - 1:
            break
        out += ch
        used += cw
    return out + "…"


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - w(text))


def line(cell: str, body: str, cell_w: int = STAMP_W) -> str:
    """一行轨迹：行首那一格（补齐到 `cell_w`）+ 内容（吃掉剩下的宽度）。"""
    return pad(fit(cell, cell_w), cell_w) + fit(body, TRACE_W - cell_w)


def raw(text: str) -> str:
    """不占行首那一格的行（分隔线、说明、或整行本来就是内容）。"""
    return text


FRAMES: list[tuple[str, str, list[str]]] = []


def frame(title: str, why: str, lines: list[str]) -> None:
    FRAMES.append((title, why, lines))


# ---------------------------------------------------------------------------
# 帧 0：今天（基线）
# ---------------------------------------------------------------------------

frame(
    "帧 0 · 今天的样子（基线）",
    "平铺、无分组、无序号；用量长在产生它的那次调用的行尾（ADR 0016）；单位之间一条 79 列的"
    "虚线。下面每一帧都与它对照。",
    [
        line("09:12:03", "▸ [上下文注入：系统提示词（按当前代码拼，不进事件流）]"),
        line("09:12:03", "▸ [上下文注入：技能清单]"),
        line("09:12:03", "[用户] ▸ 帮我把 grep 的输出截断修一下，超限时只留头尾"),
        line("09:12:04", "回合开始（第 1 次迭代）"),
        line("09:12:05", "[kimi] … 正在思考"),
        line("09:12:11", "[kimi] ▸ ✓ 思考完成"),
        line("09:12:11", "[kimi] ▸ 调用 read_file src/render/tui.rs   in=18234 out=312"),
        line("09:12:12", "[kimi] ▸ 调用 grep TRUNCATED_MARKER      in=18310 out=204"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q      in=19502 out=1880"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        line("09:12:30", "[kimi] ▸ 调用 read_file src/render/tui.rs   in=21620 out=198"),
        line("09:12:45", "[kimi] ▸ 调用 bash 运行 cargo test -q      in=22880 out=1650"),
        line("09:12:52", "[kimi] ▸ 头尾都留住了，省掉多少也写清楚了…  in=24130 out=402"),
        line("", "                                          合计 in=124126 out=4789"),
        line("09:12:52", "回合结束：完成"),
        raw(RULE),
        line("09:13:30", "[用户] ▸ 再帮我看下 bash 工具的超时"),
        line("09:13:31", "回合开始（第 1 次迭代）"),
        line("09:13:33", "[kimi] … 正在思考"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 1：分组 —— 组头怎么画
# ---------------------------------------------------------------------------

frame(
    "帧 1a · 分隔线照旧 + 组头自己一行，成员缩进两格",
    "零改动的加法：那条虚线还在，它下面多一行组头。代价是每个单位多花一行，而轨迹页在"
    "120×24 下只有 15 行可视高度。",
    [
        raw(RULE),
        line("09:12:04", "回合 1 · 49 s · bash×2 read×2 edit×1"),
        line("09:12:05", "  [kimi] … 正在思考"),
        line("09:12:11", "  [kimi] ▸ ✓ 思考完成"),
        line("09:12:11", "  [kimi] ▸ 调用 read_file src/render/tui.rs   in=18234 out=312"),
        line("09:12:12", "  [kimi] ▸ 调用 grep TRUNCATED_MARKER      in=18310 out=204"),
        line("09:12:27", "  [kimi] ▸ 调用 bash 运行 cargo test -q      in=19502 out=1880"),
        line("09:12:29", "  [kimi] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        line("09:12:52", "  [kimi] ▸ 头尾都留住了，省掉多少也写清楚了…"),
        line("09:12:52", "  回合结束：完成"),
    ],
)

frame(
    "帧 1b · 组头并进分隔线那一行（零额外行数）",
    "同一行既是分界又是组头：省下一整行，密度最好。代价是它不再是一条干净的虚线（这行仍读作"
    "框架，用 `┄` 收尾以免看着像内容被截断）。",
    [
        raw("┄┄ 回合 1 · 49 s · bash×2 read×2 edit×1 " + "┄" * 40),
        line("09:12:05", "[kimi] … 正在思考"),
        line("09:12:11", "[kimi] ▸ ✓ 思考完成"),
        line("09:12:11", "[kimi] ▸ 调用 read_file src/render/tui.rs   in=18234 out=312"),
        line("09:12:12", "[kimi] ▸ 调用 grep TRUNCATED_MARKER      in=18310 out=204"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q      in=19502 out=1880"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        line("09:12:52", "[kimi] ▸ 头尾都留住了，省掉多少也写清楚了…"),
        line("09:12:52", "回合结束：完成"),
    ],
)

frame(
    "帧 1c · 不画分隔线，组头自己一行，成员不缩进",
    "把「一条线 + 一行头」压成「一行头」：省两行。成员不缩进，内容宽度一点不损失 —— 代价是"
    "层级只靠那一行头与颜色表达，而且要借 `▸` 这个字形（它今天已经是「可展开」的意思）。",
    [
        line("09:12:04", "▸ 回合 1 · 49 s · bash×2 read×2 edit×1"),
        line("09:12:05", "[kimi] … 正在思考"),
        line("09:12:11", "[kimi] ▸ ✓ 思考完成"),
        line("09:12:11", "[kimi] ▸ 调用 read_file src/render/tui.rs   in=18234 out=312"),
        line("09:12:12", "[kimi] ▸ 调用 grep TRUNCATED_MARKER      in=18310 out=204"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q      in=19502 out=1880"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        line("09:12:52", "[kimi] ▸ 头尾都留住了，省掉多少也写清楚了…"),
        line("09:12:52", "回合结束：完成"),
    ],
)

frame(
    "帧 1d · 两级分组：单位 → 迭代",
    "DSH 的 turn → Step。衡的对应物是「单位」（回合 / 轮次）与「迭代」（一次模型调用，"
    "`TurnStarted.iteration` 已在流上）。第二级头写「第 N 次迭代」与它的跨度，成员再缩两格 ——"
    "又要一行，而内容只剩 36 列。",
    [
        line("09:12:04", "回合 1 · 49 s · 3 次迭代"),
        line("09:12:05", "  第 1 次迭代 · 7 s · 21 行模型输出"),
        line("09:12:05", "    [kimi] … 正在思考"),
        line("09:12:11", "    [kimi] ▸ ✓ 思考完成"),
        line("09:12:11", "    [kimi] ▸ 调用 read_file src/render/tui.rs  in=18234 out=312"),
        line("09:12:12", "    [kimi] ▸ 调用 grep TRUNCATED_MARKER     in=18310 out=204"),
        line("09:12:27", "  第 2 次迭代 · 18 s"),
        line("09:12:27", "    [kimi] ▸ 调用 bash 运行 cargo test -q     in=19502 out=1880"),
        line("09:12:29", "    [kimi] ▸ 调用 edit_file … 失败  in=21460 out=143"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 2：行首那一格怎么排
# ---------------------------------------------------------------------------

frame(
    "帧 2 · 行首那一格：时间戳、序号，还是要两个",
    "今天固定 9 列时刻（`HH:MM:SS `），内容拿 70 列。序号挤进来要么加宽那一格、要么顶掉时刻。"
    "下面每一行标出它给内容留了多少列。",
    [
        raw("（a）只时间戳 —— 内容 70 列（今天）"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q   in=19502 out=1880"),
        raw(""),
        raw("（b）时间戳 + 序号 —— 那一格涨到 12 列，内容 67 列"),
        line("09:12:27 #7", "[kimi] ▸ 调用 bash 运行 cargo test -q   in=19502 out=1880", 12),
        raw(""),
        raw("（c）序号 + 时间戳 —— 同样 12 列，但序号先读"),
        line("#7 09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q   in=19502 out=1880", 12),
        raw(""),
        raw("（d）只序号 —— 那一格 4 列，内容 75 列；时刻只剩在别处（详情 / 块）"),
        line("#7", "[kimi] ▸ 调用 bash 运行 cargo test -q       in=19502 out=1880", 4),
        raw(""),
        raw("（e）序号并进名字前缀 —— 那一格不动，`#N` 从内容里吃两列"),
        line("09:12:27", "[kimi #7] ▸ 调用 bash 运行 cargo test -q   in=19502 out=1880"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 3：耗时挂在哪
# ---------------------------------------------------------------------------

frame(
    "帧 3 · 耗时（`duration_ms` 今天没上屏）：三种挂法",
    "工具块手里已经有 `duration_ms`（口径是**调用墙钟**，含钩子与排队）。它要与行尾那笔用量"
    "共存 —— 那一处已经是最挤的地方：看 (a) 的第二条，它已经挤到被截断了。",
    [
        raw("（a）行尾，与用量并列（`· 1.5 s` 在前，用量在后）"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q · 1.5 s  in=19502 out=1880"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs · 0.2 s 失败  in=21460 out=143"),
        raw(""),
        raw("（b）行首那一格里右对齐一段固定宽度（`   1.5s`）"),
        line("    1.5s", "[kimi] ▸ 调用 bash 运行 cargo test -q  in=19502 out=1880"),
        line("    0.2s", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        raw(""),
        raw("（c）贴名字：`[kimi ·1.5s]`，读起来像「谁、多久」"),
        line("09:12:27", "[kimi ·1.5s] ▸ 调用 bash 运行 cargo test -q  in=19502 out=1880"),
        line("09:12:29", "[kimi ·0.2s] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        raw(""),
        raw("（d）模型调用（一次迭代）的耗时挂在它的开始行上 —— 它没有自己的行"),
        line("09:12:04", "回合开始（第 1 次迭代） · 7.3 s"),
        raw(""),
        raw("（e）拿不到耗时的那一类（还在跑的、被取消的）不画 —— 空格比编一个数好"),
        line("09:13:33", "[kimi] ▸ 调用 bash 运行 cargo test --all  in=31002 out=90"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 4：失败
# ---------------------------------------------------------------------------

frame(
    "帧 4 · 失败：今天只有末尾一个 `失败`",
    "行上给不给错误码是个真问题：**流上没有 code**（只有 `error: String`，见 `research/01`），"
    "所以 (b) 那一档要么动流、要么从文本里猜。",
    [
        raw("（a）今天：末尾一个红色的 `失败`，错误正文进详情"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败  in=21460 out=143"),
        raw(""),
        raw("（b）末尾带错误码（**要动流**：今天只有 error 文本）"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败 · not_found"),
        raw(""),
        raw("（c）行首一个 `✗`（行首那格加宽 2 列，红色落在框架位上）"),
        line("✗", "[kimi] ▸ 调用 edit_file src/render/tui.rs  in=21460 out=143", 11),
        raw(""),
        raw("（d）错误正文的首行直接上屏，不点开也知道坏在哪"),
        line("09:12:29", "[kimi] ▸ 调用 edit_file src/render/tui.rs 失败"),
        line("09:12:29", "  找不到 src/render/tui.rs：没有这个文件"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 5：空结果与结构化结果
# ---------------------------------------------------------------------------

frame(
    "帧 5 · 工具行上的结果读成什么",
    "DSH 读成 `name · args → 结果`，空输出给一句灰话。衡今天行上完全没有结果那一半 —— "
    "全在详情里。",
    [
        raw("（a）今天：行上只有调用那一半"),
        line("09:12:12", "[kimi] ▸ 调用 grep TRUNCATED_MARKER  in=18310 out=204"),
        raw(""),
        raw("（b）空结果给一句话"),
        line("09:12:12", "[kimi] ▸ 调用 grep TRUNCATED_MARKER · 无输出  in=18310 out=204"),
        raw(""),
        raw("（c）结果是 JSON 时，行上给形状而不是内容"),
        line("09:12:12", "[kimi] ▸ 调用 bash 运行 jq .name package.json · {\"name\":\"heng\"}"),
        raw(""),
        raw("（d）大结果只给量（今天详情里才看得到）"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q · 188 行  in=19502 out=1880"),
        raw(""),
        raw("（e）被截断过的结果：省掉多少要说出来"),
        line("09:12:27", "[kimi] ▸ 调用 bash 运行 cargo test -q · 188 行（省掉 412 行）"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 6：折叠
# ---------------------------------------------------------------------------

frame(
    "帧 6 · 折叠：一个单位、一串同类调用",
    "折叠要求行是**可寻址的**（今天连「选中一行」都没有，见 `research/02`）。字形要注意："
    "`▸` 已经是「可以展开的东西」。",
    [
        raw("（a）一个单位折成一行：块数与跨度都留着，读者知道省掉了什么"),
        line("09:12:04", "▸ 回合 1 · 7 个块 · 49 s · bash×2 read×2 edit×1"),
        raw(""),
        raw("（b）连续同类工具调用折成一条"),
        line("09:12:45", "▸ 调用 bash ×6 · 14.2 s（点开看每一次）"),
        raw(""),
        raw("（c）折起来之后右侧给一个「还能展开」的提示"),
        line("09:12:52", "▸ 回合 1 · 7 个块 · 49 s                      ⌄ 展开"),
        raw(""),
        raw("（d）会话开头那一段（注入 + 身份）折起来 —— 它只在开场读一次"),
        line("09:12:03", "▸ 开场 · 4 条注入 · 系统提示词 + 技能清单 + AGENTS.md"),
    ],
)

# ---------------------------------------------------------------------------
# 帧 7：无主段落
# ---------------------------------------------------------------------------

frame(
    "帧 7 · 那些不属于任何单位的块",
    "DSH 给压缩记录一个 `Between turns` 区段，于是「不属于任何回合」这件事看得见。衡这边有"
    "三处：会话开头、压缩（`HistorySuperseded`）、`/clear` 之后。",
    [
        raw("（a）会话开头：一个开场段，或者什么都不说（今天第二行起就是内容）"),
        line("09:12:03", "▸ 开场"),
        line("09:12:03", "  ▸ [上下文注入：系统提示词（按当前代码拼，不进事件流）]"),
        line("09:12:03", "  ▸ [上下文注入：技能清单]"),
        raw(""),
        raw("（b）压缩：它确实发生在一段对话之外"),
        line("09:12:00", "▸ 压缩 · 上下文过半，前面 42 轮压成摘要"),
        line("09:12:00", "  [历史：压缩] 读过的文件与踩过的坑都留着了…"),
        raw(""),
        raw("（c）`/clear` 之后：一段新的开始"),
        line("09:14:02", "▸ 新会话段（上一段留在磁盘上）"),
        line("09:14:02", "[用户] ▸ 我们从零开始讲一遍 grep 工具"),
    ],
)


def main() -> None:
    out: list[str] = []
    for title, why, lines in FRAMES:
        out.append("=" * TRACE_W)
        out.append(title)
        out.append(why)
        out.append("-" * TRACE_W)
        out.extend(lines)
        out.append("")
    print("\n".join(out))


if __name__ == "__main__":
    main()
