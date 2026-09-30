# 覆盖层立着时，滚轮按位置分派

Type: implement
Status: done

> 规格：`.scratch/tui-chrome/spec.md` §5。
> 本票推翻 `.scratch/tui-sidebar/spec.md` §7 与 `.scratch/tui-sidebar/issues/04-turn-rail.md` §2 的「待答问题吃掉一切指针」——**只推翻滚轮那一半，点击的优先级一丝不变**。
> 与票 01–04 没有文件级依赖；建议排在它们之后做，免得在同一批断言上反复改。

## 目标

问题立着的时候（问卷占着底部块、或中间模态浮在主列中央），滚轮不再被一口吃掉：**指针落在哪一块就滚哪一块**。指针在覆盖层自己那块里就归它（问卷挪高亮、模态什么都不做），指针在转录上就滚转录。

## 落点

`src/render/tui.rs`（`TuiState` 新增 `modal_rect`、`mouse`、`question_click`、`draw_modal`）；`tests/render_layout.rs`（`the_wheel_is_ignored_while_a_question_is_up` 整条改写）；`tests/ask_user_question_tui.rs`（问卷的滚轮用例，若有）。

## 具体行为

1. **`TuiState` 新增字段 `modal_rect: Option<Rect>`**，与 `detail_rect` 同形、同一个纪律：**这一帧真的画了什么就记什么**。
2. **`draw_modal` 记它**：在 `let Some(area) = panes.modal(rows.len() as u16) else { return; }` **之前**把 `state.modal_rect = None` 清掉（函数开头就清），拿到 `area` 之后 `state.modal_rect = Some(area)`。三个提前返回的分支（`inner == 0`、`rows_available == 0`、`panes.modal()` 返 `None`）因此都留下 `None`。`draw_frame` 里已有的 `state.regions.clear()` 那套纪律不要动。
3. **`mouse()` 的待答问题那一段改成**：

   ```rust
   if let Some(pending) = self.pending.as_ref() {
       match mouse.kind {
           MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
               let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
               let point = (mouse.column, mouse.row).into();
               match pending {
                   // 问卷没有边框：它占的就是排版给底部的那两块。
                   Pending::Questionnaire(_) => {
                       let in_bottom = self.last_regions_input_hits(point);
                       if in_bottom {
                           self.question_click(QuestionClick::Wheel(up));
                       } else {
                           self.pane.wheel(up);
                       }
                   }
                   // 模态有边框，所以它记矩形。
                   _ => {
                       let inside = self
                           .modal_rect
                           .is_some_and(|rect| rect.contains(point));
                       if !inside {
                           self.pane.wheel(up);
                       }
                   }
               }
           }
           MouseEventKind::Down(MouseButton::Left) => {
               self.question_click(QuestionClick::At(mouse.column, mouse.row))
           }
           _ => {}
       }
       return;
     }
   ```

   > 上面是形状，不是逐字抄。`last_regions_input_hits` 那个位置要的是「上一帧的 `Regions.input` 与 `Regions.hints` 两个矩形」——`TuiState` 目前**没有**存 `Regions`，实现时按最省的方式加：要么存一份 `Regions`（它是 `Copy`），要么存 `input` / `hints` 两个 `Rect`。**选后者**（两个字段够用，别把整个 `Regions` 塞进状态）。两个字段照样在 `draw_frame` 里从 `plan()` 的返回值填上，并遵守「这一帧真的画了什么就记什么」——问卷没画出来时它们是 `None`，滚轮于是落到转录上。

   注意：`self.pending.as_ref()` 与后面 `self.question_click(...)` 的可变借用不能共存，实现时把「要不要给问卷」先算成一个 `bool`，再分支去改状态（`question_click` 自己会 `as_mut`）。

4. **点击分派不动**：`question_click(QuestionClick::At(..))` 照旧先到覆盖层；`Some(pending)` 之外的 `_ => {}` 照旧。**尤其是**，一次落在转录上的点击**不许**因为这次改动而关掉一个还没回答的问题。
5. **详情覆盖层那一段不动**：它整块占着指针，滚轮滚详情主体——那是对的。
6. **`self.dirty = true`** 的位置与今天一致（进 `mouse()` 就置脏）。
7. **重复滚动**：`self.pane.wheel(up)` 的步长是 `WHEEL_ROWS = 3`，与今天转录自己的滚轮一致，不要另起一个步长。

## 测试

- **改写** `the_wheel_is_ignored_while_a_question_is_up`（`tests/render_layout.rs`，约 3186 行）：它断言的是被推翻的旧行为（**测试名本身也要改**，比如 `the_wheel_follows_the_pointer_while_a_question_is_up`）。新的四条：
  1. 中间模态立着、指针在**转录**上滚 → 视口上移（与没立着时同样读法：找转录第一个可见块）；
  2. 中间模态立着、指针在**模态矩形内**滚 → 视口**不动**；
  3. 问卷立着、指针在**转录**上滚 → 视口上移；
  4. 问卷立着、指针在**底部块**（输入区）内滚 → 高亮移动（`questionnaire` 的选中项变了）而视口不动。
- **新增**：模态没画出来的时候（终端小到 `panes.modal()` 返 `None`）滚轮落到转录上。
- `tests/ask_user_question_tui.rs` 里问卷滚轮的既有用例：确认它们仍然表达「指针在问卷里滚 = 挪高亮」，没有一条依赖「指针在别处也会挪高亮」。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警；`cargo fmt --check` 只留既有漂移。
- [ ] 真机：`ask_user_question` 弹出来时，鼠标放在转录上滚 → 转录滚；放到问卷上滚 → 选项列表动。权限确认弹出时，指针在弹窗外滚 → 转录滚。

## Comments

- 2026-10-01 落地。
- 状态里加了两个字段（`modal_rect`、`questionnaire_bottom`），都遵守「这一帧真的画了什么就记什么」：`draw_frame` 一进来两个都清成 `None`，`draw_modal` 拿到矩形时才填，问卷占着底部时才填。
- 问卷那一个是 `input` 与 `hints` 连同中间那条线的**一个连续矩形**（问卷没有边框，它占的就是排版给底部的那两块）；中间的模态记自己那个矩形（它有边框）。
- 点击的优先级一丝不变：覆盖层仍然先接点击，而落在别处的一次点击仍然不许关掉一个还没回答的问题。
- 用例：`the_wheel_is_ignored_while_a_question_is_up` 整条改写成 `the_wheel_follows_the_pointer_while_a_question_is_up`（覆盖层外滚转录、覆盖层内什么都不动）；新增 `the_wheel_over_the_transcript_scrolls_it_while_a_questionnaire_is_up`；`the_wheel_moves_the_questionnaire_highlight` 的指针坐标从 `(40, 10)` 挪到问卷底部块里的 `(41, 20)`（`wheel()` 辅助函数）。
- 票里那条「模态没画出来时滚轮落到转录」**没有写用例**：那几个提前返回的分支在 40×10 地板之上够不到，见票 01 的 Comments。
