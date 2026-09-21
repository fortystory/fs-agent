#!/usr/bin/env python3
"""Guard the terminal-ownership invariant at startup (spec §19).

The bug this exists for: `fs-agent` printed its startup banner with `eprintln!`
*after* the TUI renderer had been spawned. The TUI reserves its live region with
bare line feeds and then draws into it, so the banner landed on the status row —
and ratatui's diff renderer never learned those glyphs were there, leaving the
part the status line did not cover on screen. The user saw:

    ready · enter send · esc cancel · shift+tab plan · ctrl-c quitlash · mode ask · <path>

`splash` there is the tail of `model deepseek-flash`: the status line is 62
columns, and `lash` sits at offset 62 in the banner, so exactly that much of the
banner survived.

It now guards both ends of the process, because they are the two things only a
pty can see (spec §Testing Decisions): the first frame — the status row drawn
whole, the banner once, the header's identity, the pane frames present — and what
the terminal is handed back on `Ctrl-C` — the alternate screen, mouse reporting,
bracketed paste, and canonical/echoing tty flags. The cursor, the mouse and
resizing stay on the manual list (`docs/tui-manual-checklist.md`).

Why a pty script and not a Rust test: the corruption only exists on a real
terminal (the renderer is chosen by `IsTerminal`), and the CLI builds its own
sinks, so nothing in `cargo test` can observe what reaches the tty. This script
is the red-capable check; the seam tests in `tests/render_tui.rs` and
`tests/render_plain.rs` pin the mechanism the fix uses.

Run after `cargo build`:

    python3 scripts/tui-startup-check.py [binary] [runs]

Each run is made twice, once leaving by `Ctrl-C` and once by `/quit`. Exits 0 when
every run is green. A pty that does not answer the cursor-position
query (`ESC[6n`) makes ratatui fail to initialise, which is why this script
answers it.
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
# The status line ends with this word; the verdict below checks nothing foreign
# follows it on the row.
STATUS_TAIL = "退出"
# ratatui positions every wide cell with an explicit cursor move, so the raw byte
# stream is not a contiguous string once the UI is Chinese. Readiness and the
# verdict read the emulated screen instead; these ASCII anchors survive raw.
STATUS_ANCHOR = "ctrl-c"
# `fs-agent` alone also matches the store path and the bucket slug, so the banner
# anchor carries its fullwidth colon, which is written contiguously after the
# ASCII prefix.
BANNER_ANCHOR = "fs-agent："
# The four-pane frame is a horizontal rule around every block plus a vertical one
# per pane edge, so if the frames are gone the layout went with them. Three cells
# of each orientation is deliberately far below what one screen draws: this is a
# degradation guard, not a geometry assertion. The two are counted apart because
# the transcript/panel seam is a lone vertical line that survives the frames --
# counting every box character together would call a borderless screen green.
BORDER_H = "─"
BORDER_V = "│"
# What the terminal has to be given back on the way out (spec §5, §19): the
# alternate screen, mouse reporting in every encoding crossterm turns off, and
# bracketed paste. `stty`/termios is checked separately, because raw mode is a
# termios flag rather than an escape sequence.
TEARDOWN = [
    "\x1b[?1049l",
    "\x1b[?1000l",
    "\x1b[?1002l",
    "\x1b[?1003l",
    "\x1b[?1006l",
    "\x1b[?2004l",
]

# The two ways out a user actually has. Both have to hand the terminal back, so
# every run is made twice (spec §1: `/quit`, idle `Ctrl-C`; the panic path shares
# the same function but cannot be triggered on demand -- see the manual list).
GESTURES = [("ctrl-c", b"\x03"), ("/quit", b"/quit\r")]

# The tty flags a shell has to have back: canonical input, echo and signals.
Modes = collections.namedtuple("Modes", "canonical echo signals")


def tty_modes(fd):
    """The line discipline the pty was left in, as the child left it."""
    lflag = termios.tcgetattr(fd)[3]
    return Modes(
        bool(lflag & termios.ICANON),
        bool(lflag & termios.ECHO),
        bool(lflag & termios.ISIG),
    )


class Run:
    """One pty run: what was drawn, and what the terminal was left in."""

    def __init__(self, raw, exited, status, modes, survived_empty_enter):
        self.raw = raw
        self.exited = exited
        self.status = status
        self.modes = modes
        # An Enter on an empty draft is an empty line, not a closed stdin. It once
        # was read as the latter and quit the session, so the run has to still be
        # alive after one.
        self.survived_empty_enter = survived_empty_enter


def char_width(ch):
    """Terminal columns one character occupies (wide CJK is two)."""
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


class Screen:
    """Just enough VT emulation to answer "what is on the status row"."""

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
                    # A wide cell owns the column after it; blank it so joining
                    # the row does not insert a phantom space between glyphs.
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
    """One read from the pty.

    Returns the decoded text, `""` when nothing was ready, and `None` at end of
    stream. `screen`, when given, also answers any cursor-position query in the
    text: a pty that stays silent about one leaves ratatui waiting for a reply.
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
    """Send bytes, tolerating a pty that has already gone."""
    try:
        os.write(fd, data)
    except OSError:
        pass


def tty_state(fd, tries=3):
    """The line discipline the child left, read before the master is closed.

    A couple of retries because the read can race the slave closing; `None` means
    the pty would not say, which the verdict reports as itself rather than as raw
    mode left on.
    """
    for attempt in range(tries):
        try:
            return tty_modes(fd)
        except OSError:
            if attempt < tries - 1:
                time.sleep(0.05)
    return None


def capture(binary, data_home, gesture=b"\x03", timeout=20.0):
    """Run the binary on a pty until startup settles, then quit it and look behind.

    A fixed read window is flaky: assembly (context, skills, the session
    directory) can outlast it, so the banner would not have been emitted yet and
    the run would look green for the wrong reason. Wait for both the status line
    and the banner instead, plus a grace period so a duplicate write is counted.

    The way out is checked here too, because it is the same run: `gesture`, then
    wait for the process to actually go -- the terminal is only clean once it has
    (spec §19). The escape sequences it emitted and the termios it left are read
    after it exited, not guessed from the source.
    """
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["XDG_DATA_HOME"] = data_home
        os.environ["TERM"] = "xterm-256color"
        os.execv(os.path.abspath(binary), [binary])
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
    # An empty Enter first: the loop discards the empty line and asks again, so the
    # session has to still be there. This is the regression that once quit it.
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
    # Whatever was flushed as it went, then the state it left the pty in. The
    # teardown can land in the same instant as the exit, so this is not skipped.
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


def verdict(run, devnull, identity):
    """Judge one run: the first frame it drew, and what it left behind.

    The whole capture is replayed rather than sliced at the first draw: a wide
    cell is written with an explicit cursor move, so the byte offset of the status
    line is not `raw.find(STATUS_ANCHOR)`. The same replay answers the version
    anchor, which is split in the byte stream for the same reason.
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
    row = next((r for r in rows if STATUS_ANCHOR in r), None)
    if row is None:
        return False, "the status row was not on screen at the first draw"
    # The hint row lives inside the bottom block, so the block's right border
    # follows the last hint. Strip it (and any padding) before asking whether the
    # row was drawn whole; anything else after the tail is still foreign text.
    row = row.rstrip(" │")
    if not row.endswith(STATUS_TAIL):
        return False, "the status line was not drawn whole: %r" % row[-60:]
    residue = row.split(STATUS_TAIL, 1)[1].strip()
    if residue:
        return False, "the status row holds foreign text: %r" % residue[:80]
    if not any(identity in r for r in rows):
        return False, "the header does not name %r" % identity
    horizontal = sum(r.count(BORDER_H) for r in rows)
    vertical = sum(r.count(BORDER_V) for r in rows)
    if horizontal < 3 or vertical < 3:
        return False, "the pane frames are gone: %d horizontal / %d vertical" % (
            horizontal,
            vertical,
        )
    banner = run.raw.count(BANNER_ANCHOR)
    if banner != 1:
        return False, "the startup banner reached the terminal %d times" % banner
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
    """What the binary calls itself, which is what its header has to show.

    Asking the binary rather than reading `Cargo.toml` keeps the anchor honest:
    the point is that the running program's own identity reached the screen.
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
    identity = binary_identity(binary)
    if not identity:
        print("no identity from %s --version" % binary)
        return 1
    bad, total = 0, runs * len(GESTURES)
    with tempfile.TemporaryDirectory(prefix="fs-agent-tui-check-") as data_home:
        devnull = os.open(os.devnull, os.O_WRONLY)
        try:
            for i in range(runs):
                for label, gesture in GESTURES:
                    run = capture(binary, data_home, gesture)
                    ok, why = verdict(run, devnull, identity)
                    print(
                        "run %d (%s): %s -- %s"
                        % (i + 1, label, "GREEN" if ok else "RED", why)
                    )
                    bad += 0 if ok else 1
        finally:
            os.close(devnull)
    print("\n%d/%d red" % (bad, total))
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
