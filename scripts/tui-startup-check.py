#!/usr/bin/env python3
"""启动时守住「终端归属」这条不变量（spec §19）。

它守的那个 bug 长这样：`fs-agent` 在 TUI 渲染器已经被拉起来**之后**才用 `eprintln!`
打印启动横幅。TUI 用裸换行符圈出自己的活动区、再往里画，于是横幅落在状态行上 ——
而 ratatui 的差分渲染器从不知道那些字形存在，状态行没盖住的那截就留在了屏幕上。
用户看到的是：

    ready · enter send · esc cancel · shift+tab plan · ctrl-c quitlash · mode ask · <path>

那里的 `splash` 是 `model deepseek-flash` 的尾巴：状态行有 62 列，而横幅里
`lash` 坐在偏移 62 上，所以横幅活下来的正好就是这么多。

它现在守进程的两头，因为这两头只有 pty 看得见（spec §Testing Decisions）：第一帧
—— 状态行整行画出、横幅只出现一次、身份（左栏里的标记）在场、分隔列与主列
那两条横线都在 —— 以及 `Ctrl-C` 之后终端被交还回什么 —— 备用屏幕、鼠标上报、括号粘贴
（bracketed paste）、终端标题的还原序列、以及 tty 的规范 / 回显标志。进入那一头还多
一条标题：流里要出现一条 `OSC 0`，内容含工作目录的基名
（`.scratch/terminal-title/spec.md` §4）。光标、鼠标与缩放仍留在手工清单
里（`docs/tui-manual-checklist.md`）。

为什么用 pty 脚本、不用 Rust 测试：这处污染只在真终端上存在（渲染器由 `IsTerminal`
挑），而 CLI 自己组装 sink，所以 `cargo test` 里没有任何东西观察得到「到达 tty 的
是什么」。这个脚本是会红的那条检查；`tests/render_tui.rs` 与 `tests/render_plain.rs`
里的接缝测试钉住的是修复所用的那个机制。

在 `cargo build` 之后跑：

    python3 scripts/tui-startup-check.py [binary] [runs]

每一轮都按出口各跑一遍：`Ctrl-C` 连按两下、`/quit`、以及 `Ctrl-D` 连按两下
（`.scratch/exit-gesture/spec.md` §1 的双击手势），
外加每轮一次 `--continue` 重开 —— 同一个终端、一个已经存在的会话，于是启动时的历史
重播落在关键路径上。每一遍都绿才退出 0。不回答光标位置查询（`ESC[6n`）的 pty 会让
ratatui 初始化失败，所以这个脚本自己回答它。
"""
import collections
import fcntl
import os
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import unicodedata
import time

COLS, ROWS = 260, 30
# 状态行以这个词收尾；下面的判定会检查这一行里它后面没跟着别的东西。
STATUS_TAIL = "退出"
# ratatui 每画一个宽字符都要显式移动光标，所以界面一旦是中文，原始字节流就不再是一串
# 连续字符串了。就绪判定与最终判定都改读仿真出来的屏幕；这几个 ASCII 锚点在原始流里
# 依然成立。
STATUS_ANCHOR = "ctrl-c"
# 光有 `fs-agent` 也会命中会话目录的路径与桶的 slug，所以横幅锚点带上它的全角冒号 ——
# 这个冒号写在 ASCII 前缀后面、是连续的一串。
BANNER_ANCHOR = "fs-agent："
# 历史重播的进度行，在历史安定下来之前一直待在提示行。`--continue` 那一轮只有它在
# 最终屏幕上消失才算绿。
REPLAY_PROGRESS_ANCHOR = "恢复"
# 左栏按终端显示两者之一（`tui-sidebar` spec §2）：窄档是文字身份，宽到一定程度时在
# 顶上几行画**标记** —— 260x30（本脚本的尺寸）就是宽档那一级（40 列，120 列起画）。
# 标记是程序在左栏里的身份，所以两者任一都证明左栏画出来了；只认文字身份的话，
# 标记一上屏它就红。
MARK_ROW = "▄▀▀█"
# 外壳的框架如今全是**虚线**（`.scratch/tui-chrome/spec.md` §3）：左栏与主列之间的
# 那条分隔列、主列里的两条横线、以及页签条自己的两条。框架没了，就说明外壳也跟着
# 没了。每个方向只要三格，是刻意远低于一屏实际画出的量：这是降级护栏，不是几何
# 断言。横竖分开数，因为分隔列是一条孤立的竖线 —— 页签条那两条只在左栏里，数不到它。
BORDER_H = "┄"
BORDER_V = "┆"
# 退出时要交还给终端的东西（spec §5、§19）：备用屏幕、crossterm 关掉的每一种编码的
# 鼠标上报、括号粘贴，以及进入时保存的那条终端标题（CSI 23 t，见
# `.scratch/terminal-title/spec.md` §4）。`stty`/termios 另算，因为原始模式（raw mode）
# 是一个 termios 标志，不是转义序列。
TEARDOWN = [
    "\x1b[?1049l",
    "\x1b[?1000l",
    "\x1b[?1002l",
    "\x1b[?1003l",
    "\x1b[?1006l",
    "\x1b[?2004l",
    "\x1b[23;0t",
]

# 进入时那条保存标题的序列（CSI 22 t），以及标题自己那串 `OSC 0`。
TITLE_SAVE = "\x1b[22;0t"
TITLE_SET = re.compile(r"\x1b\]0;([^\x07]*)\x07")

# 用户实际有的出口。每一个都得把终端交还回来，所以每种手势各跑一轮（spec §1：`/quit`、
# 空闲时 `Ctrl-C` 与 `Ctrl-D` 都是双击 —— `.scratch/exit-gesture/spec.md` §1；panic 那条
# 路走同一个函数，但没法按需触发 —— 见手工清单）。两下要在 500 毫秒窗口内，所以一次
# `write` 发两个字节即可。
GESTURES = [
    ("ctrl-c", b"\x03\x03"),
    ("/quit", b"/quit\r"),
    ("ctrl-d", b"\x04\x04"),
]

# shell 必须拿回去的 tty 标志：规范输入、回显与信号。
Modes = collections.namedtuple("Modes", "canonical echo signals")


def tty_modes(fd):
    """pty 被留在哪种线路规程（line discipline）状态 —— 子进程离开时留下的那个样子。"""
    lflag = termios.tcgetattr(fd)[3]
    return Modes(
        bool(lflag & termios.ICANON),
        bool(lflag & termios.ECHO),
        bool(lflag & termios.ISIG),
    )


class Run:
    """一次 pty 运行：画出了什么，以及终端被留在了什么状态。"""

    def __init__(self, raw, exited, status, modes, survived_empty_enter):
        self.raw = raw
        self.exited = exited
        self.status = status
        self.modes = modes
        # 空草稿上按 Enter 是一个空行，不是 stdin 关闭。它曾经被读成后者、顺手把会话
        # 退掉了，所以按过一次之后这一轮必须还活着。
        self.survived_empty_enter = survived_empty_enter


def char_width(ch):
    """一个字符占的终端列数（宽的 CJK 算两列）。"""
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


class Screen:
    """刚够回答「状态行上是什么」的 VT 仿真。"""

    def __init__(self, reply_fd):
        self.reply_fd = reply_fd
        self.grid = [[" "] * COLS for _ in range(ROWS)]
        self.cx = self.cy = 0
        self.pending = ""

    def feed(self, text):
        buf = self.pending + text
        i = 0
        while i < len(buf):
            ch = buf[i]
            if ch == "\x1b":
                m = re.match(r"\x1b\[6n", buf[i:])
                if m:
                    reply = "\x1b[%d;%dR" % (self.cy + 1, self.cx + 1)
                    try:
                        os.write(self.reply_fd, reply.encode())
                    except OSError:
                        pass
                    i += m.end()
                    continue
                m = re.match(r"\x1b\[([0-9;?]*)([A-Za-z])", buf[i:])
                if m:
                    nums = [int(p) for p in m.group(1).split(";") if p.isdigit()]
                    cmd = m.group(2)
                    if cmd == "H":
                        self.cy = (nums[0] - 1) if nums else 0
                        self.cx = (nums[1] - 1) if len(nums) > 1 else 0
                    elif cmd == "J" and (not nums or nums[0] in (2, 3)):
                        self.grid = [[" "] * COLS for _ in range(ROWS)]
                    elif cmd == "K":
                        for x in range(self.cx, COLS):
                            self.grid[self.cy][x] = " "
                    elif cmd == "A":
                        self.cy = max(0, self.cy - (nums[0] if nums else 1))
                    elif cmd == "B":
                        self.cy = min(ROWS - 1, self.cy + (nums[0] if nums else 1))
                    elif cmd == "L":
                        for _ in range(nums[0] if nums else 1):
                            self.grid.insert(self.cy, [" "] * COLS)
                            self.grid.pop()
                    i += m.end()
                    continue
                m = re.match(
                    r"\x1b[()][A-Za-z0-9]|\x1b[=>]|\x1b\][^\x07]*\x07|\x1b\[[0-9;?]*[hlm]",
                    buf[i:],
                )
                if m:
                    i += m.end()
                    continue
                i += 1
                continue
            if ch == "\n":
                self.cy = min(ROWS - 1, self.cy + 1)
            elif ch == "\r":
                self.cx = 0
            elif ch == "\x08":
                self.cx = max(0, self.cx - 1)
            elif ord(ch) < 32:
                pass
            else:
                width = char_width(ch)
                if 0 <= self.cy < ROWS and 0 <= self.cx < COLS:
                    self.grid[self.cy][self.cx] = ch
                    # 宽字符把它后面那一列也占了；先清空，免得拼这一行时在字形之间
                    # 插进一个假空格。
                    for trail in range(1, width):
                        if self.cx + trail < COLS:
                            self.grid[self.cy][self.cx + trail] = ""
                self.cx += width
                if self.cx >= COLS:
                    self.cx = 0
                    self.cy = min(ROWS - 1, self.cy + 1)
            i += 1
        self.pending = buf[i:]

    def rows(self):
        return ["".join(r).rstrip() for r in self.grid]


def read_once(fd, screen=None, timeout=0.2):
    """从 pty 读一次。

    返回解码后的文本：没东西可读时返回 `""`，流结束时返回 `None`。给了 `screen` 的话，
    还会回答文本里的光标位置查询：对查询一声不吭的 pty 会让 ratatui 一直等回答。
    """
    readable, _, _ = select.select([fd], [], [], timeout)
    if not readable:
        return ""
    try:
        data = os.read(fd, 65536)
    except OSError:
        return None
    if not data:
        return None
    text = data.decode("utf-8", "replace")
    if screen is not None:
        screen.feed(text)
    return text


def write(fd, data):
    """发送字节，容忍一个已经消失的 pty。"""
    try:
        os.write(fd, data)
    except OSError:
        pass


def tty_state(fd, tries=3):
    """子进程留下的线路规程，在关闭 master 之前读。

    重试几次是因为这次读可能和 slave 关闭赛跑；`None` 表示 pty 不肯说 —— 判定会把它
    报成「读不到」，而不是报成「原始模式还开着」。
    """
    for attempt in range(tries):
        try:
            return tty_modes(fd)
        except OSError:
            if attempt < tries - 1:
                time.sleep(0.05)
    return None


def capture(binary, data_home, gesture=b"\x03", timeout=20.0, args=()):
    """在 pty 上跑这个二进制，等启动安定下来，再让它退出、回头看看留下了什么。

    固定的读窗口不可靠：组装（context、skills、会话目录）可能比它更久，那样横幅还没
    打印出来，这一轮就会因为错误的原因看着是绿的。所以改成等状态行与横幅**都**出现，
    再加一段宽限期，好让重复写入也被数进去。

    出口也在这里一并查了，因为它是同一轮运行：先发 `gesture`，再等进程真的走掉 ——
    只有它走了，终端才算干净（spec §19）。它发出的转义序列与它留下的 termios 都在
    进程退出之后读，不从源码里猜。

    `args` 是额外的 CLI 参数；`--continue` 用它重开同一个 `data_home` 里已有的会话。
    """
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["XDG_DATA_HOME"] = data_home
        os.environ["TERM"] = "xterm-256color"
        os.execv(os.path.abspath(binary), [binary, *args])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    raw, screen = "", Screen(fd)
    deadline, settle_by = time.time() + timeout, None
    while time.time() < deadline:
        text = read_once(fd, screen)
        if text is None:
            break
        raw += text
        if settle_by is None and BANNER_ANCHOR in raw and any(
            STATUS_ANCHOR in row for row in screen.rows()
        ):
            settle_by = time.time() + 0.4
        if settle_by is not None and time.time() >= settle_by:
            break
    # 先来一个空 Enter：循环会丢掉这条空行、再问一次，所以会话必须还在。这就是当年
    # 把会话退掉的那个回归。
    write(fd, b"\r")
    time.sleep(0.4)
    reaped, wait_status = os.waitpid(pid, os.WNOHANG)
    survived_empty_enter = reaped == 0
    exited, status = (not survived_empty_enter), (
        wait_status if not survived_empty_enter else None
    )
    if survived_empty_enter:
        write(fd, gesture)
        deadline = time.time() + 6.0
        while time.time() < deadline:
            text = read_once(fd, screen, 0.1)
            if text:
                raw += text
                continue
            reaped, wait_status = os.waitpid(pid, os.WNOHANG)
            if reaped == pid:
                exited, status = True, wait_status
                break
        if not exited:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
    # 它退出时冲出来的都收下，再看它把 pty 留在了什么状态。收尾可能与退出落在同一
    # 瞬间，所以这一步不能跳过。
    end = time.time() + 0.3
    while time.time() < end:
        text = read_once(fd, screen, 0.1)
        if not text:
            break
        raw += text
    modes = tty_state(fd)
    try:
        os.close(fd)
    except OSError:
        pass
    return Run(raw, exited, status, modes, survived_empty_enter)


def verdict(run, devnull, identity, replay=False, cwd_base=""):
    """判定一轮运行：它画出的第一帧，以及它留下的东西。

    整个抓取内容都被重放一遍，而不是在第一帧处切片：宽字符是带显式光标移动写出来的，
    所以状态行的字节偏移不是 `raw.find(STATUS_ANCHOR)`。同一次重放也用来找版本锚点
    —— 它因为同一个道理在字节流里是断开的。

    `replay` 标记 `--continue` 那一轮：此时最终屏幕还必须显示重播已经**收敛** ——
    历史进度行消失，只剩普通的状态行。重播的**内容**不在这里判，那是 `cargo test`
    的活（`.scratch/tui-history-replay/spec.md` §Testing Decisions）。

    `cwd_base` 是这个工作目录的基名：进入那一头必须有一条含它的标题到达终端
    （`.scratch/terminal-title/spec.md` §4）。
    """
    if not run.survived_empty_enter:
        return False, "the session did not survive an empty Enter"
    if not run.exited:
        return False, "the quit gesture did not end the process"
    if run.status != 0:
        return False, "the quit gesture left exit status %r" % (run.status,)
    frame = Screen(devnull)
    frame.feed(run.raw)
    rows = frame.rows()
    if replay and any(REPLAY_PROGRESS_ANCHOR in r for r in rows):
        return False, "the history replay did not converge: %r" % (
            next(r for r in rows if REPLAY_PROGRESS_ANCHOR in r)[-60:],
        )
    row = next((r for r in rows if STATUS_ANCHOR in r), None)
    if row is None:
        return False, "the status row was not on screen at the first draw"
    # 提示行住在底部区块里面，所以区块的右边框跟在最后一条提示后面。问这一行是否完整
    # 画出之前先剥掉它（以及任何填充）；尾巴之后的其他东西仍然算外来文本。
    row = row.rstrip(" │")
    if not row.endswith(STATUS_TAIL):
        return False, "the status line was not drawn whole: %r" % row[-60:]
    residue = row.split(STATUS_TAIL, 1)[1].strip()
    if residue:
        return False, "the status row holds foreign text: %r" % residue[:80]
    if not any(identity in r for r in rows) and not any(MARK_ROW in r for r in rows):
        return False, "the header shows neither %r nor the mark" % identity
    horizontal = sum(r.count(BORDER_H) for r in rows)
    vertical = sum(r.count(BORDER_V) for r in rows)
    if horizontal < 3 or vertical < 3:
        return False, "the shell's rules are gone: %d horizontal / %d vertical" % (
            horizontal,
            vertical,
        )
    banner = run.raw.count(BANNER_ANCHOR)
    if banner != 1:
        return False, "the startup banner reached the terminal %d times" % banner
    if TITLE_SAVE not in run.raw:
        return False, "the terminal title was never saved (no %r)" % TITLE_SAVE
    titles = TITLE_SET.findall(run.raw)
    if not any(cwd_base in title for title in titles):
        return False, "no title naming %r reached the terminal: %r" % (
            cwd_base,
            titles[:3],
        )
    if run.modes is None:
        return False, "could not read what the tty was left in"
    if not (run.modes.canonical and run.modes.echo and run.modes.signals):
        return False, "the tty was left raw: %r" % (run.modes,)
    missing = [seq for seq in TEARDOWN if seq not in run.raw]
    if missing:
        return False, "the terminal was not given back: %s missing" % ", ".join(
            repr(seq) for seq in missing
        )
    return True, "status row clean, banner once, terminal handed back"


def binary_identity(binary):
    """二进制自称的名字，也就是它的头部必须显示的东西。

    问二进制本身、而不是读 `Cargo.toml`，是为了让锚点诚实：要证明的是**正在跑的
    这个程序**自己的身份到了屏幕上。
    """
    out = subprocess.run(
        [os.path.abspath(binary), "--version"],
        capture_output=True,
        text=True,
        timeout=30,
    )
    return out.stdout.strip()


def main():
    binary = sys.argv[1] if len(sys.argv) > 1 else "target/debug/fs-agent"
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    if not os.path.exists(binary):
        print("no binary at %s; run cargo build first" % binary)
        return 1
    # 子进程继承脚本的工作目录，所以标题里的基名就是这里算出来的那个。
    cwd_base = os.path.basename(os.getcwd())
    identity = binary_identity(binary)
    if not identity:
        print("no identity from %s --version" % binary)
        return 1
    bad, total = 0, runs * (len(GESTURES) + 1)
    with tempfile.TemporaryDirectory(prefix="fs-agent-tui-check-") as data_home:
        devnull = os.open(os.devnull, os.O_WRONLY)
        try:
            for i in range(runs):
                for label, gesture in GESTURES:
                    run = capture(binary, data_home, gesture)
                    ok, why = verdict(run, devnull, identity, cwd_base=cwd_base)
                    print(
                        "run %d (%s): %s -- %s"
                        % (i + 1, label, "GREEN" if ok else "RED", why)
                    )
                    bad += 0 if ok else 1
                # 重开上面几轮刚建出来的那个会话，还在同一个会话目录里：这样历史重播
                # 就落在启动路径上，而它是否收敛、交还回来的终端是什么样，正是 pty
                # 能看到的。
                run = capture(
                    binary, data_home, gesture=b"\x03\x03", args=("--continue",)
                )
                ok, why = verdict(
                    run, devnull, identity, replay=True, cwd_base=cwd_base
                )
                print(
                    "run %d (--continue): %s -- %s"
                    % (i + 1, "GREEN" if ok else "RED", why)
                )
                bad += 0 if ok else 1
        finally:
            os.close(devnull)
    print("\n%d/%d red" % (bad, total))
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
