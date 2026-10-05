# 07 — 把裁剪记账归还给 pane（prefactor）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1（两个视口与共享源）。
> **这是这一轮唯一的 prefactor**：它没有用户可见变化，但把「平行表与 pane 同步」从**巧合**变成**契约** —— 后面每一张涉及两个视口的票都靠它。

## 目标

`Pane::push` 告诉调用方它这一次丢掉了几条源行；绘制侧按这个数字裁平行表，**删掉今天那个独立数 `CAP` 的循环**。做完之后，行链接表、每源行索引与回合条跟 pane 的源行窗口在任何情况下都同进同出。

## 现状（改前先复核）

- 裁剪在 `pane` 的 `evict` 里执行（由 `push` 尾部调用），按 `CAP = 20 000` 丢**最旧的源行**，连带丢它在折行缓存里的那一段。
- 绘制侧另有一个 `while` **自己数 `CAP`** 裁 `links` 并连带裁回合条 —— 两处今天只是**碰巧同速**。
- 两份索引都以「源行下标」为准（点击命中与回合条焦点都用它），所以一旦两边不同步，点击与高亮会指错行。

## 落点

`src/render/pane.rs`（`push` / `evict` 的返回值）、`src/render/tui.rs`（平行表的裁剪那一条链与回合条的 prune）。

## 具体行为

1. `Pane::push` 返回本次因 `CAP` 丢掉的**源行数**（没丢就是 0）。
2. 绘制侧的裁剪改成「按返回的丢弃数裁」：行链接表、每源行索引、回合条的格与段首。
3. 删掉那个独立数 `CAP` 的循环。
4. 用户可见行为一个字不动：帧、键位、滚动语义、`CAP` 的值都不变。

## 验证

1. `cargo test` 全绿 —— 本票是纯重构，**既有帧断言不许改**（改了就说明行为变了）。
2. 新增单测：推过 `CAP` 之后平行表长度与 pane 的源行数相等；覆盖一次推入就丢多条的情形。
3. `cargo clippy` 干净、`cargo fmt --check` 只留既有漂移。

## 不做什么

- 不做「两个 pane」——那是[轨迹视图第一次活起来](09-trace-page-alive.md)的事。
- 不改 `CAP` 的值，也不改共享源的保留策略（它今天全量保留）。

## 作答（2026-10-05）

- `src/render/pane.rs`：`Pane::push` 返回本次丢掉的源行数，`evict` 返回它丢的条数（一次
  可以多条，按循环累计）；新增 `Pane::sources()` 给平行表对账用。
- `src/render/tui.rs`：`prune_links(dropped)` 改成按 `Pane::push` 报回来的数裁 `links` 与
  `turn_rail`，那个自己数 `pane::CAP` 的 `while` 循环删掉。
- 新增三条单测：`pane::tests::push_reports_the_source_lines_the_cap_dropped`、
  `pane::tests::evicting_can_drop_several_source_lines_at_once`、
  `tui::tests::the_link_table_keeps_pace_with_the_pane_at_the_cap`（推过上限后
  `links.len() == turn_rail.lines.len() == pane.sources()`）。
- `cargo test` 全绿（既有帧断言一字未改）、`cargo clippy --all-targets` 无警告、
  `cargo fmt --check` 无漂移。
