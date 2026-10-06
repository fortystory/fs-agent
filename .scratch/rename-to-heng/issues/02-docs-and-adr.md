# 02 — 文档与 ADR：衡（heng）的定位与来路

Status: done
Part of: [../spec.md](../spec.md) §3

**What to build:** 把旧名字从人读的文档里换掉，并把「衡」的来路、它与 Forked Synthesis 的关系、以及历史不追改的边界写进 ADR。

## 验收

- [x] `README.md`：标题与 banner 换新名；第 11 行那段「`fs` 只是叙述里的框架名，不是命令行或路径的一部分」整段删除，换成一句「衡（heng）——一套自用 coding agent harness」
- [x] `CONTEXT.md`：**名字**词条重写为「衡（heng）」；**分叉合成（Forked Synthesis）**词条保留、明确降为讨论协议的机制名；其余引用逐处替换
- [x] `docs/*.md` 的路径、命令、示例换名（`docs/research/` 不动）
- [x] 新增 `docs/adr/0014-renamed-to-heng.md`：两条理由（`fs` 歧义、定位是 harness 而非 agent）、拼音方案、Forked Synthesis 降为机制名、历史不追改的边界
- [x] 受影响的旧 ADR（如 0009 里的 goals 路径）加一行补注指向 0014，**正文不改**
- [x] `.scratch/README.md` 的 feature 索引加 `rename-to-heng/` 一行
- [x] `docs/**`（不含 `research/`）里搜不到旧名的残留
