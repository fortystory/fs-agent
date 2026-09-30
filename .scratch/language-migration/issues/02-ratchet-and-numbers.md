# 收尾：棘轮提到实测值、README 数字、最后一遍清扫

Type: implement
Status: ready-for-agent
Blocked by: 01

> 规格：`.scratch/language-migration/spec.md`。票 01 落地后才做（棘轮与行数都要在内容定稿后量）。

## 目标

把迁移收干净：护栏的棘轮按实测值收紧，README 里还没跟的数字收一次，然后扫一遍剩下的零星英文散文。

## 具体行为

1. **棘轮提到实测值**（`scripts/check-language.py` 的 `COMMENT_FLOOR`）：现在是迁移前基线 `{"src": 195, "tests": 93}`；实测已是「`src` 5,209 / `tests` 2,423」一档。提到实测值（或留一点点余量），并在注释里写明「只许上升」。
2. **`docs/*.md` 的中文占比下限**（`DOCS_MIN_RATIO`，现统一 30%）：按实测值复核一遍，把明显有余量的（很多是 40%+）提到贴近实测的水平，让「有一份被翻回去」能真的报红。
3. **README 的《状态》一节**：`src/` / `tests/` 的行数与测试条数按最终值收一次（`wc -l` 与 `cargo test` 的 passed 合计）。
4. **最后一遍清扫**：
   - `python3 scripts/check-translation-batch.py --remaining` 应当只剩票 01 之外的零星项；
   - 逐条判断 `src/` 与 `tests/` 里那些**只有引用 / 图表 / 标识符的续行注释**（`/// （spec §15）。`、`//! <root>/<cwd-slug>/<session-id>/` 这类）：它们该留就留，但如果有哪条其实是散文只是被断行断成了这样，翻掉。
   - `docs/tui-manual-checklist.md` 的 ⑰ 若仍未人工过一遍，保持它「待人工验证」的标注，别改口。
5. **收尾提交**：一次 `git commit`（中文信息，例如 `docs(language): 棘轮与数字收到实测值，迁移收尾`）。

## 验收

- `python3 scripts/check-language.py` OK（且新的棘轮确实是收紧后的值：故意把一条中文注释改回英文，它应当报红）。
- `cargo test` 757/0、`clippy` 干净、`cargo fmt --check` 零漂移、`scripts/tui-startup-check.py` GREEN。
- `.scratch/language-migration/spec.md` 的 `Status:` 收成 `done`，并把两张票的 `Status:` 一并收成 `done`。

## 不做什么

- 不动模型可见 / 进流的那一侧；不往白名单里加东西。
- 不为「让检查通过」而放宽阈值：抬高下限是收紧，绝不是放宽。

## Comments

**阻塞已解除（2026-09-27）**：票 01 落地于 `76a7eb1`，内容定稿，所以本票要量的棘轮与行数现在
是稳定值。开工前先用这几条命令现查（数字别照抄本票或 spec 里的旧值）：

- `python3 scripts/check-translation-batch.py --remaining` —— 现在只剩 **90 字符**（`src/config.rs`
  里 `custom_tool_name` 的格式模板与一条内置 `base_url`，两条都不是散文），所以第 4 条那种
  「零星项」的清扫面很小：真正要看的是 `src/` 与 `tests/` 里那些**只有引用 / 图表 / 标识符**的
  续行注释；
- 中文注释行实测值（棘轮要提到它）；
- `wc -l src/*.rs src/**/*.rs` 与 `cargo test` 的 passed 合计（README《状态》一节）；
- `docs/*.md` 各自的中文占比（`DOCS_MIN_RATIO` 的收紧依据）。

验收的 757/0、clippy、fmt、`tui-startup-check.py` 四条在本票落地时仍应全部成立（票 01 落地时
它们都成立）。
