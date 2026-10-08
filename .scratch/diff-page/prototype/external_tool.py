#!/usr/bin/env python3
"""throwaway prototype：外部工具那一档长什么样。

这台机器上**没有** delta / diff-so-fancy / colordiff（只有 `bat`），所以票里那句「真跑一遍
delta、贴几行帧」跑不了。这一份用一个**假的外部工具**代替它，只回答一件事：那一档里，
我们到底收到什么、要有多难才能画出来。

    python3 .scratch/diff-page/prototype/external_tool.py

三段：
1. 假的 delta（吃 stdin 的 diff，吐带 SGR 的上色重排输出）；
2. 它吐出来的**原始字节**（我们真正收到的东西）；
3. 按「只认 SGR 子集」解出来的 span，以及画到屏幕上是什么样。
"""

from __future__ import annotations

DIFF = """diff --git a/src/render/diff_page.rs b/src/render/diff_page.rs
index 3f1a2b4..9c8d7e6 100644
--- a/src/render/diff_page.rs
+++ b/src/render/diff_page.rs
@@ -12,7 +12,9 @@ fn draw(&self, page: Rect) {
     let Some(page) = page else { return };
     let rows = match self.tab {
         Tab::Usage => self.panel.lines(page),
-        Tab::Files => self.files.lines(page),
+        Tab::Diff => self.diff.lines(page),
+        Tab::Files => self.files.lines(page),
     };
     note_rows(state, page, &rows, &[]);
 }
"""

# SGR 里我们打算认下来的那几档（够 delta 这类工具用，不需要整台终端模拟器）
RESET = "\x1b[0m"
GREEN = "\x1b[38;5;114m"
RED = "\x1b[38;5;174m"
CYAN_BOLD = "\x1b[1;36m"
DIM = "\x1b[2m"

# 解析器认得的那几档 SGR 参数 → 我们的样式名。认不出的落到 `unknown(...)`，
# 由绘制侧退成 `PLAIN` 或按最近似的一档收下。
SGR_MAP = {
    "": "plain",
    "0": "plain",
    "2": "dim",
    "1;36": "cyan+bold",
    "38;5;114": "green",
    "38;5;174": "red",
}


def fake_tool(diff: str) -> str:
    """扮演一个 delta 类的工具：丢头行、给行号、按标记上色、hunk 头重排。"""
    out: list[str] = []
    old = new = 0
    for line in diff.splitlines():
        if line.startswith(("diff --git", "index ", "--- ", "+++ ")):
            continue
        if line.startswith("@@"):
            spec = line.split("@@")[1].strip()
            old = int(spec.split(" ")[0][1:].split(",")[0])
            new = int(spec.split(" ")[1][1:].split(",")[0])
            rest = line.split("@@")[2].strip()
            out.append(f"{CYAN_BOLD}@@ {rest} @@{RESET}")
            continue
        tag = line[:1]
        if tag == "+":
            out.append(f"{DIM}{new:>4}{RESET} {GREEN}{line}{RESET}")
            new += 1
        elif tag == "-":
            out.append(f"{DIM}{old:>4}{RESET} {RED}{line}{RESET}")
            old += 1
        else:
            out.append(f"{DIM}{new:>4}{RESET} {line}")
            old += 1
            new += 1
    return "\n".join(out) + "\n"


def parse_sgr(line: str) -> list[tuple[str, str]]:
    """只认 SGR（`ESC[...m`）；其他 CSI 序列（光标、清屏）一律丢掉。"""
    spans: list[tuple[str, str]] = []
    style, buf, i = "plain", "", 0
    while i < len(line):
        if line[i] == "\x1b" and line[i + 1 : i + 2] == "[":
            end = i + 2
            while end < len(line) and (line[end].isdigit() or line[end] in ";?"):
                end += 1
            if end >= len(line):  # 截断的转义序列：剩下的都丢掉
                break
            final = line[end]
            if final == "m":  # SGR：先把攒下的文本收成一个 span，再换样式
                if buf:
                    spans.append((buf, style))
                    buf = ""
                param = line[i + 2 : end]
                style = SGR_MAP.get(param, f"unknown({param})")
            # 其他最终字节（H / J / K 那一类）整段丢掉，不留痕迹
            i = end + 1
            continue
        buf += line[i]
        i += 1
    if buf:
        spans.append((buf, style))
    return spans


def main() -> None:
    painted = fake_tool(DIFF)
    print("══ 1. 外部工具吐出来的原始字节（我们收到的就是这些）" + "═" * 12)
    for line in painted.splitlines()[:6]:
        print(f"  {line[:96]!r}")

    print("\n══ 2. 按「只认 SGR」解出来的 span（每行一组）" + "═" * 14)
    for line in painted.splitlines()[:6]:
        print("  " + "  ".join(f"{text!r}:{style}" for text, style in parse_sgr(line)))

    print("\n══ 3. 画到屏幕上（这里是草图，样式用括注标出）" + "═" * 15)
    for line in painted.splitlines():
        spans = parse_sgr(line)
        rendered = "".join(
            f"⟨{style}⟩{text}⟨/⟩" if style != "plain" else text for text, style in spans
        )
        print("  " + rendered)

    print("\n══ 4. 解析器的规模" + "═" * 34)
    print(f"  认下来的是 {len(['', '0', '2', '1;36', '38;5;114', '38;5;174'])} 档 SGR 参数，")
    print("  其他 CSI 序列整段丢弃；不认识的颜色档（256 色 / truecolor）落到 `unknown(...)`，")
    print("  由绘制侧退成 `PLAIN` 或按最近似的一档收下 —— 没有终端模拟器那套网格与光标。")


if __name__ == "__main__":
    main()
