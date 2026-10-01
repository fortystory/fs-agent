# 标题文本的生成：形状、状态词与 40 列封顶

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/terminal-title/spec.md` §1（三段形状与 `$HOME` 缩写）、§2（状态词表与优先级）、§3（40 列封顶与从右往左的丢弃顺序）。
> 这一票只做**纯函数**：给定 cwd、状态与目标名，算出那一条标题字符串。往终端写 OSC、保存/还原、以及绘制路径上的比对是 [`02-set-and-restore.md`](02-set-and-restore.md)；真机与文档是 [`03-manual-and-docs.md`](03-manual-and-docs.md)。

## 目标

- `<路径> · <状态> · <目标名>` 三段，按需省略（spec §1）。
- 四个状态、三个词、一条固定优先级（spec §2）。
- 整条按**显示列宽**（不是字节）40 列封顶，超了从右往左丢，一次一段（spec §3）。

## 现状

- [`src/render/wording.rs`](../../../src/render/wording.rs) 是「每一条给人看的短语，都在一处」的措辞层：纯函数、硬编码中文、没有运行时 locale（文件头注释）。它现在**没有任何终端标题相关的函数**（`grep -n '终端标题\|fn .*title' src/render/wording.rs` 只命中现有的模态标题与详情小标题）。
- `src/` 里也没有任何 OSC 标题代码（`grep -rn 'x1b]0\|SetTitle' src/` 零命中）。
- 宽度算术已有归宿：[`src/render/width.rs`](../../../src/render/width.rs) 的 `text_columns`（显示列宽）与 `truncate_columns`（按列裁、不劈开宽字符）。`src/render/panel.rs`、`src/render/markdown.rs` 就是这么用的。
- [`tests/wording.rs`](../../../tests/wording.rs) 是这个仓库里唯一断言**精确文本**的一层（文件头注释），逐条 `assert_eq!`；函数名英文、散文中文。它现在有 1065 行、按「一族措辞一组测试」组织。

## 落点

- [`src/render/wording.rs`](../../../src/render/wording.rs)：新增一个状态枚举、五个纯函数与一个常量。
- [`tests/wording.rs`](../../../tests/wording.rs)：新增一组 `#[test]`，沿用现有文件的形状。

## 具体行为

1. **状态枚举与词**（spec §2）。落一个 `pub enum TitleState { Idle, Running, Waiting, Replaying }` 与一个 `pub fn title_word(state: TitleState) -> Option<&'static str>`：`Idle` → `None`、`Running` → `"运行中"`、`Waiting` → `"等你"`、`Replaying` → `"重放中"`。空状态**省略整段**，不是空字符串。
2. **优先级解析**（spec §2）。落一个 `pub fn title_state(replaying: bool, pending: bool, busy: bool) -> TitleState`，按 `replaying` → `Replaying`、否则 `pending` → `Waiting`、否则 `busy` → `Running`、否则 `Idle` 的顺序短路。调用方（票 02）把 `state.replay.is_some()` / `state.pending.is_some()` / `state.busy()` 三个布尔递进来，优先级只住在这里一处。
   - `muted`（禁言）**不**单独成词、也不参与这个函数：它是运行中的一种（spec §2）。
3. **路径**（spec §1）。落 `pub fn title_path(cwd: &Path, home: Option<&Path>) -> String`，非 UTF-8 用 `to_string_lossy()`。
   - **补判（spec §1 这里有两句会打架）**：§1 正文说路径是 cwd 的「父目录基名/当前基名」，但第三个例子 `~/code/fs-agent · 运行中 · 修文档索引` 在 `$HOME/code/fs-agent` 下按「父/基」只会得到 `code/fs-agent`、拿不到 `~`。本票按**能同时产出 §1 那四个例子**的规则钉死：
     - cwd 就是 `/` → `/`；
     - cwd 等于 `$HOME` → `~`；
     - cwd 在 `$HOME` 之下 → `~` + cwd 相对 `$HOME` 的路径（`/` 分隔、不带前导斜杠）：`$HOME/code/fs-agent` → `~/code/fs-agent`；
     - 其余 → `父目录基名/当前基名`：`/x/fortystory/fs-agent` → `fortystory/fs-agent`，`/x/fs-agent`（父就是根）→ `fs-agent`。
   - 前缀匹配按**路径分量**（`Path::strip_prefix` 或逐 `Component` 比），不是字符串前缀：`/home/forty2` 不算落在 `/home/forty` 之下。
   - `home` 是**参数**，不在 `wording.rs` 里读 `$HOME`：这一层是纯函数，测试要能钉死；`HOME` 在组装处读（票 02）。
4. **只剩基名的路径**（spec §3 第 3 步）。落 `pub fn title_path_base(cwd: &Path, home: Option<&Path>) -> String`：`/` → `/`；`$HOME` → `~`；其余取最后一段（`$HOME/code/fs-agent` → `fs-agent`，`/x/fortystory/fs-agent` → `fs-agent`）。
5. **拼接**。三段用 ` · `（空格 + `U+00B7` + 空格）连接；省略任一段后不留多余分隔符。落一个常量 `pub const TITLE_COLUMNS: usize = 40`。
6. **封顶与丢弃顺序**（spec §3 的 1–4 步）。落 `pub fn terminal_title(cwd: &Path, home: Option<&Path>, state: TitleState, goal: Option<&str>) -> String`：
   1. 先拼完整（路径 + 状态词（Idle 则无）+ 目标名（`None` 则无））。`text_columns(&full) <= TITLE_COLUMNS` 就返回它；
   2. 丢 `<目标名>` 再拼，`<= 40` 就返回；
   3. 再丢 `<状态>`（只剩路径），`<= 40` 就返回；
   4. 路径退成 `title_path_base(...)`，`<= 40` 就返回；
   5. 仍然超，用 `truncate_columns(&base_only, TITLE_COLUMNS)` 硬截断。
   - 规则必须**固定**、可测：标题没有宽度反馈，任何「看情况缩」的写法都不合规格（spec §3 末）。`goal` 缺席时第 2 步是一次空操作，直接落到第 3 步。
   - 每一处比较都走 `text_columns`；不许用 `str::len()`（中文一个字三字节）。

## 测试

在 [`tests/wording.rs`](../../../tests/wording.rs) 新增（函数名英文、断言中文，沿用现有文件）：

- `a_title_is_path_status_and_goal_joined_by_middle_dots`：`terminal_title("/x/fortystory/fs-agent", None, Running, None)` 得 `fortystory/fs-agent · 运行中`；带 `Some("修文档索引")` 得 `fortystory/fs-agent · 运行中 · 修文档索引`。
- `an_idle_title_omits_the_status_segment`：`Idle` 且无目标得 `fortystory/fs-agent`。
- `a_title_without_a_goal_omits_the_goal_segment`：`Waiting`、`goal = None` 得 `fortystory/fs-agent · 等你`。
- `the_home_directory_is_abbreviated_to_a_tilde`：`home = Some("/home/forty")` 时，`/home/forty/code/fs-agent` → `~/code/fs-agent`；`/home/forty` → `~`；`/home/forty2/code` **不**缩（分量边界）。
- `the_root_directory_and_a_home_less_run_have_their_own_paths`：`"/"` → `/`；`home = None` 且 `/home/forty/code/fs-agent` → 走「父/基」分支 `code/fs-agent`。
- `a_title_longer_than_forty_columns_drops_the_goal_first`：造一条只有丢掉目标名才 ≤ 40 的输入，断言结果里**没有**目标名、**有**状态词。
- `a_still_too_long_title_drops_the_status_next`：造一条丢了目标仍超、丢了状态才 ≤ 40 的输入，断言结果只剩路径。
- `a_still_too_long_path_falls_back_to_its_base_name`：造一条只剩路径仍超、退成基名才 ≤ 40 的输入，断言结果是基名（`$HOME` 下仍是基名，不是 `~` + 全路径）。
- `a_path_that_cannot_fit_is_truncated_to_forty_columns`：退成基名后仍超（基名本身很长），断言 `text_columns(result) <= 40` 且结果是原基名的前缀（宽字符不劈开）。
- `replay_outranks_a_pending_question_which_outranks_a_running_turn`：`(true, true, true)` → `Replaying`；`(false, true, true)` → `Waiting`；`(false, false, true)` → `Running`；全 `false` → `Idle`。
- `an_idle_session_has_no_status_word`：`title_word(TitleState::Idle) == None`，其余三个词逐条 `assert_eq!`。
- 宽字符一条：把目标名换成中文，确认 40 列门槛按显示列算（40 个 ASCII 能过、20 个汉字占 40 列也在门槛上）。

`cargo test --test wording` 全绿；这一票不该动任何别的测试。

## 不做什么

- **不写终端序列**、不碰 `TerminalModes`、不给 `TuiState` 加字段：那是票 02。
- 不做 prompt 摘要 / 任务摘要进标题（spec §1 末、§明确不做）。
- 不加配置项、不做标题模板语法（spec §明确不做）。
- 不改 `src/render/wording.rs` 里现有的任何函数与文本，也不改状态行 / 提示行的任何一句（spec §明确不做）。
- 不读 `$HOME` / 不读终端响应；路径与 `home` 都是参数（spec §明确不做「不读终端响应」）。

## Comments

- **落地**：`src/render/wording.rs` 里新增 `TITLE_COLUMNS`、`TitleState`、`title_word`、`title_state`、`title_path`、`title_path_base`、`terminal_title`（加上私有的 `title_base` 与 `join_title`）。`home` 是参数，这一层不读 `$HOME`；前缀按 `Path::strip_prefix` 的分量语义比。
- **一处与 spec 例子对不上**（本票补判，写在这里供 spec 作者改）：spec §1 举的例子 `fortystory/fs-agent · 运行中 · 修文档索引` 按 §3 的 40 列封顶算**是 41 列** —— 它会被规则丢掉目标名，留下 `fortystory/fs-agent · 运行中`。本票以 §3 那条固定规则为准（规则是可测的那一条），测试里把目标名换成 `修文档`（整串 37 列）。spec §1 那个例子要么改短，要么得承认它只是形状示意。
- **测试**（`tests/wording.rs`，新增 11 条）：三段齐全 / 省略状态 / 省略目标 / `~` 缩写与分量边界 / 根与无 `home` / 三种丢弃顺序各一条 / 基名仍超时按列截断（不劈开宽字符）/ 状态优先级 / 状态词表 / 40 列按显示列而非字节算。
- `cargo test` 全绿（954 条）。
