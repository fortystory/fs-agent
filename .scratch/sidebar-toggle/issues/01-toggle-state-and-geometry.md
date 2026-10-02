# 左栏开关的状态与几何：`Ctrl-O` 收起、意愿与宽度相乘

Type: implement
Status: done
Blocked by: —

> 规格：[`.scratch/sidebar-toggle/spec.md`](../spec.md) §1–§3、`Testing Decisions`。
> 本票动状态机与排版；提示行那一条文案归 [票 02](02-hint-line-entry.md)，文档与 `CONTEXT.md`
> 的词条回改归 [票 03](03-docs-and-sweep.md)。

## 目标

`TuiState` 多一位「用户想不想看见左栏」，`Ctrl-O` 切换它，并且**这一位要一路走进排版函数** ——
收起时主列拿回整屏（含分隔线那一列）、编辑器跟着变宽、页签命中矩形随帧消失；宽度 `< 80` 时
叫不回来，也不给任何反馈。

## 现状（2026-10-02 量的，改前先复核）

- `layout::plan`（`src/render/layout.rs:293`）里 `let tier = sidebar_tier(area.width);`（`:295`）是
  左栏去留的**唯一**判据；`sidebar` / `divide` / `main_x` / `main.width` 全部从它派生（`:305-308`）。
- `sidebar_tier(width) -> Option<u16>` 在 `:356`（`w < SIDEBAR_NARROW_FROM` 返 `None`）；
  `sidebar_content(width, content_rows)` 在 `:379`，`tier == None` 时给 `(SidebarKind::Hidden, 0)`。
- `main_width(width)` 在 `:369`；两个公开的宽度查询 `input_text_width(area)` 在 `:273`、
  `content_width(area)` 在 `:281`，都走 `main_width`。
- 调用点**一共 5 处，全在 `src/render/tui.rs`**：`layout::plan(self.area, 1)`（`:2095`，详情宽度）、
  `layout::plan(area, content_rows)`（`:3134`，`draw_frame` 主绘制）、`layout::content_width(area)`
  （`:2862`）、`layout::input_text_width(area)`（`:2866` 与 `:3922`）。
- `Key` 枚举在 `src/render/tui.rs:84`，`map_key` 在 `:122`（CONTROL 分支 `:123-141`，只认
  `c d a e u k w p n g j z`，其余 `_ => None`）。
- `TuiState::key` 在 `:2478`。前三个提前返回依次是：`CtrlZ`（`:2483`）、`replay`（`:2490`）、
  `detail_open`（`:2496`）；`expire_exit_gesture` 在 `:2512-2516`。
- `TuiState` 字段初始化的地方在 `:1396` 一带（`tab: Tab::Usage` 那一串）。
- **测试接缝是 `draw_frame`**：`tests/render_layout.rs` 从不直接调 `layout::plan`，所以签名改动
  **不返工任何既有测试**（改前 `grep -c 'plan(' tests/render_layout.rs` = 0）。

## 落点

`src/render/layout.rs`、`src/render/tui.rs`、`tests/render_layout.rs`。

## 具体行为

1. **`layout.rs` 的签名贯穿**（`wanted: bool`）：
   `sidebar_tier` / `sidebar_content` / `main_width` / `plan` / `content_width` / `input_text_width`。
   `!wanted` 时 `sidebar_tier` 返 `None`，**其余分支一个字不改** —— `plan` 里那几行派生照旧，
   于是 `wanted == false` 与今天 `w < 80` 的那一支逐格相同。
2. **`TuiState.sidebar_wanted: bool`**，初值 `true`；rustdoc 写明「只活在这一次进程里，与选中的
   页签同级；不落配置、不跨会话」。
3. **5 个调用点**补上意愿（`self.sidebar_wanted` / `state.sidebar_wanted`）。**别漏 `:3922`**
   （`input_text_width(frame.area())`，输入区绘制那处）。
4. **`Key::CtrlO`** + `map_key` 的 `'o' => Some(Key::CtrlO)`。
5. **`TuiState::key` 的分派位置**：`replay` 与 `detail_open` 两个提前返回**之后**、
   `expire_exit_gesture` **之前**：

   ```rust
   if key == Key::CtrlO {
       self.sidebar_wanted = !self.sidebar_wanted;
       return;
   }
   ```

   由此：空闲与 busy 生效；详情覆盖层、历史重放不生效；问卷 / `/` 菜单立着时生效；**不清举手**。
   这一段要带 rustdoc 说明为什么排在这里（`.scratch/sidebar-toggle/spec.md` §3 的表）。
6. **`layout.rs` 里那句「只由宽度决定」的 rustdoc 要改**（`sidebar_tier` 上面那段，`:354-355`）：
   改成「宽度说可行性、意愿说偏好，两者相乘」。

## 验证

`cargo test`（`tests/render_layout.rs` 用 `draw_frame` + `TestBackend`）+ `cargo clippy` + `cargo fmt`：

1. **默认帧逐格不变**：`sidebar_wanted = true` 的 120×24 帧与改动前相等（回归锚）。
2. **收起后主列拿回整屏**：120×24 收起时转录文本宽 = `w − 2`，整帧没有结构性分隔列（`┆`）。
3. **页签消失且不可点**：收起帧里没有 `调用量` / `todo` 标签；原先点标签的那个坐标不再切页。
4. **意愿 × 宽度**：40×10 与 60×24 下切换 `sidebar_wanted` 后**帧逐格相等**；80×24 切到 `false`
   主列变宽、切回 `true` 与默认帧相等。
5. **`todo` 常驻不受影响**：有过非空列表的会话收起后按回来，标签与页内容照旧；收起期间来一条
   `todo` 调用不会自己弹回。
6. **守卫**：`replay` 中按 `Ctrl-O` 不改帧；详情覆盖层立着时按 `Ctrl-O` 不改帧且不关覆盖层；
   busy（一次运行在飞）时按 `Ctrl-O` 生效。
7. **编辑器跟着走**：断言 `content_width(area, false) > content_width(area, true)`
   （或一条长草稿在收起后折行数变少）。

## Comments

- **落地（2026-10-02）**：全部落地，`cargo test` 全绿。

- **`layout.rs`**：`sidebar_tier` / `sidebar_content` / `main_width` / `plan` / `content_width` /
  `input_text_width` 六个函数带上 `sidebar_wanted`，`!wanted` 走与 `w < 80` 完全相同的那一支；
  模块 rustdoc 与 `sidebar_tier` / `main_width` / `sidebar_content` 的 rustdoc 把「只由宽度
  决定」改写成两者相乘。
- **`tui.rs`**：`Key::CtrlO`、`map_key` 的 `'o'`、`TuiState.sidebar_wanted`（初值 `true`）、
  `key()` 里排在 `replay` 与 `detail_open` 之后、`expire_exit_gesture` 之前的那个 toggle；
  5 个调用点（`plan` ×2、`content_width` ×1、`input_text_width` ×2）全部带上意愿。
- **测试**：`tests/render_layout.rs` 新增 8 条（toggle 往返、窄档无效、busy 生效、详情覆盖层
  拦住、问卷立着时生效、页签随帧消失、`todo` 不弹回且页内容还在、编辑器折行宽与转录正文宽），
  `tests/history_replay.rs` 新增 1 条（重放期间不生效、跑完恢复）。
- **「默认帧逐格不变」这条回归锚由既有断言承担**：`the_sidebar_has_two_widths_and_a_hidden_third`、
  `a_wide_terminal_draws_the_mark_the_sidebar_and_the_main_column`、
  `the_wide_sidebar_is_forty_columns_and_centres_the_mark`、
  `the_main_rules_stop_short_of_the_divide_column` 读的就是默认帧，本次改动前后都绿 —— 这就是
  「加开关没有顺手改默认外观」的证据。再写一条「默认帧 == 默认帧」只会是同义反复。
- **收尾审查（`/code-review` 双轴）之后改的**：`wanted` 与 `sidebar_wanted` 统一成后者（Standards
  轴的 Mysterious Name）；`fn sidebar_content` 的 rustdoc 把 spec 路径补全。Data Clumps
  （六个签名都带「空间 + 意愿」）**接受为判断题、不改**：那是真实的两层判据（可行性 + 偏好），
  包成一个类型只是把同一个布尔换个名字，而 spec §2 已经论证过为什么不把它们折成一个数。
