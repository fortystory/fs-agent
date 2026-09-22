# grilling：Ctrl-D 退出的确认与提示行

Type: grilling
Status: resolved
Blocked by: —
Part of: ../map.md

## Question

加 `Ctrl-D` 退出 TUI，并在按下后弹确认。键位、默认答案与提示行都已有冻结方向，本票把剩下要用户拍板的细节定死（文案、边界、与既有退出手势的关系）。本票体量不大，但仍是一张决策票：文案与边界要一次说清。

## 冻结的输入（不得重开）

- **空闲按 `Ctrl-D`** → 弹确认；**默认答案「否」**，`Esc` = 否，`y` = 退出。
- **忙（回合在飞）时按 `Ctrl-D` 忽略**。
- 底部提示行**加 `ctrl-d 退出`**，与 `ctrl-c 退出` 同级保留。
- `Ctrl-C` 现有行为（忙时取消回合、闲时退出）**不变**。
- 四种既有 pending 问题分别有默认答案；本票的确认是**第五种**还是复用其中一个，由本票定。

## 需要定

1. **文案**（全部落 `src/render/wording.rs`）：确认弹窗的标题、正文、候选键行（对照 `wording::clear_draft_title` / `clear_draft_body` / `CLEAR_CHOICES` 的既有形态）；提示行里 `ctrl-d 退出` 的词序与是否与 `ctrl-c 退出` 合并。
2. **它是哪种 pending**：新增一个 `Pending` 变体，还是复用 `Pending::ClearDraft` / `Paste` 式的渲染器自有确认（都走 `answer_key` / `default_answer`）。给出选择与理由，并说明 `RunState { running: false }` 撤回逻辑要不要管它（渲染器自有的确认不受运行边界影响，见票 31）。
3. **边界**：`Ctrl-D` 在**问卷/权限模态/详情覆盖层**打开时按下会怎样（被模态吃掉、还是仍能退出）；`Ctrl-D` 在粘贴确认、`Esc` 清草稿确认打开时呢；空闲但输入框有**多行草稿**时是否先警告（`Esc` 已有类似确认）。
4. **`Ctrl-D` 与空输入**：`Ctrl-D` 在 readline 里是 EOF；本 TUI 里按下即触发退出确认，与「草稿非空」无关（确认），以及它不再插入任何字符。
5. **提示行的降级**：`ctrl-d 退出` 插在六条提示的哪里；`40` 列只显示 3 项时它还在不在（现有算法：状态词与 `ctrl-c 退出` 先占位、中间从左往右填）。要给出**新的逐宽度条目表**（40 / 60 / 80 / 120）。
6. **文档落点**：`docs/tui-manual-checklist.md` 与 `scripts/tui-startup-check.py` 的锚点是否需要跟着改（与该脚本 docstring 的分界口径一致）。

## 先读

- `src/render/tui.rs` 的 `map_key` / `Key` / `pending` / `answer_key` / `default_answer` / `Press` 路由 / `status_line`
- `src/render/wording.rs` 的 `clear_draft_*` / `CLEAR_CHOICES` / `status_line` / 提示集常量
- `src/render/input.rs`（`Question` / `AnswerChoice` / 默认答案）
- `.scratch/tui-layout/spec.md` §6（键位表）、§9（模态）、§10（提示行与降级）
- `.scratch/fs-agent-v1/issues/31-modal-outlives-the-run-it-belongs-to.md`（运行边界与撤回）
- `scripts/tui-startup-check.py` 的 docstring（pty 只保启动/退出交还）

## 进度

**100%** —— 完成。两轮 grilling、6 条决定，票面 6 个「需要定」全部覆盖；契约见 `## Answer`。同时回改了票 02 的 Answer（详情覆盖层对 `Ctrl-D` 的例外）。

**下一步**：无（已 resolved）。`/to-spec` 按 §6 回改 tui-layout spec §6/§10；`grilling：测试与验证迁移` 按 §4 的预期梯子实测。

## Answer

**契约（2026-09-23，两轮 grilling、6 条决定）。**

### §1 键位与状态机

- `Key` 新增 `CtrlD`；`map_key` 的 CONTROL 分支加 `'d'`。
- `TuiState::key` 在**初始 match**（与 `Ctrl-C` 同层、早于 `pending` 守卫）加 `Key::CtrlD`，**按顺序**判：
  1. `self.busy()` → **忽略**（不产生事件、不改状态）。
  2. 详情覆盖层打开 → **关闭详情覆盖层**（等于 `Esc` 关闭那条路径；不弹退出确认）。**这是用户的决定**，也是票 02「其它键忽略」的例外。
  3. `self.pending.is_some()`（权限 / 计划冲突 / 粘贴确认 / 清草稿确认 / 问卷，以及退出确认自身）→ **忽略**。
  4. 否则 → `self.pending = Some(Pending::Exit)`。
- `Pending` 新增第五态 **`Exit`**（渲染器自有，无回传通道）。`RunState { running: false }` 的撤回**不管它**：它不是循环的问题，且只在空闲时出现。

### §2 文案与应答（落 `src/render/wording.rs` + `tui.rs`）

- `exit_title()` = **`退出会话`**
- `exit_body()` = **`会话记录会保留；未发送的草稿会丢弃`**
- `EXIT_CHOICES: [Choice; 2]` = `[y] 退出` / `[n] 取消`
- `Pending::modal()` 加 `Exit` 一条（title = `exit_title`，detail = `exit_body`，choices = `EXIT_CHOICES`）。
- `answer_key` 的 `Pending::Exit` 分支：`agrees(key)`（`y`/`Y`）→ `self.quit = true`；其它可达键（`Char`/`Enter`）**不动作**，确认关闭 = 默认「否」。
- `decline(Pending::Exit)` → **no-op**（确认关闭、留在应用里 = 否）。
- **不加二次草稿警告**：退出确认本身就是那次确认，body 已说明草稿会丢。

### §3 边界汇总

| 情形 | 按 `Ctrl-D` |
| --- | --- |
| 忙（回合在飞） | 忽略 |
| 详情覆盖层打开 | **关闭详情覆盖层** |
| 权限 / 计划冲突 / 粘贴确认 / 清草稿确认 | 忽略 |
| 问卷（`ask_user_question`） | 忽略 |
| 退出确认本身 | 忽略（它就是 `pending`） |
| 空闲、无覆盖层 | 弹退出确认（默认「否」，`y` 退出） |
| 草稿非空 / 多行 | 同样弹确认（不再二次警告） |

### §4 提示行

- `hint_line` 改成接收一个 `exit: &str` 参数，新增两个常量：**`EXIT_HINT_IDLE = "ctrl-c/ctrl-d 退出"`** 与 **`EXIT_HINT_BUSY = "ctrl-c 退出"`**（沿用旧值）。`status_line` / `viewer_status_line` 按 `busy` 选。
- 理由：忙时 `Ctrl-D` 是忽略的，忙时/查看行**不得广告它**（现有的纪律是「提示描述键盘现在真的做什么」）。
- **空闲行**新梯子（按现有 `hint_line` 算法手算；**待票 08 用真函数实测确认**）：

| width | 整行（`就绪` 是否在场） |
| --- | --- |
| 40 | `就绪 · enter 发送 · ctrl-c/ctrl-d 退出`（3 项） |
| 60 | `enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c/ctrl-d 退出`（4 项，**`就绪` 放不下**） |
| 80 | `就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 计划 · ctrl-c/ctrl-d 退出`（6 项） |
| 120 | `就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 计划 · PgUp/PgDn 滚动 · ctrl-c/ctrl-d 退出`（7 项） |

- 已知怪癖（如实记录）：出口文本长了 7 列，所以 `60` 列下 `就绪` 反而放不下，而 `40` 列下放得下（那里只填进 1 条提示）。这是「提示优先、状态词最后加」的直接结果，**不为本票改算法**。
- **忙时/查看行不变**：仍是 `VIEWER_HINTS`（`esc 取消` / `PgUp/PgDn 滚动`）+ `ctrl-c 退出`，40 列 3 项、≥60 列 4 项。

### §5 跨票修正（已回改）

- 票 02 的 Answer 写「详情覆盖层独占键盘…其它键忽略」。本票把 **`Ctrl-D` 作为例外**：它关闭详情覆盖层；已回改票 02 的 Answer 并在那里指向本条。

### §6 文档与回改落点

- `docs/tui-manual-checklist.md`：新增 `Ctrl-D` 项（空闲弹确认 / `y` 退出 / `Esc` 与其它键 = 否 / 忙时无反应 / 详情打开时关详情 / 提示行文案与 40 列下的样子）。
- `scripts/tui-startup-check.py`：它的 docstring 只保「启动 + 退出交还终端」。`Ctrl-D` 是第三条退出路径，可加一步「`Ctrl-D` → `y` → 终端交还干净」；按该脚本既有分界（只有 pty 看得见的东西）由 `grilling：测试与验证迁移` 决定加不加。
- `.scratch/tui-layout/spec.md` **§6 键位表**加 `Ctrl-D`，**§10 提示行**换成两个 exit 常量与上表；`/to-spec` 回改。
