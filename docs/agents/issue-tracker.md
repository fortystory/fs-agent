# issue tracker：本地 Markdown

本仓库的 issue 与 spec，都是放在 `.scratch/` 下的 markdown 文件。

## 约定

- 一个 feature 占一个目录：`.scratch/<feature-slug>/`
- spec 就是 `.scratch/<feature-slug>/spec.md`
- 实现 issue 是一票一个文件，放在 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`，从
  `01` 编号，永远不是把所有票合成一个文件
- 分诊状态记在每个 issue 文件靠顶部的那一行 `Status:` 里（角色串见 `triage-labels.md`）
- 评论与对话历史一律追加到文件的底部，放在 `## Comments` 标题之下

## 当某个技能说「发布到 issue tracker」

在 `.scratch/<feature-slug>/` 下新建一个文件（该建目录时就把目录一并建出来）。

## 当某个技能说「取回相关的票」

把被引用路径上的那个文件读掉。用户通常会把路径或者 issue 编号直接传过来。

## wayfinder 的操作

由 `/wayfinder` 使用。**决策图**就是这样一个文件，每张票各配一个**子**文件。

- **决策图**：`.scratch/<effort>/map.md`（`Notes` / `Decisions so far` /
  `Not yet specified` 那几段正文）。
- **子票**：`.scratch/<effort>/issues/NN-<slug>.md`，从 `01` 编号，问题写在正文里。
  `Type:` 那一行记录票的类型（决策票是 `research`/`prototype`/`grilling`/`task`；图走完后
  经由 `/to-tickets` 交棒的构建切片是 `implement`）；`Status:` 那一行记录
  `claimed`/`resolved`。
- **阻塞**：靠顶部的那一行 `Blocked by: NN, NN`。等它列出的每个文件都变成 `resolved`
  之后，这张票才解除阻塞。
- **frontier**：扫 `.scratch/<effort>/issues/`，去找 open、未阻塞、也未被认领的那些文件；
  编号最小的那个胜出。**跳过 `Type: implement`** —— 那是设计票之后住在同一个目录里的构建切片，
  由 `/implement` 认领，不由 wayfinder 会话认领。设计票全部关掉的图就算走完了，哪怕它的
  实现票还开着。
- **认领**：动手之前先写下 `Status: claimed` 并保存。
- **收尾**：把答案追加到 `## Answer` 标题之下，写 `Status: resolved`，然后把一个 context
  指针（要点 + 链接）追加到 `map.md` 里决策图的 `Decisions so far` 一段。
- **核对**：`python3 scripts/wayfinder-check.py .scratch/<effort>/map.md`。本地 markdown 里
  没有子 issue、也没有依赖边，所以这个脚本代它做的是原生 tracker 本来会做的那次「期望
  vs 实际」比对：图的 `## 任务清单` 里的勾选项必须恰好是 `issues/` 里那些文件，而且每个
  子票都必须带上 `Type:` / `Status:` / `Part of:` / `Blocked by:`，而且指向一个同目录的
  兄弟票。任何对不上都以非零码退出；所以在宣布这张图走完之前先跑它。
