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

Why a pty script and not a Rust test: the corruption only exists on a real
terminal (the renderer is chosen by `IsTerminal`), and the CLI builds its own
sinks, so nothing in `cargo test` can observe what reaches the tty. This script
is the red-capable check; the seam tests in `tests/render_tui.rs` and
`tests/render_plain.rs` pin the mechanism the fix uses.

Run after `cargo build`:

    python3 scripts/tui-startup-check.py [binary] [runs]

Exits 0 when every run is green. A pty that does not answer the cursor-position
query (`ESC[6n`) makes ratatui fail to initialise, which is why this script
answers it.
"""
import fcntl
import os
import pty
import re
import select
import struct
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


def capture(binary, data_home, timeout=20.0):
    """Run the binary on a pty until startup has settled, then quit it.

    A fixed read window is flaky: assembly (context, skills, the session
    directory) can outlast it, so the banner would not have been emitted yet and
    the run would look green for the wrong reason. Wait for both the status line
    and the banner instead, plus a grace period so a duplicate write is counted.
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
        readable, _, _ = select.select([fd], [], [], 0.2)
        if readable:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if not data:
                break
            text = data.decode("utf-8", "replace")
            raw += text
            screen.feed(text)
        if settle_by is None and BANNER_ANCHOR in raw and any(
            STATUS_ANCHOR in row for row in screen.rows()
        ):
            settle_by = time.time() + 0.4
        if settle_by is not None and time.time() >= settle_by:
            break
    try:
        os.write(fd, b"\x03")
    except OSError:
        pass
    time.sleep(0.3)
    try:
        while True:
            readable, _, _ = select.select([fd], [], [], 0.15)
            if not readable:
                break
            data = os.read(fd, 65536)
            if not data:
                break
            raw += data.decode("utf-8", "replace")
    except OSError:
        pass
    try:
        os.close(fd)
    except OSError:
        pass
    return raw


def verdict(raw, devnull):
    """Judge the reconstructed status row.

    The whole capture is replayed rather than sliced at the first draw: a wide
    cell is written with an explicit cursor move, so the byte offset of the status
    line is not `raw.find(STATUS_ANCHOR)`.
    """
    frame = Screen(devnull)
    frame.feed(raw)
    row = next((r for r in frame.rows() if STATUS_ANCHOR in r), None)
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
    banner = raw.count(BANNER_ANCHOR)
    if banner != 1:
        return False, "the startup banner reached the terminal %d times" % banner
    return True, "status row clean, banner shown once"


def main():
    binary = sys.argv[1] if len(sys.argv) > 1 else "target/debug/fs-agent"
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    if not os.path.exists(binary):
        print("no binary at %s; run cargo build first" % binary)
        return 1
    bad = 0
    with tempfile.TemporaryDirectory(prefix="fs-agent-tui-check-") as data_home:
        devnull = os.open(os.devnull, os.O_WRONLY)
        try:
            for i in range(runs):
                ok, why = verdict(capture(binary, data_home), devnull)
                print("run %d: %s -- %s" % (i + 1, "GREEN" if ok else "RED", why))
                bad += 0 if ok else 1
        finally:
            os.close(devnull)
    print("\n%d/%d red" % (bad, runs))
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
