# 10 — 一条 `git` 工具（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §3。来源票：[git 工具：一条入口与 op 分档](04-grilling-git-tool.md)。

## 目标

模型用**一条**工具看仓库状态、改动、提交历史与某次提交的内容，并在同一条工具里做暂存与提交；
读侧操作在只读档下照常放行，写侧与 `bash` 一样独占工作区。

**可 demo**：`op: "status"` 拿回真实输出；`op: "log"` 配 `args: ["--oneline", "-5"]` 拿回历史。

## 现状（改前先复核）

- 模型侧今天**没有任何** git 集成；渲染层有一份宿主侧实现，但它的纪律是"不过权限门、不过沙箱"，
  与模型侧的要求正相反 —— **两边不共用代码**，共用的只是"起一个子进程"这件事本身。
- bash 段里 `git` 出现 1799 次（读侧 78%：`diff` 436 / `status` 395 / `log` 285 / `show` 78）。
- 沙箱已经精确保护 `.git/config` 与 `.git/hooks`（不是整个 `.git`），因为 `git add` / `commit`
  要写 `.git/index.lock`。

## 验收

- [x] 两个字段：`op`（枚举，必填）+ `args`（字符串数组，可选、原样透传）
- [x] 读 op = `status` / `diff` / `log` / `show` → `ReadOnly`（四档放行、不取工作区锁）
- [x] 写 op = `add` / `commit` / `stash` → `Exclusive`（写的是 `.git/index` 之类，枚举不出恰好这些路径）
- [x] 未知 op 是参数错误，并且在 `effect()` 判定之前就能判出来
- [x] 读 op 与写 op **同批**声明、同批实现（声明了不实现会让模型调用到失败；分两批发布要再废一次缓存）
- [x] 输出原样（stdout / stderr / 退出码），溢出走既有截断流水线；**不加** `cwd:` 首行
- [x] **不给** `workdir`；需要换目录时 `args` 透传 `-C`
- [x] `.git/config` 与 `.git/hooks` 仍被压回只读，而 `git add` / `commit` 能正常写 index
- [x] 模型侧必须过权限门与沙箱（不继承渲染层那条例外）
- [x] 新增一份 git 工具文档：为什么不是一条 shell 命令、op 表、effect 分档、与渲染层的分界
- [x] 新增测试：读 op 在只读档放行、写 op 不放行、未知 op 报错、`args` 透传的形状
