# 左栏身份从顶上下移一行

Type: implement
Status: done

> 规格：`.scratch/tui-chrome/spec.md` §6（a）。
> 来源：第一轮落地之后维护者在真终端上提的两条微调之一 —— 「`fs-agent` 向下一行，和上方留一行空行」。

## 目标

左栏的身份（宽档的标记、窄档的文字身份）不再紧贴屏幕顶上：它上面留一行空行。页签条与左栏各页跟着一起下移，竖分隔列不动。

## 落点

`src/render/layout.rs`（新增 `SIDEBAR_TOP_GAP`、`plan` 里 `sidebar` 的 y 与高度、`sidebar_content` 收到的内容行）；`tests/render_layout.rs` 里以左栏行号为锚点的断言。

## 具体行为

1. **新增 `const SIDEBAR_TOP_GAP: u16 = 1;`**，rustdoc 写明这是**花掉的行**、不是白得的。
2. **`plan()` 里**：
   - `let sidebar_rows = area.height.saturating_sub(SIDEBAR_TOP_GAP);`
   - `sidebar_content(area.width, sidebar_rows)`（原来传 `area.height`）；
   - `sidebar = Rect::new(area.x, area.y + SIDEBAR_TOP_GAP, tier, sidebar_rows)`。
3. **`tabs` 与 `sidebar_page` 一行不改**：它们从 `sidebar` 派生（`sidebar.y + kind.rows() + …`），所以自动跟着下移 —— 这条改动的全部算术就上面那一处。
4. **主列、转录、状态行、输入区、提示行、分隔列都不动**：留白只花左栏自己的行。
5. **高度阶梯重新取档**（内容行 = `h − 1`）：`h ≥ 15` 是标记、`11–14` 退成文字身份、`h = 10`（地板）连身份都放下 —— 第一轮里够不到的「退到什么都不画」那一档因为这一行的让出**又回来了**。

## 测试

- 左栏行号锚点整体 +1：`a_wide_terminal_draws_the_mark_the_sidebar_and_the_main_column` 的标记行（0/4 → 1/5）与页签行（5/6 → 6/7）；`the_wide_sidebar_is_forty_columns_and_centres_the_mark` 的三格（y=0 → y=1）；`the_mark_is_lit_from_above_and_only_on_the_wide_rung` 的两格；`mark_colours` / `dash_cell` 的行范围（`0..=4` → `1..=5`）。
- **新增**：`the_sidebar_gives_up_its_identity_then_its_fields_as_it_shrinks` 的档位表加上 `(10, "none", 6)` —— 那是这一行让出来的那一档。

## 验收

- [x] `cargo test --all` 全绿（44 个套件）。
- [x] `cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩既有的那一处漂移。
- [x] 真机：120×24 下标记从第 1 行起、80×14 下 `fs-agent 0.1.0` 从第 1 行起。

## 评论

- 2026-10-01 落地，**与票 08 同批**（同一条真机反馈的两半）。
- 实测：120×24 下标记占第 1 到 5 行、页签条在 6/7/8、读数页 9..14；80×14 下身份在第 1 行、页签条 2/3/4、读数页 5..10。
- 左栏的内容高度从 `h` 变 `h − 1`，所以高度阶梯整体抬了一档的门槛；`h = 10` 的地板下内容行是 9，正好是「页签条 + 上下文/token/回合」的地板，于是「退到什么都不画」那一档重新可达 —— 档位表按实测重取。

- 2026-10-07 追记：身份阶梯从三档变四档（小篆块字 → 收起来那版 → 文字身份 → 隐藏）。本票记的 `120×24` 数字一个字没变 —— 24 行装不下二十三行的小篆，那句"标记占第 1 到 5 行"如今描述的是收起来的那一版（[ADR 0014](../../../docs/adr/0014-renamed-to-heng.md) 的补注）。
