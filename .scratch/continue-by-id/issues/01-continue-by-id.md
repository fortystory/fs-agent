# `-c` / `--continue` 吃一个可选的 id，另加 `--session <id>`

Type: implement
Status: done
Blocked by: —

> 规格：[`.scratch/continue-by-id/spec.md`](../spec.md) §1（旗标形状）、§2（去哪找）、§3（工作目录跟谁走）、§4（回执与 spec 回改）、「Testing Decisions」的三个 seam。

## 目标

- `-c [<id>]` / `--continue [<id>]` 与 `--session <id>` 都能指名一场会话；不带 id 仍是本工作区最新那场。
- 指名的那一场在别的工作区时，这一趟的工作目录跟着它走，并在屏幕上说一句。
- 找不到、缺值、旗标当 id 这三种情形各自的说法。

## 现状

- `InteractiveArgs.resume` 是 `bool`，消费点只有 `SessionStore::latest`（`src/cli.rs`）—— 形状上
  容不下 id。
- `find_session(store, cwd, id)` 早就存在（`sessions show <id>` 用）：先试「id 是不是会话目录」，
  否则 `list(cwd)` + `list_all()` 合并后按 id 匹配。这条路可以直接复用。
- `SessionStore` 没有「这场会话在不在这个桶里」的查询；`session::observe::summarize` 自己从
  `SessionStarted` 里取 cwd，与「按 id 续上时要知道它开在哪」是**同一条事实**。

## 落点

- `src/cli.rs`：`Resume` 枚举（`Latest` / `Session(String)`）、`parse_interactive` 的两处分支、
  应用侧新抽的 `choose_session`（返回 `SessionChoice { stored, cwd, elsewhere }`）与它接上的
  `interactive()`（`elsewhere` 时打 `session_followed`）。
- `src/session/store.rs`：`is_in_bucket(cwd, id)`、`started_cwd(events)`、`session_cwd(session)`。
- `src/session/observe.rs`：`summarize` 的 cwd 改用 `started_cwd`（一条事实一个来源）。
- `src/render/wording.rs`：`session_receipt` 改成「接着跑」、新增 `session_followed`、两份 help
  文本点名 `--continue, -c [ID]` 与 `--session ID`。

## 具体行为

1. 解析：`-c <id>` / `--continue <id>` / `--session <id>` 都得到「续这一场」；`-c` 后面那个词以
   `-` 开头时不算 id（`fs-agent -c --plain` 仍是「续最新 + plain」）；`--session` 缺值或后面跟旗标
   都报 `needs_value`；不写 `resume` 就是新开一场。
2. 查找：本桶 → 全 store；id 是会话目录路径时直接用那个目录。找不到时点名 id 并说清搜过哪儿。
3. 工作目录：本桶命中用请求的目录；别处命中用它自己的 `SessionStarted.cwd` 并打一句
   「接着跑的是 `<目录>` 里的那场会话」。读不出桶当本桶；读不出它自己的 cwd 就退回请求的目录、
   且不打那句。
4. 回执：`fs-agent: 会话 <id>；接着跑：fs-agent -c <id>`（`discuss` 与交互式退出共用同一句）。

## 测试

- `src/cli.rs` 的 `mod tests`：`a_session_is_named_by_id_or_by_its_directory`（两种拼写、可选值、
  旗标不当 id、目录路径、缺值两条路）、`the_continue_flag_parses_both_spellings_and_defaults_to_off`、
  `opening_a_session_is_newest_named_or_fresh`（`choose_session` 的七条分支，用真 store + 临时目录，
  流里手写一条 `SessionStarted`）、`the_receipt_names_the_session_and_never_changes_the_exit_code`。
- `tests/wording.rs`：`session_receipt` 与 `session_followed` 的文案；两份 help 文本点名两个旗标。
- 手工：在别的工作区用 `-c <id>` / `--session <id>` 续上（看目录跟过去）、打错 id、桶里没得续、
  回执那一行。

## 不做什么

- 不做模糊匹配、不动 `prune` / `sessions` 一族、不给「续一场已结束的会话」加特判、不引入全局索引
  （见 spec 的「明确不做」）。

## 评论

- **落地**：2026-10-02 一轮做完（就是这一份 spec 与这张票的来源那次访谈）。
  `Resume` 是三值枚举，`parse_interactive` 两处分支各让「旗标不当 id」这一条成立；
  `choose_session` 收下「打开哪一场 + 落在哪个目录」这一整件事，`interactive()` 只剩接线。
  「在不在本桶」与「这场会话开在哪」都搬进了 `SessionStore`（`is_in_bucket` / `started_cwd` /
  `session_cwd`），`observe::summarize` 的 cwd 也改走 `started_cwd`。
- **两轴 review 的处置**（`/code-review`，固定点 `7415ea4`）：Spec 轴指出 `--session --plain` 会把
  旗标当 id（已修，与 `-c` 对称）、`elsewhere` 在读失败时会说假话（改成「读不出当本桶，读不出它
  自己的 cwd 就不打那句」）、`docs/discussion.md` 没跟上回执改形状（已改）；Standards 轴指出这次
  能力变更没落 feature 与票（就是这一份）、新测试断言了 `log_path` 这个实现细节（已删，改用
  「续最新能找回它」间接证明）、`open_session` / `OpenedSession` 与 `lib.rs`、`wording.rs` 里的同名
  东西撞名（改成 `choose_session` / `SessionChoice`）、「在不在本桶」的判据与 cwd 提取该住进
  `session/`（已搬）。`Resume::Session(String)` 一个字符串兼装 id 与会话目录路径**保留**：那是
  `find_session` 的既有语义（`sessions show <id>` 一直这么收），为它新造一个类型要牵动那一族。
