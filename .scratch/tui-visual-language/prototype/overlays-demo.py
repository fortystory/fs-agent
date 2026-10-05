#!/usr/bin/env python3
"""四个浮层表面的现状与提案 —— 对着真终端看（prototype，用完即弃）。

用法：

    python3 .scratch/tui-visual-language/prototype/overlays-demo.py

四个表面：模态（权限 / 问题）、`/` 与 `@` 菜单、详情覆盖层、问卷。
每一节先画现状，再画按色板 / 字形 / 层级三票定下来的形态。
"""

from __future__ import annotations

import unicodedata

MUTED = "\x1b[38;5;8m"
PLAIN = "\x1b[39m"
CHROME = "\x1b[38;2;74;74;74m"
ACCENT = "\x1b[38;5;13m"
WARN = "\x1b[38;5;3m"
INJECTED = "\x1b[38;5;12m"
BOLD = "\x1b[1m"
REV = "\x1b[7m"
OFF = "\x1b[0m"
DIMC = "\x1b[2m"


def cw(text: str) -> int:
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - cw(text))


def box(lines: list[str], width: int, corner: str, border: str, body_style: str = "") -> list[str]:
    """四个角给 `corner`（空格 = 空角），横竖用 `border`。"""
    inner = width - 2
    top = border * 2 + corner if False else corner + border * inner + corner
    out = [top]
    for line in lines:
        out.append(border + body_style + pad(line, inner) + OFF + border + OFF)
    out.append(corner + border * inner + corner)
    return out


def cbox(lines: list[str], width: int, border_color: str) -> list[str]:
    """带空角的虚线框（04 定的框架档形态）。"""
    inner = width - 2
    top = f"{border_color} {'┄' * inner} {OFF}"
    bot = top
    out = [top]
    for line in lines:
        out.append(f"{border_color}┆{OFF} " + pad(line, inner - 2) + f" {border_color}┆{OFF}")
    out.append(bot)
    return out


def section(title: str, note: str = "") -> None:
    print()
    print(f"{PLAIN}{BOLD}{title}{OFF}")
    if note:
        print(f"{MUTED}{note}{OFF}")
    print()


def show(label: str, lines: list[str]) -> None:
    print(f"  {MUTED}{label}{OFF}")
    for line in lines:
        print("    " + line)
    print()


def main() -> int:
    # ── ① 模态 ──────────────────────────────────────────────────────────────
    section("① 模态（权限 / 问题覆盖层）", "现状：边框、正文、按钮、键名**整块 Yellow**（tui.rs:4024/4038/4053/4112）。")
    body = ["要跑这条命令吗？", "cargo test --all-targets"]
    show("现状", box(body + ["", "[y] 允许   [n] 拒绝"], 36, "┌", f"{WARN}┄{OFF}") if False else
         cbox([f"{WARN}{l}{OFF}" if l else "" for l in body + ["", "     [y] 允许   [n] 拒绝"]], 36, WARN))
    show(
        "提案：边框 CHROME + 空角、正文 PLAIN、按钮行 ACCENT（键名 BOLD）",
        cbox(body + ["", f"     {ACCENT}{BOLD}[y]{OFF}{ACCENT} 允许{OFF}   {ACCENT}{BOLD}[n]{OFF}{ACCENT} 拒绝{OFF}"], 36, CHROME),
    )
    print(f"    {DIMC}整块黄拆成三件事：框是框（CHROME）、话是话（PLAIN）、要按的键是焦点（ACCENT）。{OFF}")

    # ── ② 菜单 ──────────────────────────────────────────────────────────────
    section("② `/` 与 `@` 菜单", "现状：边框 CHROME_LINE，未选中行 Yellow，选中行 Black on Yellow（tui.rs:4809-4816）。")
    show("现状", cbox([f"{WARN} /help      查看帮助{OFF}", f"{WARN}\x1b[43m\x1b[30m /clear     清空上下文{OFF}", f"{WARN} /loop      认领目标{OFF}"], 34, CHROME))
    show(
        "提案：正文 PLAIN，光标行 REVERSED（不再引黄）",
        cbox([f"{PLAIN} /help      查看帮助{OFF}", f"{PLAIN}{REV} /clear     清空上下文{OFF}", f"{PLAIN} /loop      认领目标{OFF}"], 34, CHROME),
    )

    # ── ③ 详情 ──────────────────────────────────────────────────────────────
    section("③ 详情覆盖层", "现状：整个框穿**发言者色**（tui.rs:5940，理由是「不用读一个字就知道是谁的行」）。")
    show("现状", cbox([f"{BOLD}[kimi] 调用 bash{OFF}", "", f"{MUTED}── 参数 ──{OFF}", f"command=cargo test"], 46, INJECTED))
    show(
        "提案：边框归 CHROME + 空角；发言者色退到**标题行**",
        cbox([f"{INJECTED}{BOLD}[kimi] 调用 bash{OFF}", "", f"{MUTED}── 参数 ──{OFF}", f"command=cargo test"], 46, CHROME),
    )
    print(f"    {DIMC}「这是谁的行」没丢，只是从整个框缩到一行；框架语法因此四个表面共一套。{OFF}")

    # ── ④ 问卷 ──────────────────────────────────────────────────────────────
    section("④ 问卷", "现状：表头 Cyan+BOLD、已选 Yellow+BOLD、当前高亮 REVERSED（选项区）/ DIM（输入区）。")
    q = [
        "【旧】",
        f"{INJECTED}{BOLD}选一个方案{OFF}",
        f"{PLAIN}> {WARN}{BOLD}○ 方案 A{OFF}",
        f"{PLAIN}  \x1b[7m○ 方案 B{OFF}",
        f"  {MUTED}自定义：{OFF}",
        f"{MUTED}1 / 1 · esc 退出询问{OFF}",
    ]
    show("现状", q)
    q2 = [
        "【新】",
        f"{PLAIN}{BOLD}选一个方案{OFF}",
        f"{PLAIN}> ○ 方案 A{OFF}",
        f"{PLAIN}  {REV}○ 方案 B{OFF}{MUTED}   ← 键盘在这儿{OFF}",
        f"{PLAIN}  自定义：{OFF}",
        f"{MUTED}1 / 1 · esc 退出询问{OFF}",
    ]
    show("提案：表头 PLAIN+BOLD；已选只靠 BOLD 与标记；**全屏唯一的 REVERSED 就是键盘所在**", q2)
    print(f"    {DIMC}已选不再是黄色（问卷不是警告）；输入区聚焦时把 REVERSED 从选项行移到自定义行，{OFF}")
    print(f"    {DIMC}于是「键盘在哪儿」永远只有一个答案，也不再需要 DIM。{OFF}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
