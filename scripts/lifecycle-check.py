#!/usr/bin/env python3
"""拿 docs/lifecycle.md 的 mermaid 图与它的证据表对账。

**定位：只守指称完整性，不守语义正确性。** 这个脚本不知道 `boot --> load` 这条边画得对
不对，只知道它的两端都有人认领、每条 `文件:行号` 都还指得到一个真实符号。**通过 ≠ 图是
对的** —— 图是否如实描述运行时，只能靠人读。

**天花板声明：不认识的 mermaid 构造一律报红，绝不静默跳过。** 解析器漏掉一个节点会伪装成
「图里有、表里没有」的假差异，那比没有护栏更糟；所以任何一行既不是方向声明、又不是认识的
节点/边，都进失败清单，让解析器的边界可见。

方言、虚实判据与**七条对账规则的全部内容**住在 `docs/lifecycle.md` §1：这里不复述，只指过去
（约定只有那一处，两份必然漂移）。

用法：

    python3 scripts/lifecycle-check.py [docs/lifecycle.md] [--list] [--strict]

退出码：`0` 通过 / `1` 有失败 / `2` 用法错。行号漂移只算提示（`--strict` 才计失败）——
行号是结构性易漂的量：文件顶部插一个 `use`，下面所有证据都 +1，而图与代码其实没脱节。
"""

from __future__ import annotations

import os
import re
import sys
from typing import NamedTuple

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_DOC = os.path.join("docs", "lifecycle.md")

USAGE = "usage: lifecycle-check.py [docs/lifecycle.md] [--list] [--strict]"

# 证据表的「符号」列只用这批稳定的写法；`impl` 与 `macro_rules!` 不算（书写形态不统一）。
SYMBOL_KINDS = ("fn", "struct", "enum", "trait", "const", "static", "mod", "type", "union")

NODE_ID = re.compile(r"^[a-z][a-z0-9_]*$")
FLOWCHART_HEAD = re.compile(r"^(flowchart|graph)\s+(TD|TB|BT|LR|RL)$")
SEQUENCE_HEAD = re.compile(r"^sequenceDiagram$")
NODE_DEF = re.compile(r"^([A-Za-z_]\w*)\s*([\[{])(.*)([\]}])$")
EDGE = re.compile(r"^([A-Za-z_]\w*)\s*(-->|-\.->)\s*(?:\|([^|]*)\|\s*)?([A-Za-z_]\w*)$")
FENCE_HEAD = re.compile(r"^\s*(`{3,}|~{3,})\s*(.*)$")
STRING_LITERAL = re.compile(r'"[^"]*"')
EVIDENCE = re.compile(r"([\w./-]+):(\d+)(?:-(\d+))?")
SYMBOL = re.compile(r"\b(" + "|".join(SYMBOL_KINDS) + r")\s+([A-Za-z_][A-Za-z0-9_]*)")
NODE_SECTION = re.compile(r"^###\s*图\s*(\d+)\s*的节点\s*$")
EDGE_SECTION = re.compile(r"^###\s*图\s*(\d+)\s*的边\s*$")
HEADING = re.compile(r"^#{1,6}(\s|$)")


class Block(NamedTuple):
    """一个 mermaid 围栏块。`index` 从 1 起，与正文的「图 N」对得上。"""

    index: int
    first_line: int
    lines: list[str]


class Graph(NamedTuple):
    nodes: dict[str, int]
    edges: list[tuple[str, str, int]]
    problems: list[str]


class Evidence(NamedTuple):
    raw: str
    symbols: str


def read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def strip_strings(line: str) -> str:
    """摘掉引号字符串。

    必须在剥 `%%` 注释**之前**做 —— 否则 `id["100%%"]` 会被当成注释截断。
    """
    return STRING_LITERAL.sub('""', line)


def mermaid_blocks(text: str) -> list[Block]:
    """按围栏切出 mermaid 块（认 ``` 与 ~~~ 两种，info string 允许跟参数）。"""
    lines = text.splitlines()
    blocks: list[Block] = []
    i = 0
    while i < len(lines):
        head = FENCE_HEAD.match(lines[i])
        if head and head.group(2).strip().startswith("mermaid"):
            fence = head.group(1)
            close = re.compile(r"^\s*" + re.escape(fence[0]) + rf"{{{len(fence)},}}\s*$")
            start = i + 1
            j = start
            while j < len(lines) and not close.match(lines[j]):
                j += 1
            blocks.append(Block(len(blocks) + 1, start + 1, lines[start:j]))
            i = j + 1
        else:
            i += 1
    return blocks


def parse_flowchart(block: Block) -> Graph:
    """行级状态机：认得的行收进节点/边，其余一律进 problems。"""
    nodes: dict[str, int] = {}
    edges: list[tuple[str, str, int]] = []
    problems: list[str] = []
    for offset, raw in enumerate(block.lines):
        lineno = block.first_line + offset
        line = strip_strings(raw).split("%%")[0].strip()
        if not line:
            continue
        if FLOWCHART_HEAD.match(line):
            continue
        node = NODE_DEF.match(line)
        if node:
            name, open_shape, label, close_shape = node.groups()
            if (open_shape, close_shape) not in (("[", "]"), ("{", "}")):
                problems.append(f"第 {lineno} 行：不认识的节点形状 `{line}`")
                continue
            if any(ch in label for ch in "[]{}"):
                problems.append(f"第 {lineno} 行：节点标签里套了形状括号 `{line}`")
                continue
            if not NODE_ID.match(name):
                problems.append(
                    f"第 {lineno} 行：节点 id `{name}` 不合约定（限 [a-z][a-z0-9_]*）"
                )
                continue
            if name in nodes:
                problems.append(
                    f"第 {lineno} 行：节点 `{name}` 重复定义（第 {nodes[name]} 行已经定义过）"
                )
                continue
            nodes[name] = lineno
            continue
        edge = EDGE.match(line)
        if edge:
            edges.append((edge.group(1), edge.group(4), lineno))
            continue
        problems.append(f"第 {lineno} 行：不认识的 mermaid 构造 `{line}`")
    return Graph(nodes, edges, problems)


def table_rows(text: str, section: re.Pattern[str]) -> dict[int, list[list[str]]]:
    """按 `### 图 N 的…` 节扫出表格的数据行：图号 → 每行的单元格。

    节点表与边表共用这一遍扫描 —— 它们只在「取哪一列」上不同。
    """
    tables: dict[int, list[list[str]]] = {}
    current: int | None = None
    for line in text.splitlines():
        match = section.match(line)
        if match:
            current = int(match.group(1))
            tables.setdefault(current, [])
            continue
        if HEADING.match(line):
            current = None
            continue
        if current is None:
            continue
        row = line.strip()
        if not row.startswith("|"):
            continue
        cells = [cell.strip() for cell in row.strip("|").split("|")]
        if not cells or cells[0].startswith(("节点", "边")) or set(cells[0]) <= set("-: "):
            continue
        tables[current].append(cells)
    return tables


def parse_node_tables(text: str) -> dict[int, dict[str, Evidence]]:
    """`### 图 N 的节点` 下的表：图号 → 节点 id → 证据。"""
    return {
        index: {
            cells[0].strip("`").strip(): Evidence(cells[2], cells[3])
            for cells in rows
            if len(cells) >= 4 and cells[0].strip("`").strip()
        }
        for index, rows in table_rows(text, NODE_SECTION).items()
    }


def parse_edge_tables(text: str) -> dict[int, list[str]]:
    """`### 图 N 的边` 下的表：图号 → 边的原文（第一列）。"""
    return {
        index: [cells[0] for cells in rows]
        for index, rows in table_rows(text, EDGE_SECTION).items()
    }


def within(start: str, end: str, lineno: int) -> bool:
    """证据给的行号（`a` 或 `a-b`）是否罩住真实行号 `lineno`。"""
    return int(start) <= lineno <= int(end or start)


def find_symbol(path: str, kind: str, name: str) -> int | None:
    """符号在文件里的真实行号（1 起），找不到返回 None。按词边界找，不搜裸名字。"""
    pattern = re.compile(rf"\b{kind}\s+{name}\b")
    for lineno, line in enumerate(read(path).splitlines(), 1):
        if pattern.search(line):
            return lineno
    return None


def resolve_evidence(
    root: str, node: str, ev: Evidence, problems: list[str], hints: list[str]
) -> int:
    """核一条证据：文件在不在、行数够不够、符号还在不在、行号漂没漂。返回点名的文件数。"""
    refs = EVIDENCE.findall(ev.raw)
    if not refs:
        problems.append(f"`{node}` 的证据里没有 `路径:行号`：{ev.raw}")
        return 0
    symbols = SYMBOL.findall(ev.symbols)
    if not symbols and ev.symbols.strip() not in {"", "-", "—", "–"}:
        # 符号列留空或写 `—` = 这个节点没有稳定符号可用（`refuse`、`env`、`rt`、`kind` 就是），
        # 那不是失败；写了东西却认不出「关键词 + 名字」才报。
        problems.append(f"`{node}` 的符号列里没有可核对的符号：{ev.symbols}")
    for kind, name in symbols:
        hit: tuple[str, str, str, int] | None = None
        for path, start, end in refs:
            full = os.path.join(root, path)
            if not os.path.isfile(full):
                continue
            where = find_symbol(full, kind, name)
            if where is None:
                continue
            # 证据给的常是「这个节点在哪几段代码」，符号的定义处在另一段里也正常；
            # 优先认那个**包含**符号真实行号的区间，全都装不下才是漂移信号（C9）。
            if within(start, end, where):
                hit = (path, start, end, where)
                break
            if hit is None:
                hit = (path, start, end, where)
        if hit is None:
            problems.append(f"`{node}`：符号 `{kind} {name}` 不在证据点名的文件里（改名了？）")
            continue
        path, start, end, where = hit
        if not within(start, end, where):
            span = f"{start}-{end}" if end else start
            hints.append(f"{path}:{span} 的 `{kind} {name}` 现在在 {where} 行")
    return len(refs)


def check_all_refs(root: str, text: str, problems: list[str]) -> None:
    """C1：全文里每一处 `路径:行号` 都要指得到一个存在的文件、且行号不越界。

    扫的是**全文**，不是只有附录的证据表 —— §7 与各节正文里引的证据同样算数。
    """
    for path, start, end in sorted(set(EVIDENCE.findall(text))):
        full = os.path.join(root, path)
        if not os.path.isfile(full):
            problems.append(f"文件不存在：`{path}`（正文或证据表里点名了它）")
            continue
        total = len(read(full).splitlines())
        if int(end or start) > total:
            problems.append(f"`{path}:{start}-{end}` 超出文件行数（共 {total} 行）")


def check_readme(root: str, doc_path: str, problems: list[str]) -> None:
    """C3：文档仍被 README.md 引用（「文档」表或「架构」节任一命中即可）。"""
    readme = os.path.join(root, "README.md")
    rel = os.path.relpath(doc_path, root).replace(os.sep, "/")
    if not os.path.isfile(readme):
        problems.append(f"README.md 不存在：无法核对 `{rel}` 的引用")
        return
    if not re.search(re.escape(rel), read(readme)):
        problems.append(f"README.md 里找不到指向 `{rel}` 的链接（文档成了孤儿）")


def main(argv: list[str]) -> int:
    doc = None
    list_only = False
    strict = False
    for arg in argv[1:]:
        if arg == "--list":
            list_only = True
        elif arg == "--strict":
            strict = True
        elif arg.startswith("-"):
            print(f"{USAGE}\n未知选项：{arg}", file=sys.stderr)
            return 2
        elif doc is None:
            doc = arg
        else:
            print(f"{USAGE}\n多余的参数：{arg}", file=sys.stderr)
            return 2

    doc_path = os.path.abspath(doc) if doc else os.path.join(REPO_ROOT, DEFAULT_DOC)
    if not os.path.isfile(doc_path):
        print(f"FAIL\n  - 文档不存在：{doc_path}")
        return 1
    root = os.path.dirname(os.path.dirname(doc_path))

    text = read(doc_path)
    problems: list[str] = []
    hints: list[str] = []
    graphs: dict[int, Graph] = {}
    for block in mermaid_blocks(text):
        head = next((line.strip() for line in block.lines if line.strip()), "")
        if SEQUENCE_HEAD.match(head):
            # 图 3 是 sequenceDiagram：它没有「节点集合」这回事，不进 C4/C5。
            continue
        if not FLOWCHART_HEAD.match(head):
            problems.append(
                f"第 {block.first_line} 行：不认识的图类型 `{head}`（只认 flowchart / sequenceDiagram）"
            )
            continue
        graph = parse_flowchart(block)
        graphs[block.index] = graph
        problems.extend(graph.problems)

    tables = parse_node_tables(text)
    edge_tables = parse_edge_tables(text)
    files: set[str] = set()
    symbols_found: set[tuple[str, str]] = set()
    for index, graph in graphs.items():
        table = tables.get(index)
        if table is None:
            problems.append(f"图 {index} 没有对应的节点证据表（`### 图 {index} 的节点`）")
            continue
        for name in sorted(set(graph.nodes) - set(table)):
            problems.append(f"图 {index} 里有、证据表没有的节点：`{name}`")
        for name in sorted(set(table) - set(graph.nodes)):
            problems.append(f"图 {index} 证据表有、图里没有的节点：`{name}`（名字对不上？）")
        for name in sorted(set(graph.nodes) & set(table)):
            ev = table[name]
            resolve_evidence(root, name, ev, problems, hints)
            files.update(path for path, _, _ in EVIDENCE.findall(ev.raw))
            symbols_found.update(SYMBOL.findall(ev.symbols))
        for src, dst, lineno in graph.edges:
            for endpoint in (src, dst):
                if endpoint not in graph.nodes:
                    problems.append(
                        f"图 {index} 第 {lineno} 行的边 `{src} --> {dst}`："
                        f"`{endpoint}` 没有显式定义（本仓库约定每个节点先定义一次）"
                    )
    check_all_refs(root, text, problems)
    check_readme(root, doc_path, problems)

    diagram_nodes = sum(len(graph.nodes) for graph in graphs.values())
    table_nodes = sum(len(table) for table in tables.values())
    diagram_edges = sum(len(graph.edges) for graph in graphs.values())
    table_edges = sum(len(rows) for rows in edge_tables.values())

    print(f"doc:      {os.path.relpath(doc_path)}")
    if list_only:
        print("nodes:")
        for index in sorted(graphs):
            for name in sorted(graphs[index].nodes):
                print(f"  图 {index} `{name}`（第 {graphs[index].nodes[name]} 行）")
        print("edges:")
        for index in sorted(graphs):
            for src, dst, lineno in graphs[index].edges:
                print(f"  图 {index} `{src} --> {dst}`（第 {lineno} 行）")
        print("evidence:")
        for index in sorted(tables):
            print(f"  图 {index}: {', '.join(sorted(tables[index]))}")
        return 0

    print(f"nodes:    {diagram_nodes} in diagram, {table_nodes} in evidence table")
    print(f"edges:    {diagram_edges} in diagram, {table_edges} in evidence table")
    print(
        f"evidence: {len(files)} files, {len(symbols_found)} symbols found, "
        f"{len(hints)} line drift"
    )
    if hints:
        print("提示（不计失败）：")
        for hint in hints:
            print(f"  - {hint}")
    if problems or (strict and hints):
        print("FAIL")
        for problem in problems:
            print(f"  - {problem}")
        if strict:
            for hint in hints:
                print(f"  - 行号漂移（--strict）：{hint}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
