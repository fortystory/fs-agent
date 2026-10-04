# 04 — `/` 的提交语义放宽：与 `@` 对等

Type: implement
Status: done
Blocked by: 02

> 来源：[`../spec.md`](../spec.md) §5。菜单在票 02 里已经拿到了与 `@` 同一条边界；这一票让
> **解析**跟上 —— 否则界面就是说了谎。

## 目标

- **整个草稿里任意行、任意位置**的 `/name`（name 在命令表里）都算命令，取**最靠左**的那个。
  `text.split_once('\n')` 那条「只看第一行」的限制**取消**。
- 记号**前面的文字（含它前面的每一行）并入任务**：`请 /loop 我的目标` → `Loop("请 我的目标")`。
- **无参命令仍要求整条只有它**：`/quit`/`/exit`/`/undo`/`/clear` 的 `if whole` 不动，于是
  「请把 `/clear` 加到文档里」是普通消息，不会被劫持成一次清空。
- 草稿里找不到命令记号时，整条仍当普通消息（不变）。
- 记号推导复用票 02 那个**不依赖 `Input`** 的纯函数，不在这里再写一份。

## 现状（2026-10-04 核实，改前先复核）

- 解析在 [`src/cli.rs:1462-1482`](../../../src/cli.rs)：`first` 是第一行 `trim()` 之后，
  `_ if first.starts_with('/')` 才进命令分支；`name` 与 `inline` 从第一个空白切开。
- `whole`（[:1475](../../../src/cli.rs)）管 `/quit`/`/exit`/`/undo`/`/clear` 的「整条才算」。
- [:1473-1474](../../../src/cli.rs) 的注释写着那条规矩的理由 —— **丢掉用户写下的东西**正是它
  要防的事；这一票的「前文并入任务」就是为了不重蹈它。
- `submission()` 已经拿到一个 `has_skill` 闭包（用来判技能），加一个「是不是命令」的判据同形。
- **这一档是有牙的**：`@` 被误认没有后果，而 `/` 被误认**会真的执行** —— `用 /loop 做目标
  循环` 就是一条命令。这是 spec §5 里明码标价选下的代价，实现时**不要**自作主张加一条
  「整行起头」的守卫。

## 收尾

- 测试（`cli.rs`）：
  - `请 /loop 我的目标` → `Loop("请 我的目标")`；
  - **第二行里的 `/loop` 也算**（多行草稿，位置不看行）；
  - `请把 /clear 加到文档里` → 普通消息；
  - `用 /loop 做目标循环` → **命令**（如实钉住这个代价，别把它当缺陷改掉）；
  - `/clear` 单独一行 → `Clear`（不变）；`/ask-matt 优化这个` → 命令带任务（不变）。
- `cargo test`、`python3 scripts/check-language.py` 通过。

## 不做什么

- 不让无参命令出现在句子里。
- 不动 `/discuss`、`/goal-new`、`/loop` 各自的参数形状与 `task_of()`。
- 不加「整行起头」这类守卫：位置与 `@` 对等正是这一票定的规则。

## 评论

- 2026-10-05 落地：`submission()` 改为用票 02 的 `render::token::tokens()` 找**整个草稿里
  最靠左**的那个名字在命令表里的 `/` 记号（表 = `wording::BUILT_IN_COMMANDS` ∪ 技能 ∪
  MCP 模板）。名字仍旧剥掉开头的斜杠（`//undo` 读作 `undo`）。
- `task_of(inline, rest)` 由 `task_around(text, token)` 接手：记号两侧拼起来，前文按原样
  保留（尾部空白去掉），后文 trim 起始空白与结尾换行；两边之间——记号前文以换行结尾就用
  换行、否则用空格。于是 `请 /loop 我的目标` → `Loop("请 我的目标")`、
  `第一行\n/loop 目标` → `Loop("第一行\n目标")`。参数形状没变，只是前文也进来了。
- 无参命令的 `whole` 换成「`text.trim()` 的字符数正好是这个记号的长度，**而且记号的原文就是
  `/<名字>`**」，语义与原来的 `rest.trim().is_empty()` 等价，但不再需要「第一行」这个概念。
  后半条是 `/code-review` 之后补的：只比长度时 `//undo` 会剥成 `undo` 而被执行，而旧路径把
  它当错字（`Unknown`）—— 那是票面没要求的放宽，何况 `Undo` 有回滚副作用。现在 `//undo` 落到
  「内建命令带着记号之外的字」那一条，整条当普通消息，不执行。
- 未知命令那条提示窄化成「以 `/` 开头、整行只有一个不认识的名字」；只要草稿里有命令记号，
  它就永不被当成错字 —— `看 /tmp/x` 与「提一句 `/clear`」都仍是普通消息。
- 测试：`a_command_can_sit_anywhere_and_earlier_words_join_the_task`、
  `a_no_argument_command_still_needs_the_whole_draft`、
  `a_draft_without_a_command_token_is_still_a_message` 三条新测试，加上既有的
  `a_single_line_still_reads_exactly_as_it_did` / `a_pasted_paragraph_that_opens_with_a_slash_is_a_prompt`
  / `clear_is_a_whole_submission_and_not_a_task` / `a_built_in_takes_no_task_so_a_line_after_it_is_not_dropped`
  全部原样通过。
