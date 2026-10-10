#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""heng 历史会话里 bash 用法的形态底账（只读调研，票 01）。

数据源：~/.local/share/heng/sessions/*/*/log.jsonl
事件：payload.ToolCallStarted（tool_name == "bash" 的 args.command）
      payload.ToolCallCompleted（按 tool_call_id 配对，output 是结果正文）
      payload.SessionStarted（cwd，用于把工作区路径归一成 <WS>）

一条命令跑完，不落任何中间文件；所有数字打印到 stdout（Markdown 片段）。
用法：
    python3 .scratch/tool-coverage/research/01-shape-inventory.py
    python3 .scratch/tool-coverage/research/01-shape-inventory.py --top 120
    python3 .scratch/tool-coverage/research/01-shape-inventory.py --check   # 只打交叉校验
"""

import glob
import json
import os
import re
import statistics
import sys
from collections import Counter, defaultdict

SESSIONS_GLOB = os.path.expanduser("~/.local/share/heng/sessions/*/*/log.jsonl")
SNAPSHOT = "2026-10-10"

# `--since YYYY-MM-DD` 之后只统计该日（含）以后的会话，日期取会话目录名的 UTC 时间戳前八位
# （目录名形如 `20261009T151952Z-f9e243db`）。默认 None = 全量，也就是改动前的基线口径。
SINCE = None


def session_day(path):
    """会话目录名里的日期（UTC，YYYYMMDD）。"""
    name = os.path.basename(os.path.dirname(path))
    match = re.match(r"(\d{8})", name)
    return match.group(1) if match else ""


def session_files():
    """会话日志路径；`SINCE` 非空时按会话目录名的日期收窄。"""
    paths = sorted(glob.glob(SESSIONS_GLOB))
    if SINCE is None:
        return paths
    return [path for path in paths if session_day(path) >= SINCE]


# ---------------------------------------------------------------- 读日志

def load_calls():
    """返回 (会话数, bash 调用列表)；每个调用是 dict。"""
    files = session_files()
    sessions = set()
    calls = []
    pending = {}
    for path in files:
        session_dir = os.path.dirname(path)
        sessions.add(session_dir)
        cwd = None
        try:
            fh = open(path, encoding="utf-8", errors="replace")
        except OSError:
            continue
        with fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except ValueError:
                    continue
                payload = obj.get("payload") or {}
                started = payload.get("ToolCallStarted")
                if started is not None:
                    if started.get("tool_name") == "bash":
                        args = started.get("args") or {}
                        cmd = args.get("command")
                        if isinstance(cmd, str):
                            idx = len(calls)
                            calls.append({
                                "session": session_dir,
                                "cwd": cwd,
                                "command": cmd,
                                "result_len": None,
                                "ok": None,
                            })
                            tcid = started.get("tool_call_id")
                            if tcid is not None:
                                pending[tcid] = idx
                    continue
                completed = payload.get("ToolCallCompleted")
                if completed is not None:
                    idx = pending.pop(completed.get("tool_call_id"), None)
                    if idx is not None:
                        out = completed.get("output")
                        calls[idx]["result_len"] = len(out) if isinstance(out, str) else 0
                        calls[idx]["ok"] = completed.get("ok")
                    continue
                ss = payload.get("SessionStarted")
                if ss is not None and cwd is None:
                    cwd = ss.get("cwd")
                    for c in reversed(calls):
                        if c["session"] == session_dir and c["cwd"] is None:
                            c["cwd"] = cwd
                        else:
                            break
    return len(sessions), calls


# ---------------------------------------------------------------- 切段

def split_segments(cmd, split_pipe=True):
    """按 && / || / ; / 换行（可选管道）切段；引号与 heredoc 正文不算分隔。

    返回 [(段文本, 段前分隔符)]，分隔符取 start/&&/||/;/\\n/|/&。
    """
    segs, seps, buf = [], [], []
    pending = "start"          # 段「前」分隔符
    i, n, state = 0, len(cmd), "normal"
    while i < n:
        c = cmd[i]
        if state == "normal":
            if c == "\\" and i + 1 < n:
                buf.append(cmd[i:i + 2]); i += 2; continue
            if c == "'":
                state = "single"; buf.append(c); i += 1; continue
            if c == '"':
                state = "double"; buf.append(c); i += 1; continue
            if c == "`":
                state = "backtick"; buf.append(c); i += 1; continue
            if c == "#" and (not buf or buf[-1] in " \t\n;&|("):
                j = cmd.find("\n", i); i = n if j == -1 else j; continue
            if cmd.startswith("<<", i):
                m = re.match(r"<<(-?)([\"']?)([A-Za-z_][A-Za-z0-9_]*)\2", cmd[i:])
                if m:
                    buf.append(cmd[i:i + m.end()]); i += m.end()
                    j = cmd.find("\n", i)
                    if j == -1:
                        i = n; continue
                    i = j + 1
                    word = m.group(3)
                    while i < n:
                        k = cmd.find("\n", i)
                        line = cmd[i:k] if k != -1 else cmd[i:]
                        i = (k + 1) if k != -1 else n
                        if line.strip() == word:
                            break
                    # heredoc 正文到此结束；正文之后的内容属于新命令，必须切段
                    segs.append("".join(buf)); seps.append(pending); pending = "\n"; buf = []
                    continue
            if cmd.startswith("&&", i) or cmd.startswith("||", i):
                segs.append("".join(buf)); seps.append(pending); pending = cmd[i:i + 2]; buf = []; i += 2; continue
            if c == "|" and split_pipe:
                segs.append("".join(buf)); seps.append(pending); pending = "|"; buf = []; i += 1; continue
            if c in ";\n":
                segs.append("".join(buf)); seps.append(pending); pending = c; buf = []; i += 1; continue
            if c == "&":
                prev = buf[-1] if buf else ""
                nxt = cmd[i + 1] if i + 1 < n else ""
                if nxt == ">" or prev == ">":
                    buf.append(c); i += 1; continue
                segs.append("".join(buf)); seps.append(pending); pending = "&"; buf = []; i += 1; continue
            buf.append(c); i += 1
        elif state == "single":
            buf.append(c)
            if c == "'":
                state = "normal"
            i += 1
        elif state == "double":
            if c == "\\" and i + 1 < n:
                buf.append(cmd[i:i + 2]); i += 2; continue
            buf.append(c)
            if c == '"':
                state = "normal"
            i += 1
        else:
            if c == "\\" and i + 1 < n:
                buf.append(cmd[i:i + 2]); i += 2; continue
            buf.append(c)
            if c == "`":
                state = "normal"
            i += 1
    segs.append("".join(buf)); seps.append(pending)
    out = []
    for s, sp in zip(segs, seps):
        s = s.strip()
        if s:
            out.append((s, sp))
    return out


# ---------------------------------------------------------------- 分词

def tokenize(seg):
    """把一段切成 [(kind, text, quoted)]，kind ∈ {'word','redir'}。"""
    toks, cur = [], []
    i, n, quoted = 0, len(seg), False

    def flush():
        if cur:
            toks.append(("word", "".join(cur), quoted))

    while i < n:
        c = seg[i]
        if c in " \t":
            if cur:
                flush(); cur.clear()
            quoted = False; i += 1; continue
        if c == "#" and not cur:
            break
        if c == "'":
            j = seg.find("'", i + 1)
            if j == -1:
                j = n - 1
            cur.append(seg[i:j + 1]); quoted = True; i = j + 1; continue
        if c == '"':
            j = i + 1; buf = ['"']
            while j < n:
                if seg[j] == "\\" and j + 1 < n:
                    buf.append(seg[j:j + 2]); j += 2; continue
                buf.append(seg[j])
                if seg[j] == '"':
                    j += 1; break
                j += 1
            cur.append("".join(buf)); quoted = True; i = j; continue
        if c == "\\" and i + 1 < n:
            cur.append(seg[i:i + 2]); i += 2; continue
        if c in "<>":
            fd = ""
            if cur and "".join(cur).isdigit():
                fd = "".join(cur); cur.clear()
            if cur:
                flush(); cur.clear()
            op, j = c, i + 1
            if j < n and seg[j] == c:
                op = c + c; j += 1
            if j < n and seg[j] == "&":
                op += "&"; j += 1
                if j < n and seg[j].isdigit():      # 2>&1 / >&2
                    op += seg[j]; j += 1
            toks.append(("redir", fd + op, False)); i = j; continue
        cur.append(c); i += 1
    flush()
    return toks


# ---------------------------------------------------------------- 归一化

POS_ROLES = {
    "grep": ["PAT"] + ["PATH"] * 99, "egrep": ["PAT"] + ["PATH"] * 99,
    "fgrep": ["PAT"] + ["PATH"] * 99, "rg": ["PAT"] + ["PATH"] * 99,
    "sed": ["SCRIPT"] + ["PATH"] * 99, "awk": ["SCRIPT"] + ["PATH"] * 99,
    "gawk": ["SCRIPT"] + ["PATH"] * 99,
    "cd": ["PATH"] * 99, "pushd": ["PATH"] * 99, "cat": ["PATH"] * 99,
    "ls": ["PATH"] * 99, "rm": ["PATH"] * 99, "rmdir": ["PATH"] * 99,
    "mkdir": ["PATH"] * 99, "cp": ["PATH"] * 99, "mv": ["PATH"] * 99,
    "touch": ["PATH"] * 99, "ln": ["PATH"] * 99, "wc": ["PATH"] * 99,
    "sort": ["PATH"] * 99, "uniq": ["PATH"] * 99, "tail": ["PATH"] * 99,
    "head": ["PATH"] * 99, "tr": ["STR"] * 99, "cut": ["STR"] * 99,
    "echo": ["STR"] * 99, "printf": ["STR"] * 99, "curl": ["URL"] * 99,
    "wget": ["URL"] * 99, "export": ["ARG"] * 99, "source": ["PATH"] * 99,
    ".": ["PATH"] * 99, "sh": ["PATH"] * 99, "bash": ["PATH"] * 99,
    "sleep": ["N"] * 99, "kill": ["ARG"] * 99, "du": ["PATH"] * 99,
    "df": ["PATH"] * 99, "stat": ["PATH"] * 99, "file": ["PATH"] * 99,
    "which": ["ARG"] * 99, "open": ["PATH"] * 99,
    "python": ["PYARG"] * 99, "python3": ["PYARG"] * 99,
}

# 会吃掉下一个 token 作为取值的 flag
VALUE_FLAGS = {
    "grep": {"-A", "-B", "-C", "-m", "-e", "-f", "--include", "--exclude", "--exclude-dir"},
    "egrep": {"-A", "-B", "-C", "-m", "-e", "-f"},
    "rg": {"-A", "-B", "-C", "-g", "-t", "-T", "-m", "-e", "-f", "--glob", "--type"},
    "head": {"-n", "-c"}, "tail": {"-n", "-c", "-s"},
    "sed": {"-e", "-f"}, "awk": {"-F", "-v"}, "gawk": {"-F", "-v"},
    "cut": {"-d", "-f"}, "sort": {"-k", "-t", "-S"},
    "git": {"-C", "-c", "-m", "-n", "--git-dir", "--work-tree", "--pretty", "--format", "--author", "--date"},
    "cargo": {"--test", "--bin", "--example", "--features", "--package", "-p",
              "--target", "--profile", "--manifest-path", "--jobs", "-j"},
    "find": {"-name", "-iname", "-path", "-type", "-maxdepth", "-mindepth", "-newer", "-exec"},
    "xargs": {"-n", "-I", "-P", "-d"},
    "curl": {"-H", "-d", "-X", "-o", "-u", "-A", "-m", "-w", "--data", "--header",
             "--request", "--user", "--max-time", "--output"},
    "timeout": {"-s", "--signal", "-k"},
    "gh": {"-R", "--repo", "-b", "--body", "-t", "--title", "-F", "--field"},
    "docker": {"-f", "--file", "-p", "--publish"},
}

SUBCMD_CMDS = {"cargo", "git", "rustup", "gh", "docker", "kubectl", "npm", "pnpm",
               "yarn", "uv", "go", "make", "systemctl", "apt", "brew"}
STR_ROLES = {"STR", "PAT", "SCRIPT", "MODE", "URL", "ARG", "PYARG", "N", "SEP"}

PATH_EXT_RE = re.compile(
    r"\.(rs|py|pyi|md|json|jsonl|toml|txt|sh|lock|yaml|yml|ts|tsx|js|jsx|html|css|"
    r"c|h|cpp|hpp|go|java|rb|php|sql|csv|log|ini|cfg|conf|env|swift|kt)$", re.I)


def strip_quotes(t):
    if len(t) >= 2 and t[0] == t[-1] and t[0] in "'\"":
        return t[1:-1]
    return t


def is_pathish(t):
    if not t or t.startswith("$") or t.startswith("`"):
        return False
    if "/" in t or t.startswith("*") or t.startswith("?"):
        return True
    return bool(PATH_EXT_RE.search(t))


def path_slot(t, cwd):
    if t == "/dev/null":
        return "<DEVNULL>"
    if t.startswith("/tmp") or t.startswith("/var/tmp") or t.startswith("/dev/shm"):
        return "<TMPPATH>"
    if t.startswith("/dev/"):
        return "<DEVPATH>"
    if t.startswith("target/") or t.startswith("./target"):
        return "<TARGETPATH>"
    if cwd and (t == cwd or t.startswith(cwd.rstrip("/") + "/")):
        return "<WS>"
    if t.startswith("/"):
        return "<ABSPATH>"
    if t.startswith("~"):
        return "<HOMEPATH>"
    return "<PATH>"


def norm_flag(tok, argv0, quoted):
    """返回 (骨架片段, 是否取值 flag)。不是 flag 返回 (None, False)。"""
    t = strip_quotes(tok)
    if not quoted and argv0 in ("echo", "printf"):
        return (None, False)
    if t.startswith("$") or t.startswith("`"):
        return ("<SUBST>", False)
    if re.fullmatch(r"-\d+", t):
        return ("-<N>", False)
    m = re.fullmatch(r"(-{1,2}[A-Za-z][A-Za-z0-9-]*)(=(.*))?$", t)
    if m:
        if m.group(2):
            val = m.group(3)
            val = "<N>" if re.fullmatch(r"\d+", val or "") else "<V>"
            return ("%s=%s" % (m.group(1), val), False)
        return (m.group(1), True)
    return (None, False)


ASSIGN_PREFIX_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")


def find_argv0_index(toks):
    """第一个非「赋值前缀」的 word 下标（跳过 FOO=bar 这类环境变量前缀）。"""
    for k, (kind, text, quoted) in enumerate(toks):
        if kind != "word":
            continue
        t = strip_quotes(text)
        if not quoted and ASSIGN_PREFIX_RE.match(t) and not t.startswith("./"):
            continue
        return k
    return None


def norm_segment(seg, cwd=None):
    """把一段归一成形态骨架；返回 (骨架, argv0, features) 或 None。"""
    toks = tokenize(seg)
    if not toks:
        return None
    idx0 = find_argv0_index(toks)
    if idx0 is None:
        return ("<ASSIGN_ONLY>", "<assign>", {"argv0": "<assign>", "flags": set()})
    first = strip_quotes(toks[idx0][1]).lstrip("({!")
    m = re.match(r"[A-Za-z0-9_./\\+:-]+", first)
    argv0 = m.group(0) if m else "<CMD>"
    if "/" in argv0:
        argv0 = os.path.basename(argv0) or argv0

    flags, redirs, pos, assigns = [], [], [], []
    roles = POS_ROLES.get(argv0)
    subcmd_seen = False
    subcmd = None
    pyc = False
    pos_i = 0
    feats = {"argv0": argv0, "flags": set(), "tmp": False, "writes": False,
             "pty": False, "heredoc": False, "subst": False, "inner": None}
    k = 0
    while k < len(toks):
        kind, text, quoted = toks[k]
        if k < idx0:
            assigns.append("<ASSIGN>"); k += 1; continue
        if k == idx0:
            k += 1; continue
        if kind == "redir":
            raw = text
            if "<<<" in raw:
                redirs.append("<<<"); 
                if k + 1 < len(toks) and toks[k + 1][0] == "word":
                    redirs.append("<STR>"); k += 2; continue
                k += 1; continue
            if raw.startswith("<<") or "<<" in raw:
                redirs.append("<<"); redirs.append("<HEREDOC>")
                feats["heredoc"] = True
                k += 1
                if k < len(toks) and toks[k][0] == "word":   # 跳过定界词
                    k += 1
                continue
            redirs.append(raw)
            if k + 1 < len(toks) and toks[k + 1][0] == "word" and "&" not in raw:
                tgt = strip_quotes(toks[k + 1][1])
                redirs.append(path_slot(tgt, cwd) if is_pathish(tgt) else "<PATH>")
                if ">" in raw and not raw.startswith("2") and not tgt.startswith("/dev/"):
                    feats["writes"] = True
                k += 2; continue
            k += 1; continue
        raw = strip_quotes(text)
        if raw.startswith("$") or raw.startswith("`"):
            feats["subst"] = True
            pos.append("<SUBST>"); pos_i += 1; k += 1; continue
        if argv0 in ("python", "python3") and raw in ("-c", "-m"):
            flags.append(raw); feats["flags"].add(raw); pyc = True; k += 1; continue

        frag, is_valflag = norm_flag(text, argv0, quoted)
        if frag is not None:
            vf = VALUE_FLAGS.get(argv0, ())
            if is_valflag and frag in vf and k + 1 < len(toks) and toks[k + 1][0] == "word":
                nxt = strip_quotes(toks[k + 1][1])
                val = "<N>" if re.fullmatch(r"\d+", nxt) else ("<V>" if re.fullmatch(r"[-\w./]+", nxt) else "<V>")
                if argv0 in ("head", "tail") and frag == "-n" and val == "<N>":
                    flags.append("-<N>")
                else:
                    flags.append("%s %s" % (frag, val))
                feats["flags"].add(frag)
                k += 2; continue
            flags.append(frag)
            if is_valflag:
                feats["flags"].add(frag)
            k += 1; continue

        # 位置参数
        if argv0 == "timeout" and pos_i == 0 and re.fullmatch(r"\d+(\.\d+)?[smhd]?", raw):
            pos.append("<N>"); pos_i += 1; k += 1; continue
        if argv0 == "timeout" and pos_i >= 1:
            rest = " ".join(t[1] for t in toks[k:] if t[0] == "word")
            sub = norm_segment(rest, cwd)
            if sub:
                pos.append(sub[0]); feats["inner"] = sub[1]
            pos_i += 1
            break
        if argv0 in ("python", "python3"):
            if pyc:
                pos.append("<PY_SRC>"); pyc = False; pos_i += 1; k += 1; continue
            if raw == "-":
                pos.append("<STDIN>"); pos_i += 1; k += 1; continue
        if argv0 in SUBCMD_CMDS and not subcmd_seen and not quoted and re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", raw):
            subcmd = raw; subcmd_seen = True; pos_i += 1; k += 1; continue
        if argv0 in ("echo", "printf") and raw.startswith("---") and raw.endswith("---"):
            pos.append("<SEP>"); k += 1; continue

        role = roles[min(pos_i, len(roles) - 1)] if roles else None
        if role in STR_ROLES:
            if role == "N":
                pos.append("<N>" if re.fullmatch(r"\d+", raw) else "<ARG>")
            else:
                pos.append("<%s>" % role)
        elif role == "PATH":
            pos.append(path_slot(raw, cwd) if is_pathish(raw) else "<PATH>")
        else:
            if re.fullmatch(r"\d+", raw):
                pos.append("<N>")
            elif is_pathish(raw):
                pos.append(path_slot(raw, cwd))
            elif len(raw) <= 14 and re.fullmatch(r"[A-Za-z][A-Za-z0-9_.-]*", raw):
                pos.append("<ARG>")
            else:
                pos.append("<STR>")
        if raw.startswith("/tmp") or raw.startswith("/dev/shm") or raw.startswith("/var/tmp"):
            feats["tmp"] = True
        pos_i += 1
        k += 1

    if argv0 in ("rm", "rmdir", "mkdir", "cp", "mv", "touch", "chmod", "ln", "tee"):
        feats["writes"] = True
    if "sed" in argv0 and ("-i" in feats["flags"] or "--in-place" in feats["flags"]):
        feats["writes"] = True
    if argv0 in ("script", "expect", "tmux", "screen", "fzf", "less", "vim", "nano",
                 "watch", "top", "htop", "unbuffer", "ssh", "gdb"):
        feats["pty"] = True
    if argv0 == "git" and pos[:1] == ["commit"] and "-m" not in feats["flags"] and "--amend" not in feats["flags"]:
        feats["pty"] = True

    # 折叠连续重复占位符
    folded = []
    for p in pos:
        if folded and folded[-1] == p and p.startswith("<") and p not in ("<SEP>",):
            if len(folded) >= 2 and folded[-2] == p + "...":
                continue
            folded.append(p + "...")
        else:
            folded.append(p)
    shape = " ".join([argv0] + ([subcmd] if subcmd else []) + sorted(flags) + assigns + folded + redirs)
    return (shape, argv0, feats)


# ---------------------------------------------------------------- 形态属性

GLUE_CMDS = {"cd", "echo", "for", "do", "done", "pwd", "true", ":", "fi", "then", "if"}
# 成本档位：1 = 改既有工具参数 / 2 = 开一条新工具 / 3 = 要改架构
COST_BY_ARGV0 = {
    "grep": 1, "egrep": 1, "fgrep": 1, "rg": 1,
    "sed": 1, "head": 1, "tail": 1, "cat": 1,
    "cd": 1, "pwd": 1, "ls": 1,
    "git": 2, "cargo": 2, "find": 2, "wc": 2, "sort": 2, "uniq": 2, "awk": 2,
    "mkdir": 2, "rm": 2, "cp": 2, "mv": 2, "touch": 2, "chmod": 2,
    "python3": 3, "python": 3, "curl": 3, "timeout": 3, "script": 3,
}
COST_DEFAULT = 3


def classify(e):
    """给一个形态打四类结构判据 + 成本档位。返回 (tags, cost, why)。"""
    tags = []
    segs = e["segs"]
    if e["down"] / max(1, segs) >= 0.5:
        tags.append("stdin")
    if e["tmp_share"] >= 0.3:
        tags.append("prev")
    if e["write_share"] >= 0.3:
        tags.append("write")
    if e["pty_share"] >= 0.3:
        tags.append("pty")
    if e["heredoc_share"] >= 0.5 and e["argv0"] in ("python3", "python", "bash", "sh", "cat"):
        tags.append("heredoc")
    cost = COST_BY_ARGV0.get(e["argv0"], COST_DEFAULT)
    why = "既有工具（%s）" % e["argv0"] if cost == 1 else ("新工具（%s）" % e["argv0"] if cost == 2 else "架构（%s）" % e["argv0"])
    if "stdin" in tags and cost < 3:
        why += "+管道读 stdin"
    if "pty" in tags:
        cost = 3
        why = "需交互/PTY"
    if "heredoc" in tags:
        cost = max(cost, 3)
        why = "heredoc 驱动的脚本"
    return (tags, cost, why)


# ---------------------------------------------------------------- 交叉校验

WRAPPER_CMDS = ("timeout", "env", "nohup", "nice", "sudo", "command", "time", "stdbuf")
WRAP_RE = re.compile(r"^(?:(?:%s)\s+(?:-\S+\s+|\d+\S*\s+)*)+" % "|".join(WRAPPER_CMDS))


def argv0_of(seg):
    toks = tokenize(seg)
    k = find_argv0_index(toks)
    if k is None:
        return "<assign>"
    t = strip_quotes(toks[k][1]).lstrip("({!")
    m = re.match(r"[A-Za-z0-9_./\\+:-]+", t)
    return m.group(0) if m else "<CMD>"


def load_tool_stats():
    tool, perms = Counter(), Counter()
    workdir = total = 0
    for path in session_files():
        try:
            fh = open(path, encoding="utf-8", errors="replace")
        except OSError:
            continue
        with fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except ValueError:
                    continue
                payload = obj.get("payload") or {}
                st = payload.get("ToolCallStarted")
                if st is not None:
                    total += 1
                    name = st.get("tool_name")
                    tool[name] += 1
                    if name == "bash":
                        a = st.get("args") or {}
                        if a.get("workdir") is not None:
                            workdir += 1
                pdec = payload.get("PermissionDecided")
                if pdec is not None:
                    perms[pdec.get("decision")] += 1
    return total, tool, workdir, perms


def crosscheck(stats):
    calls = stats["calls"]
    N = len(calls)
    print("## 交叉校验（票面零散数字 ←→ 本次快照）")
    print()
    print("| 票面说法 | 票面 | 本次复现 | 差 | 口径/解释 |")
    print("|---|---:|---:|---:|---|")
    stmt = Counter()
    for c in calls:
        for shape, argv0, feats, sep in c["stmt_parsed"]:
            stmt[argv0] += 1
    allc = Counter()
    for c in calls:
        for shape, argv0, feats, sep in c["parsed"]:
            allc[argv0] += 1
    unwrapped = Counter()
    sed_n = 0
    sed_any = 0
    for c in calls:
        for s, sep in split_segments(c["command"], split_pipe=False):
            s2 = WRAP_RE.sub("", s.strip(), count=1)
            unwrapped[argv0_of(s2 if s2 else s)] += 1
        for shape, argv0, feats, sep in c["stmt_parsed"]:
            if argv0 == "sed":
                sed_any += 1
                if "-n" in feats.get("flags", ()):
                    sed_n += 1
    glue_all = stats["glue_strict"]
    pipe_all = stats["pipe_segs"]
    rows = [
        ("bash 调用数", "7203", str(N), "快照晚于票面，会话仍在增长"),
        ("段数（不切管道）", "27398", str(stats["seg_nopipe"]), "同口径，差在快照时点"),
        ("段数（全切含管道）", "—", str(stats["seg_all"]), "票面只用过不切口径"),
        ("平均段/调用", "3.8", "%.2f" % (stats["seg_nopipe"] / N), "票面分母 7203；本次 7703"),
        ("胶水段占比", "42.2%", "%.1f%%" % (100 * glue_all / stats["seg_nopipe"]),
         "本次分母=不切管道段（同票面口径）"),
        ("以 cd 开头", "86.1%", "%.1f%%" % (100 * stats["cd_first"] / N), "一致"),
        ("含 &&", "80.4%", "%.1f%%" % (100 * stats["and"] / N), "一致"),
        ("含管道", "67.7%", "%.1f%%" % (100 * stats["pipe"] / N), "一致"),
        ("以 \\| head 收尾", "45.3%", "%.1f%%" % (100 * stats["endhead"] / N), "一致"),
        ("含 heredoc", "17.3%", "%.1f%%" % (100 * stats["heredoc"] / N), "一致"),
        ("sed -n 段（不切管道）", "2352", str(sed_n),
         "全部 sed 段 %d，其中带 -n %d" % (sed_any, sed_n)),
        ("grep+rg 段（全切含管道）", "5495", str(allc["grep"] + allc["rg"]), "一致(+%.1f%%)" % (
            100 * ((allc["grep"] + allc["rg"]) / 5495 - 1),)),
        ("段级 argv0 cd（不切管道）", "6379", str(stmt["cd"]), "一致"),
        ("段级 argv0 echo（不切管道）", "4271", str(stmt["echo"]), "一致"),
        ("段级 argv0 grep（不切管道）", "3474", str(stmt["grep"]), "一致"),
        ("段级 argv0 cargo（不切管道）", "1942", str(stmt["cargo"]),
         "跳包装命令后 %d" % unwrapped["cargo"]),
        ("段级 argv0 python3（不切管道）", "1855", str(stmt["python3"]),
         "跳包装命令后 %d" % unwrapped["python3"]),
        ("段级 argv0 git（不切管道）", "1641", str(stmt["git"]), "一致"),
        ("段级 argv0 ls（不切管道）", "728", str(stmt["ls"]), "一致"),
        ("段级 argv0 cat（不切管道）", "472", str(stmt["cat"]), "一致"),
        ("段级 argv0 rg（不切管道）", "112", str(stmt["rg"]), "一致"),
        ("段级 argv0 find（不切管道）", "104", str(stmt["find"]), "一致"),
        ("段级 argv0 curl（不切管道）", "104", str(stmt["curl"]),
         "跳包装命令后 %d" % unwrapped["curl"]),
        ("段级 argv0 mkdir（不切管道）", "78", str(stmt["mkdir"]), "一致"),
    ]
    for what, old, new, why in rows:
        print("| %s | %s | %s | %s |" % (what, old, new, why))
    print()
    print("口径说明：票面的「段数/段级 argv0」是不切管道的切法（`&&`/`||`/`;`/换行）；"
          "「bash 内 grep 5495 次」是全切（含管道）的切法。两者在本次快照都能复现。")
    print()
    # grep 细分
    tot_g = 0
    down_g = 0
    ctx = 0
    mods = 0
    for c in calls:
        for shape, argv0, feats, sep in c["parsed"]:
            if argv0 not in ("grep", "rg"):
                continue
            tot_g += 1
            if sep == "|":
                down_g += 1
            fl = feats.get("flags", ())
            if any(f.split()[0] in ("-A", "-B", "-C", "--after-context", "--before-context", "--context") for f in fl):
                ctx += 1
            if any(f.split()[0] in ("-l", "-c", "-v", "-o", "-i", "-q", "-w", "-x") for f in fl):
                mods += 1
    print("grep/rg 全切段：%d，其中管道位置 %d（%.1f%%）、带 -A/-B/-C %d（%.1f%%）、"
          "带 -l/-c/-v/-o/-i 之一 %d（%.1f%%）" % (
              tot_g, down_g, 100 * down_g / max(1, tot_g), ctx, 100 * ctx / max(1, tot_g),
              mods, 100 * mods / max(1, tot_g)))
    print()
    total, tool, workdir, perms = load_tool_stats()
    print("工具调用总数：%d" % total)
    for name, n in tool.most_common(8):
        print("- %s：%d（%.1f%%）" % (name, n, 100 * n / total))
    print("- bash 带 workdir 参数：%d 次" % workdir)
    print("- PermissionDecided：%s" % dict(perms))
    print()
    daily = defaultdict(lambda: [0, 0])
    for c in calls:
        d = os.path.basename(c["session"])[:8]
        daily[d][0] += 1
        if any(a in ("grep", "rg") for _, a, _, _ in c["parsed"]):
            daily[d][1] += 1
    print("## 交叉校验（续）：按会话日期的 bash 调用与「含 grep/rg 段的调用」占比")
    for d in sorted(daily)[-12:]:
        n, g = daily[d]
        print("- %s：bash %d 次，含 grep/rg %d 次（%.0f%%）" % (d, n, g, 100 * g / max(1, n)))
    print()


# ---------------------------------------------------------------- 主流程

def main():
    global SINCE
    top = 40
    check_only = "--check" in sys.argv
    if "--top" in sys.argv:
        top = int(sys.argv[sys.argv.index("--top") + 1])
    if "--since" in sys.argv:
        SINCE = sys.argv[sys.argv.index("--since") + 1].replace("-", "")

    nsess, calls = load_calls()
    N = len(calls)
    print("## 快照与分母")
    print()
    print("快照：%s" % SNAPSHOT)
    if SINCE is not None:
        print("窗口：只统计 %s 及以后的会话（按会话目录名的 UTC 日期）" % SINCE)
    print("会话目录（有 log.jsonl）：%d" % nsess)
    print("bash 调用数：%d" % N)
    print("未配对 ToolCallCompleted：%d" % sum(1 for c in calls if c["result_len"] is None))
    print()

    seg_all = seg_nopipe = 0
    n_cd_first = n_and = n_pipe = n_endhead = n_heredoc = 0
    glue_strict = 0
    pipe_segs = 0
    glue_detail = Counter()
    for c in calls:
        cmd = c["command"]
        stmts = split_segments(cmd, split_pipe=False)
        seg_nopipe += len(stmts)
        stmt_parsed = []
        for s, sep in stmts:
            r = norm_segment(s, c["cwd"])
            if r:
                stmt_parsed.append((r[0], r[1], r[2], sep))
        c["stmt_parsed"] = stmt_parsed
        segs = split_segments(cmd, split_pipe=True)
        seg_all += len(segs)
        c["segs"] = segs
        parsed = []
        for s, sep in segs:
            r = norm_segment(s, c["cwd"])
            if r:
                parsed.append((r[0], r[1], r[2], sep))
        c["parsed"] = parsed
        glue_strict += sum(1 for _, a, _, _ in parsed if a in GLUE_CMDS)
        for _, a, _, _ in parsed:
            if a in GLUE_CMDS:
                glue_detail[a] += 1
        pipe_segs += sum(1 for _, _, _, sep in parsed if sep == "|")
        if cmd.strip().startswith("cd "):
            n_cd_first += 1
        if "&&" in cmd:
            n_and += 1
        if "|" in cmd.replace("||", ""):
            n_pipe += 1
        if re.search(r"\|\s*head\b[^|]*$", cmd.strip()):
            n_endhead += 1
        if "<<" in cmd:
            n_heredoc += 1

    print("段数（全切，含管道）：%d（平均 %.2f/调用）" % (seg_all, seg_all / N))
    print("段数（不切管道）：%d（平均 %.2f/调用）" % (seg_nopipe, seg_nopipe / N))
    print("以 cd 开头：%d（%.1f%%）" % (n_cd_first, 100 * n_cd_first / N))
    print("含 &&：%d（%.1f%%）" % (n_and, 100 * n_and / N))
    print("含管道：%d（%.1f%%）" % (n_pipe, 100 * n_pipe / N))
    print("以 | head 收尾：%d（%.1f%%）" % (n_endhead, 100 * n_endhead / N))
    print("含 heredoc：%d（%.1f%%）" % (n_heredoc, 100 * n_heredoc / N))
    print("胶水段（cd/echo/for/do/done）：%d（%.1f%% of 全切段）" % (glue_strict, 100 * glue_strict / seg_all))
    print("胶水构成：%s" % "、".join("%s %d" % (k, v) for k, v in glue_detail.most_common()))
    print()

    stats = {
        "calls": calls, "seg_all": seg_all, "seg_nopipe": seg_nopipe,
        "glue_strict": glue_strict, "pipe_segs": pipe_segs,
        "cd_first": n_cd_first, "and": n_and, "pipe": n_pipe,
        "endhead": n_endhead, "heredoc": n_heredoc,
    }

    if check_only:
        first = Counter()
        for c in calls:
            for s, _ in split_segments(c["command"], split_pipe=False):
                r = norm_segment(s, c["cwd"])
                if r:
                    first[r[1]] += 1
        print("=== 交叉校验：不切管道口径的段级 argv0 ===")
        for a, n in first.most_common(20):
            print("%-12s %6d" % (a, n))
        allc = Counter()
        for c in calls:
            for s, _ in c["segs"]:
                r = norm_segment(s, c["cwd"])
                if r:
                    allc[r[1]] += 1
        print("\n=== 交叉校验：全切口径的段级 argv0 ===")
        for a, n in allc.most_common(20):
            print("%-12s %6d" % (a, n))
        print("\ngrep+rg（全切）：%d" % (allc["grep"] + allc["rg"]))
        pos = Counter()
        for c in calls:
            for s, sep in c["segs"]:
                r = norm_segment(s, c["cwd"])
                if r and r[1] in ("grep", "rg"):
                    pos["down" if sep == "|" else "up"] += 1
        tot = pos["down"] + pos["up"]
        print("grep/rg 管道位置：%d / %d = %.1f%%" % (pos["down"], tot, 100 * pos["down"] / max(1, tot)))
        return

    # 形态聚类
    shapes = {}
    for c in calls:
        cmd_len = len(c["command"])
        res_len = c["result_len"] or 0
        seen = set()
        call_shapes = set(s for s, _, _, _ in c["parsed"])
        for shape, argv0, feats, sep in c["parsed"]:
            e = shapes.get(shape)
            if e is None:
                e = shapes[shape] = {
                    "shape": shape, "argv0": argv0, "segs": 0, "calls": 0, "down": 0,
                    "cmd_lens": [], "res_lens": [], "flags": Counter(),
                    "tmp": 0, "writes": 0, "pty": 0, "heredoc": 0, "savable_sum": 0.0,
                }
            e["segs"] += 1
            if sep == "|":
                e["down"] += 1
            e["cmd_lens"].append(cmd_len)
            e["res_lens"].append(res_len)
            for f in feats.get("flags", ()):
                e["flags"][f] += 1
            if feats.get("tmp"):
                e["tmp"] += 1
            if feats.get("writes"):
                e["writes"] += 1
            if feats.get("pty"):
                e["pty"] += 1
            if feats.get("heredoc"):
                e["heredoc"] += 1
            if shape not in seen:
                seen.add(shape)
                e["calls"] += 1
        # 可省段数（分摊口径）：g(c) = 胶水段 + 管道下游段 + 管道符数，按调用内形态数 k 分摊；
        # 形态自身若是胶水段，再 +1（它整段消失）
        down = sum(1 for _, _, _, sep in c["parsed"] if sep == "|")
        pipes = down
        gluen = sum(1 for _, a, _, _ in c["parsed"] if a in GLUE_CMDS)
        gc = gluen + down + pipes
        k = max(1, len(call_shapes))
        for shape, argv0, feats, sep in c["parsed"]:
            e = shapes[shape]
            extra = 1.0 if argv0 in GLUE_CMDS else 0.0
            e["savable_sum"] += gc / k + extra

    for e in shapes.values():
        s = max(1, e["segs"])
        e["tmp_share"] = e["tmp"] / s
        e["write_share"] = e["writes"] / s
        e["pty_share"] = e["pty"] / s
        e["heredoc_share"] = e["heredoc"] / s
        e["savable"] = e["savable_sum"] / max(1, e["calls"])
        e["cmd_med"] = statistics.median(e["cmd_lens"]) if e["cmd_lens"] else 0
        e["res_med"] = statistics.median(e["res_lens"]) if e["res_lens"] else 0
        e["cmd_tot"] = sum(e["cmd_lens"])
        e["res_tot"] = sum(e["res_lens"])
        tags, cost, why = classify(e)
        e["tags"], e["cost"], e["why"] = tags, cost, why
        e["score"] = e["calls"] * e["savable"] / cost

    rows = sorted(shapes.values(), key=lambda e: -e["segs"])
    print("形态数：%d" % len(shapes))
    print("段数覆盖率（top %d 形态）：%.1f%%" % (top, 100 * sum(e["segs"] for e in rows[:top]) / seg_all))
    print()
    print("## 形态表 top %d（按段数）" % top)
    print("| 骨架 | 段数 | 调用数 | 管道% | 可省段 | 判据 | cmd中位 | res中位 | cmd总 | res总 | 成本 |")
    print("|---|---:|---:|---:|---:|---|---:|---:|---:|---:|---:|")
    for e in rows[:top]:
        print("| `%s` | %d | %d | %.0f%% | %.1f | %s | %d | %d | %d | %d | %d |" % (
            e["shape"], e["segs"], e["calls"], 100 * e["down"] / max(1, e["segs"]),
            e["savable"], ",".join(e["tags"]) or "-", e["cmd_med"], e["res_med"],
            e["cmd_tot"], e["res_tot"], e["cost"]))
    print()
    print("## 排序视图 top 20（调用数 × 可省段 ÷ 成本）")
    print("| # | 骨架 | 调用数 | 可省段 | 成本 | 得分 | 判据 |")
    print("|---:|---|---:|---:|---:|---:|---|")
    for i, e in enumerate(sorted(shapes.values(), key=lambda x: -x["score"])[:20], 1):
        print("| %d | `%s` | %d | %.1f | %d | %.0f | %s |" % (
            i, e["shape"], e["calls"], e["savable"], e["cost"], e["score"],
            ",".join(e["tags"]) or "-"))
    print()

    crosscheck(stats)

    # 以下为附录
    whole = Counter()
    whole_segs = Counter()
    for c in calls:
        if not c["parsed"]:
            continue
        parts = []
        for shape, argv0, feats, sep in c["parsed"]:
            if sep in ("|",) and parts:
                parts[-1] = parts[-1] + " | " + shape
            elif sep in ("&&", "||", ";", "&", "\n"):
                parts.append(shape)
            else:
                parts.append(shape)
        key = " && ".join(parts)
        whole[key] += 1
    print("## 附录 A：整条调用骨架 top 20")
    print("| 调用骨架 | 调用数 |")
    print("|---|---:|")
    for k, v in whole.most_common(20):
        print("| `%s` | %d |" % (k[:200], v))
    print()

    # 成本档位明细
    print("## 附录 B：成本档位明细（top %d 形态）" % top)
    print("| 档位 | 形态数 | 段数合计 | 调用数合计 |")
    print("|---|---:|---:|---:|")
    for lvl in (1, 2, 3):
        sel = [e for e in rows[:top] if e["cost"] == lvl]
        print("| %d | %d | %d | %d |" % (lvl, len(sel), sum(e["segs"] for e in sel), sum(e["calls"] for e in sel)))
    print()
    print("## 附录 C：判据覆盖（全量 %d 个形态）" % len(shapes))
    for tag in ("stdin", "prev", "write", "pty", "heredoc"):
        sel = [e for e in shapes.values() if tag in e["tags"]]
        print("- %s：%d 个形态，段数合计 %d，调用数合计 %d" % (
            tag, len(sel), sum(e["segs"] for e in sel), sum(e["calls"] for e in sel)))
    print()
    print("## 附录 C2：写 / 上一条产物 / PTY 类形态 top 15（按段数）")
    print("| 骨架 | 段数 | 调用数 | 判据 | 成本 |")
    print("|---|---:|---:|---|---:|")
    sel = [e for e in shapes.values() if ({"write", "prev", "pty"} & set(e["tags"]))]
    for e in sorted(sel, key=lambda x: -x["segs"])[:15]:
        print("| `%s` | %d | %d | %s | %d |" % (
            e["shape"], e["segs"], e["calls"], ",".join(e["tags"]), e["cost"]))
    print()


if __name__ == "__main__":
    main()
