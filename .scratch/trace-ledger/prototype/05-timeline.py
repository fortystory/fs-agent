#!/usr/bin/env python3
"""一次性原型：时间轴在字符格里的候选形态。

票：`.scratch/trace-ledger/issues/05-prototype-timeline.md`
跑法：
    python3 .scratch/trace-ledger/prototype/05-timeline.py > .scratch/trace-ledger/prototype/05-timeline.txt

**throwaway**：不接渲染器，只把三种候选在真宽度下画出来给眼睛判一次。宽度照今天的实现
（轨迹内容 79 列；泳道标签吃掉 5 列，横轴 74 列），措辞照今天的实现。

数据是**造的一条会话**（形态演示），但每一个量都只取数据面证过的来源：

  · 模型段   = `TurnStarted.at` → 下一个边界（`TurnStarted(i+1).at` 或 `TurnEnded.at`）
  · 工具段   = `ToolCallStarted.at` → `ToolCallCompleted.at`（`duration_ms` 是同一个区间）
  · 等审批   = `PermissionAsked.at` → `PermissionDecided.at`（在工具段**之内**）
  · 空档     = 以上都没盖住的列（组装请求没有事件，只能读作「这里什么都没记」）

**不画**：TTFT / 生成时长 / 吞吐 / 「输入」泳道 —— 数据面证明这四样今天没有来源
（首 token 时刻在 provider / agent / 事件流三层都不存在）。分辨率与轴宽两条张力见各帧说明。

时刻账与 `.scratch/trace-ledger/prototype/03-ledger-skeleton.txt` 那场会话对齐：
read_file 完成 09:12:11、grep 完成 09:12:12、cargo test 完成 09:12:27 与 09:12:45、
edit_file 失败于 09:12:29、read_file 完成 09:12:30。
"""

import unicodedata
from datetime import datetime, timedelta

TRACE_W = 79  # 轨迹内容宽（主列内容宽，扣掉滚动条与回合条那两列的口径见票面）
LABEL_W = 5  # 泳道标签：4 列（`模型` / `工具` / `空档`）+ 一个空格
AXIS_W = TRACE_W - LABEL_W  # 横轴：74 列

BASE = datetime(2026, 10, 9, 9, 12, 4)


def w(text: str) -> int:
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - w(text))


def stamp(sec: float) -> str:
    return (BASE + timedelta(seconds=sec)).strftime("%H:%M:%S")


# ---------------------------------------------------------------------------
# 这一趟的时间账（回合 1：09:12:04 → 09:12:52，48.0 s）
# ---------------------------------------------------------------------------

T0, T1 = 0.0, 48.0  # 相对 BASE 的秒
SPAN = T1 - T0

# 模型调用：一次迭代一段，从 TurnStarted.at 到下一个边界。
MODEL = [(0.0, 7.0), (9.0, 20.0), (26.0, 39.5), (42.0, 48.0)]

# 工具段：(开始, 结束, 名字)。迭代 2 里 bash 与 read_file 是**并发**的，
# edit_file 又落在 read_file 之内（`src/agent.rs:1212-1250`）。
TOOLS = [
    (7.2, 7.6, "read_file"),
    (8.0, 8.2, "grep"),
    (20.0, 23.0, "bash"),
    (21.8, 26.0, "read_file"),
    (24.8, 25.0, "edit_file"),
    (39.5, 41.0, "bash"),
]

ITER_BOUNDS = [0.0, 9.0, 26.0, 42.0, 48.0]  # 5 个边界 = 4 次迭代


def cols(a: float, b: float, span: float = SPAN, width: int = AXIS_W) -> tuple[int, int]:
    """一段秒数 → 半开列区间，保底一格（0.65 s 以下的段否则会缩成零列）。"""
    c0 = int(round(a / span * width))
    c1 = int(round(b / span * width))
    if c1 <= c0:
        c1 = c0 + 1
    return max(0, min(c0, width - 1)), min(c1, width)


def blank(n: int = AXIS_W, ch: str = " ") -> list[str]:
    return [ch] * n


def lane_model(segments=MODEL, span=SPAN, fill="█", width=AXIS_W) -> list[str]:
    row = blank(width)
    for a, b in segments:
        c0, c1 = cols(a, b, span, width)
        for c in range(c0, c1):
            row[c] = fill
    return row


def lane_tools(segments=TOOLS, span=SPAN, width=AXIS_W) -> list[str]:
    """工具泳道：一格上一段是 `▁`，两段叠着是 `█` —— 并发读得出来。"""
    depth = [0] * width
    for a, b, _ in segments:
        c0, c1 = cols(a, b, span, width)
        for c in range(c0, c1):
            depth[c] += 1
    return [" " if d == 0 else ("▁" if d == 1 else "█") for d in depth]


def combine(model_row: list[str], tool_row: list[str]) -> list[str]:
    """两条泳道压成一行：`━` 模型在跑、`▂` 工具在跑、`▃` 工具叠着（按同一轴重采样）。"""
    out = blank(len(model_row))
    for c in range(len(out)):
        if model_row[c] == "█":
            out[c] = "━"
        elif tool_row[c] == "▁":
            out[c] = "▂"
        elif tool_row[c] == "█":
            out[c] = "▃"
    return out


def lane_idle(span=SPAN) -> list[str]:
    busy = [False] * AXIS_W
    for a, b in MODEL:
        c0, c1 = cols(a, b, span)
        for c in range(c0, c1):
            busy[c] = True
    for a, b, _ in TOOLS:
        c0, c1 = cols(a, b, span)
        for c in range(c0, c1):
            busy[c] = True
    return [" " if busy[c] else "·" for c in range(AXIS_W)]


def ruler(span=SPAN, every=12) -> str:
    """刻度行：每 `every` 秒一个 8 字符时刻戳，末端那一个右对齐到轴尾。"""
    row = blank()

    def put(sec: float, right_align: bool = False) -> None:
        text = stamp(sec)
        c = int(round(sec / span * AXIS_W))
        if right_align or c + len(text) > AXIS_W:
            c = AXIS_W - len(text)
        for i, ch in enumerate(text):
            if 0 <= c + i < AXIS_W:
                row[c + i] = ch

    sec = 0.0
    while sec <= span - every * 0.5:
        put(sec)
        sec += every
    put(span, right_align=True)
    return " " * LABEL_W + "".join(row)


# ---------------------------------------------------------------------------
# 帧
# ---------------------------------------------------------------------------

FRAMES: list[tuple[str, str, list[str]]] = []


def frame(title: str, why: str, lines: list[str]) -> None:
    FRAMES.append((title, why, lines))


def bar(share: float, width: int = 45) -> str:
    full = int(share * width)
    eighths = " ▏▎▍▌▋▊▉"
    rest = share * width - full
    tail = eighths[int(rest * 8)] if full < width else ""
    return "█" * full + tail


# --- 帧 0：时间账 -----------------------------------------------------------

model_total = sum(b - a for a, b in MODEL)
tool_sum = sum(b - a for a, b, _ in TOOLS)  # 各段相加（并发那一段数两次）
tool_union = 1.0 + 6.0 + 1.5  # 迭代 1 / 2 / 3 的工具墙钟并集
idle_total = SPAN - model_total - tool_union

frame(
    "帧 0a · 先把这一趟的秒数摊开（所有候选都押在它上面）",
    "口径全部来自数据面：模型段 = `TurnStarted.at` → 下一个边界；工具段 = `ToolCallStarted.at` → "
    "`ToolCallCompleted.at`；空档 = 谁都没盖住的那些列。结论本身就有说服力 —— 78% 的时间在等模型。",
    [
        f"回合 1：{stamp(T0)} → {stamp(T1)} = {SPAN:.1f} s，4 次迭代、6 次工具调用",
        "",
        pad("模型调用", 16) + f"{model_total:5.1f} s  {model_total / SPAN:4.0%}  {bar(model_total / SPAN)}",
        pad("工具（墙钟并集）", 16) + f"{tool_union:5.1f} s  {tool_union / SPAN:4.0%}  {bar(tool_union / SPAN)}",
        pad("空档", 16) + f"{idle_total:5.1f} s  {idle_total / SPAN:4.0%}  {bar(idle_total / SPAN)}",
        "",
        f"（工具各段相加 {tool_sum:.1f} s；并集 {tool_union:.1f} s —— 差的 {tool_sum - tool_union:.1f} s 是并发那一段数了两次。",
        " 两个口径都真，看你回答的是哪一个。）",
    ],
)

frame(
    "帧 0b · 逐段账（横轴上的每一格是从哪来的）",
    "48 s 铺在 74 列上 —— 一列 ≈ 0.65 s。这条分辨率决定了下面每一种候选能说什么、不能说什么。",
    [
        pad("段", 22) + "时长    是什么",
        "-" * TRACE_W,
    ]
    + [
        pad(f"{stamp(a)} → {stamp(b)}", 22) + f"{b - a:4.1f} s  {what}"
        for a, b, what in [
            (0.0, 7.0, "迭代 1 · 模型"),
            (7.0, 9.0, "迭代 1 · 工具 read_file + grep，中间 0.4 s 空档"),
            (9.0, 20.0, "迭代 2 · 模型"),
            (20.0, 26.0, "迭代 2 · 工具 bash + read_file（并发）"),
            (26.0, 39.5, "迭代 3 · 模型"),
            (39.5, 42.0, "迭代 3 · 工具 bash，然后 1.0 s 空档"),
            (42.0, 48.0, "迭代 4 · 模型（最后一句话，没有工具）"),
        ]
    ]
    + [
        "",
        "0.2 s 的调用在 74 列里只值 0.3 格 —— 保底画一格就是把它说成 0.65 s。",
        "拿不到的（不画）：TTFT · 生成时长 · 吞吐（分母是首 token 时刻）· 「输入」泳道。",
    ],
)

# --- 候选一：顶部横带 -------------------------------------------------------

model_row = lane_model()
tool_row = lane_tools()
idle_row = lane_idle()

frame(
    "帧 1a · 候选一：顶部横带，三泳道 + 刻度 = 4 行",
    "四行从转录的 15 行里出（120×24 下）。标尺一行给绝对时刻；泳道自上而下是模型 / 工具 / 空档。"
    "代价：4 行 = 可视高度的 27%，而账本一屏只有 15 行。",
    [
        ruler(),
        pad("模型", LABEL_W) + "".join(model_row),
        pad("工具", LABEL_W) + "".join(tool_row),
        pad("空档", LABEL_W) + "".join(idle_row),
        "",
        "读得出来：哪几段在等模型、哪几段在跑工具、空档在哪。",
        "读不出来：模型那 7.0 s 里有多少在等首 token（首 token 时刻不存在，不许切）。",
        "并发：21.8–23.0 s 那两格是 `█`（bash 与 read_file 同时在跑），一格就是 0.65 s。",
    ],
)

frame(
    "帧 1b · 候选一（省行版）：两泳道，空档靠留白读 = 3 行",
    "去掉空档那一行：空档变成两条泳道之间的真空，读得出「这里什么都没跑」，但读不出「空档有多长」"
    "与隔壁工具的重叠。省下的一行还给账本。",
    [
        ruler(),
        pad("模型", LABEL_W) + "".join(model_row),
        pad("工具", LABEL_W) + "".join(tool_row),
        "",
        "09:12:12.2 → 09:12:13 那 0.8 s 空档，在这一版里与「轴到头了」长得一样。",
    ],
)

single = combine(model_row, tool_row)

frame(
    "帧 1c · 候选一（最省版）：一行，模型在上半、工具在下半",
    "`━` 是模型在跑、`▂` 是工具在跑、`▃` 是工具叠着 —— 一行说清「谁在占这一格」。代价：读不出"
    "时长（`━` 与 `▂` 只说明有没有），也读不出「模型与工具之间的因果」。",
    [
        ruler(),
        pad("全程", LABEL_W) + "".join(single),
        "",
        "这一行能塞进状态行上方（候选三）或者页签条旁边，不占账本的行。",
    ],
)

# 位置感：整趟运行 ≠ 这一屏
LONG_SPAN = 716.0  # 09:12:04 → 09:24:00
long_model = [(a, b) for a, b in MODEL]
long_tools = [(a, b, n) for a, b, n in TOOLS]
for k in range(1, 5):
    off = 150.0 * k
    long_model += [(off + a, off + b) for a, b in MODEL]
    long_tools += [(off + a, off + b, n) for a, b, n in TOOLS]

lm = lane_model(long_model, LONG_SPAN)
lt = lane_tools(long_tools, LONG_SPAN)
view = blank()
for c in range(*cols(0.0, 48.0, LONG_SPAN)):
    view[c] = "▔"

frame(
    "帧 1d · 位置感：这一屏在整趟运行的哪一段",
    "今天的账本只有右侧那 1 列滚动条说「你在哪、还有多少」。带上加一行 `▔▔▔` 说同一件事，"
    "而且说得更准：它同时给出「这一段多长」与「前后还有多少」。",
    [
        ruler(LONG_SPAN, every=180),
        pad("模型", LABEL_W) + "".join(lm),
        pad("工具", LABEL_W) + "".join(lt),
        pad("视口", LABEL_W) + "".join(view),
        "",
        "整趟 09:12:04 → 09:24:00（12 分钟）里，这一屏是头 48 s —— 右边还有 93%。",
        "轴宽随会话增长：12 分钟里一格 ≈ 9.7 s，48 s 那一趟缩成 5 格。所以带要么跟窗口走，",
        "要么只画当前单位（候选三）。整趟数据是形态演示，真实只有回合 1/2 那两段。",
    ],
)

# --- 候选二：行内迷你条 -----------------------------------------------------

def mini(seconds: float, scale: float, cells: int = 6) -> str:
    """行尾迷你条：长度按 `scale` 归一化，保底一格。"""
    n = int(round(seconds / scale * cells))
    return "█" * max(1, min(cells, n))


frame(
    "帧 2a · 候选二：行尾迷你条，按整趟 48 s 归一化",
    "零新区域，不占账本一行。代价票面自己就写了：看不出谁与谁重叠、看不出空档。还有一个更硬的：",
    [
        "09:12:11  [kimi] ▸ 调用 read_file src/render/tui.rs        ▏",
        "09:12:12  [kimi] ▸ 调用 grep TRUNCATED_MARKER              ▏",
        "09:12:24  [kimi] ▸ 调用 bash 运行 cargo test -q · 3.0 s    ███▊",
        "09:12:25  [kimi] ▸ 调用 read_file src/render/tui.rs        █████▎",
        "09:12:29  [kimi] ▸ 调用 edit_file src/render/tui.rs 失败   ▏",
        "09:12:43  [kimi] ▸ 调用 bash 运行 cargo test -q · 1.5 s    █▉",
        "",
        "0.2 s 的两次调用与 0.4 s 的那一次都塌成同一格 —— 三行在说同一个数字。",
        "行尾已是最挤的地方（03 票张力 1：耗时一加、用量就被截断），再加 6 格是逼它让位。",
    ],
)

frame(
    "帧 2b · 候选二（换归一化）：按本回合内最大的一次工具（4.2 s）",
    "同一批数字换分母：小的那几次终于看得见差别。代价是**分母会变** —— 新来一次更长的调用，"
    "前面每一行的条子都缩水，「这一行多长」于是不可比。",
    [
        "09:12:11  [kimi] ▸ 调用 read_file src/render/tui.rs        █",
        "09:12:12  [kimi] ▸ 调用 grep TRUNCATED_MARKER              █",
        "09:12:24  [kimi] ▸ 调用 bash 运行 cargo test -q · 3.0 s    ████▎",
        "09:12:25  [kimi] ▸ 调用 read_file src/render/tui.rs        ██████",
        "09:12:29  [kimi] ▸ 调用 edit_file src/render/tui.rs 失败   █",
        "09:12:43  [kimi] ▸ 调用 bash 运行 cargo test -q · 1.5 s    ██▏",
        "",
        "两条都活不了：迷你条必须有个分母，而两种分母各自坏一件事。",
    ],
)

# --- 候选三：状态行附近 -----------------------------------------------------

frame(
    "帧 3a · 候选三：贴状态行上方一行，只画当前回合",
    "只画当前单位（回合 1 的 48 s）：分辨率比顶部那条全程带高得多，也不额外占行 —— 但那 1 行本身",
    [
        pad("模型", LABEL_W) + "".join(model_row),
        pad("工具", LABEL_W) + "".join(tool_row),
        "模型 claude-sonnet-4-5 │ 模式 询问 │ 上下文 6% │ ⟳ 运行中",
        "",
        "是从账本的 15 行里拿的（贴状态行 = 占转录最后一行）。15 行 → 14 行，而且它会把账本",
        "最后一行盖住 —— 贴底跟随时，最新那一行正好在它底下。",
    ],
)

status_content = "模型 claude-sonnet-4-5 │ 模式 询问 │ 上下文 6% │ ⟳ 运行中"
slots = AXIS_W - w(status_content) - 3
narrow = combine(lane_model(width=slots), lane_tools(width=slots))
frame(
    f"帧 3b · 候选三（并进状态行）：79 列里剩下的那 {slots} 列",
    "状态行自己已经占了 57 列。并进去的好处是零新行；坏处是这条带子在降级阶梯的最末尾 —— "
    "终端一窄，先掉的就是它（状态行本来就会截断）。",
    [
        status_content + " " + "".join(narrow),
        "",
        f"（这一版一格 ≈ {SPAN / slots:.1f} s，比顶部那条（一格 {SPAN / AXIS_W:.2f} s）粗 {AXIS_W / slots:.1f} 倍；",
        "  而且它跟「上下文 6%」抢同一行。）",
    ],
)

# --- 选中一个区间 -----------------------------------------------------------

SEL_A, SEL_B = 20.0, 26.0  # 迭代 2 的工具那 6 秒
sel_lane = blank()
for c in range(*cols(SEL_A, SEL_B)):
    sel_lane[c] = "▔"

frame(
    "帧 4a · 选中一个区间：起止怎么标、账本怎么聚焦",
    "票面第 4 条要求的那一帧。手势本身归 06 票（拖选 / 点两下 / 键位），这里只画**选中之后**"
    "长什么样：带上多一行 `▔▔▔` 标出区间，账本把区间外的行整行褪到 MUTED（不删、不折叠）。",
    [
        ruler(),
        pad("模型", LABEL_W) + "".join(model_row),
        pad("工具", LABEL_W) + "".join(tool_row),
        pad("选择", LABEL_W) + "".join(sel_lane),
        "（`▔▔▔` 这一段 = 09:12:24 → 09:12:30，也就是迭代 2 的三次工具调用）",
        "",
        "命中（区间内的行留原样）：",
        "09:12:24  [kimi] ▸ 调用 bash 运行 cargo test -q · 3.0 s    20.0 → 23.0 s",
        "09:12:26  [kimi] ▸ 调用 read_file src/render/tui.rs         21.8 → 26.0 s",
        "09:12:29  [kimi] ▸ 调用 edit_file src/render/tui.rs 失败    24.8 → 25.0 s",
        "",
        "未命中（整行 MUTED，帧里看不出来）：",
        "09:12:11  [kimi] ▸ 调用 read_file src/render/tui.rs         07.2 → 07.6 s",
        "09:12:12  [kimi] ▸ 调用 grep TRUNCATED_MARKER               08.0 → 08.2 s",
        "09:12:43  [kimi] ▸ 调用 bash 运行 cargo test -q · 1.5 s     39.5 → 41.0 s",
        "",
        "两条必须先定：**命中判据**是「跨度与区间相交」还是「开始时刻落在区间里」；**聚焦是褪色",
        "还是过滤**（褪色保位置感、过滤省行数 —— DSH 是淡出 + 保位置）。判据不是小事：区间取",
        "09:12:25 → 09:12:26 时，bash 那一行（20.0 → 23.0）**跨过**了它，相交判据留下它、开始时刻",
        "判据漏掉它 —— 而它正是那 1 秒里在跑的东西。区间选中与搜索命中是否同一套机制，归 09 票。",
    ],
)

# --- 空档与等待 -------------------------------------------------------------

frame(
    "帧 5a · 三类等待：模型在想 / 等审批 / 工具在跑",
    "票面第 5 条。三类里只有一类今天能在**带内部**切开 —— 等审批有 `PermissionAsked` / "
    "`PermissionDecided` 两条带时刻的事件；「模型在想」那一段切不开，因为首 token 时刻不存在。",
    [
        pad("模型", LABEL_W) + "".join(model_row),
        pad("工具", LABEL_W) + "".join(tool_row),
        pad("空档", LABEL_W) + "".join(idle_row),
        "",
        "「模型在想」= 模型那条实心本身。它内部有多少是「等首 token」、多少是「在吐字」：",
        "  今天无从而知。唯一的诚实画法就是整段说「在等模型」。",
        "「工具在跑」= 工具那条 `▁` / `█`。",
        "「等审批」= 工具段**之内**的一小段 —— 只在有权限事件的调用上存在，见 5b。",
        "「空档」= 第三行那些 `·`。它读作「这一段什么都没记」：请求组装没有事件，所以空档的",
        "  成因（组装？排队？）在流上没有名字。",
    ],
)

# 放大等审批那 3 秒（回合 2：bash 09:13:41 → 09:13:44，其中 09:13:41.4 → 09:13:43.8 在等人）
ZOOM_SPAN = 3.0
ZOOM_BASE = datetime(2026, 10, 9, 9, 13, 41)
ZOOM_TRAIL = 14  # 行尾给「bash 3.0 s」那种标注留的列
ZOOM_AXIS = AXIS_W - ZOOM_TRAIL
zoom_tool = blank(ZOOM_AXIS)
for c in range(*cols(0.0, 3.0, ZOOM_SPAN, ZOOM_AXIS)):
    zoom_tool[c] = "▁"
zoom_wait = blank(ZOOM_AXIS)
for c in range(*cols(0.4, 2.8, ZOOM_SPAN, ZOOM_AXIS)):
    zoom_wait[c] = "▓"


def zoom_ruler() -> str:
    row = blank(ZOOM_AXIS)
    for sec in (0.0, 1.0, 2.0):
        text = (ZOOM_BASE + timedelta(seconds=sec)).strftime("%H:%M:%S")
        c = int(round(sec / ZOOM_SPAN * ZOOM_AXIS))
        if c + len(text) > ZOOM_AXIS:
            c = ZOOM_AXIS - len(text)
        for i, ch in enumerate(text):
            row[c + i] = ch
    return " " * LABEL_W + "".join(row)


frame(
    "帧 5b · 放大那 3 秒：工具段内部能切到哪里",
    "工具段 = `ToolCallStarted.at` → `ToolCallCompleted.at`，而 `duration_ms` 的口径含钩子、权限与"
    "排队（`src/agent.rs:956`）—— 也就是说**一次 3.0 s 的调用里可能 2.4 s 是在等人**。权限事件",
    [
        zoom_ruler(),
        pad("工具", LABEL_W) + "".join(zoom_tool) + "  bash 3.0 s",
        pad("审批", LABEL_W) + "".join(zoom_wait) + "  等审批 2.4 s",
        "",
        "带时刻，所以这一段**切得开**：`PermissionAsked.at` → `PermissionDecided.at`。",
        "这是「等待」三类里唯一有名字、有起止的一段，也是唯一能指着说「这里在等你」的一段。",
        "代价：它只在真有权限询问的调用上存在 —— 没问过的工具段里，等人的时间读不出来。",
    ],
)

# --- 代价小结 ---------------------------------------------------------------

frame(
    "帧 6 · 三种候选的代价，并排放",
    "票面的三种候选各自活在哪一格上。底下三行是这次画出来才看清的三件事。",
    [
        pad("候选", 20) + "占账本行数   读数密度        主要代价",
        "-" * TRACE_W,
        pad("一 · 顶部横带", 20) + "3（省行版）  全程 / 当前屏   吃掉 15 行里的 3–4 行",
        pad("二 · 行内迷你条", 20) + "0            每一行自己      要分母；增量在跑的行没有长度",
        pad("三 · 状态行附近", 20) + "1            当前回合        盖住贴底那一行；降级阶梯最末尾",
        "",
        "画出来才看清的三件事：",
        "1. **分辨率是硬约束**：74 列铺 48 s 是一格 0.65 s。0.2 s 的调用与 0.4 s 的调用在带上",
        "   长得一样 —— 除非保底画一格，而那一格就是在说谎。轴越宽（会话越长）越糟。",
        "2. **「模型在想」这一类切不开**：TTFT 拿不到，模型段只能整段。所以时间轴能给的最强",
        "   结论是「78% 的时间在等模型」，而不是「其中多少在等首 token」。",
        "3. **等审批是唯一切得开的内段**，但 `duration_ms` 已经把它算进工具耗时里了 —— 时间轴",
        "   要切开它，就得说清「工具 3.0 s」与「工具本体 0.6 s」是两个口径，别并排放而不说明。",
    ],
)


frame(
    "帧 7 · 这张产物给 06 票的输入（三件必须拍的事）",
    "只列问题、不给答案 —— 答案归 `issues/06-grilling-timeline.md` 的 live exchange。",
    [
        "1. 选哪一种、要不要组合：全程带（1b 那种 3 行）/ 当前回合带（1 行，贴状态行）/ 行内迷你条（0 行）。",
        "   两条现成的组合路子：全程带给位置感、当前回合作细读；或者全程带 + 行选中高亮（帧 4a）。",
        "2. 拿不到的那几样怎么读：TTFT / 生成时长 / 吞吐 / 「输入」泳道 —— 留白、写一句「没有这个数」、",
        "   还是为它动一次流（`MessageCompleted` 加一个可选字段 + 一条 ADR，代价在数据面 B 节）。",
        "3. 计时口径先钉死，因为两处会并排出现：工具的「3.0 s」含等审批与排队（本体只 0.6 s）；",
        "   模型的「11 s」含组装与预算检查。时间轴画哪一个、检视器显示哪一个，得是同一套说法。",
        "",
        "已在别处、这里只提醒：区间聚焦与搜索命中的关系归 09 票；区间的手势归 06 票自己拍。",
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
