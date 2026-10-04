# 收尾：棘轮提到实测值、README 数字、最后一遍清扫

Type: implement
Status: done
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

## 评论

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

## 评论

**实现完成（2026-09-27）**。提交 `b423cc4`（`scripts/check-language.py` + `README.md`）。
维护者在动手前拍了两个阈值（下面前两条就是拍下来的）。

1. **`COMMENT_FLOOR` 提到精确实测值**：`src` **195 → 5,209**、`tests` **93 → 2,423**
   （用脚本自己那套数法量的：`//` 开头且含 CJK 的行）。注释里补了两句：它是**只许上升**的
   棘轮，以及确实要删代码时的正确做法是**显式下调并在提交信息里写明理由**，别让它悄悄漂。
   没留余量是有意的 —— 任何一行被改回英文就报红。
2. **`DOCS_MIN_RATIO` 逐份收到「实测 −2 点」**（原来是 12 份统一 30%）：`domain` 29、
   `issue-tracker` 29、`custom-tools` 30、`bash` 32、`triage-labels` 32、`executor` 38、
   `credentials` 39、`skills` 41、`discussion` 42、`render` 42、`repo-map` 43、
   `observability` 43。实测区间是 31.2%（`domain.md`，最低）– 45.5%（`observability.md`，
   最高）；留 2 点是因为文档里必然有英文标识符 / 路径 / 命令，插一段代码块会拉低比例，而
   整段散文被翻回英文则必须报红。余量的理由写在 `DOCS_MIN_RATIO` 上面。
3. **README《状态》的行数是真漂移，不是照抄旧值**：那一行由 `6186070`（09-27 00:12）写下，
   在语言迁移之前，而翻译让注释行数掉了下来 —— `src/` **31,462 → 29,680**、
   `tests/` **29,179 → 28,612**（`wc -l`），**757 条测试不变**。
4. **反证（护栏真的紧了吗）**：在 `git archive HEAD` 解出来的**隔离 checkout** 里做，不动
   工作区 —— 把 `src/` 与 `tests/` 各一条中文注释改回英文，两条棘轮同时报红
   （`5208 < 5209`、`2422 < 2423`）；再往 `docs/agents/domain.md` 追加 1,180 字符英文散文，
   占比检查报红（`15.8% < 29%`）；改回后全绿。三条检查各报各的，互不掩盖。
5. **最后一遍清扫的结果：没有需要翻的**。清单逐条过完：
   - `check-translation-batch.py --remaining` 只剩 **90 字符 / 2 条**，两条都不是散文
     （`custom_tool_name` 的格式模板、一条内置 `base_url`）；
   - `src/` 与 `tests/` 里**整行英文**的注释行共 56 条（≥4 个英文词的 44 条 + 恰好 3 个的
     12 条），逐条看过：全是引用 / 图表 / 标识符续行（`.scratch/*/spec.md` 的出处、
     `spec §15`、`//! <root>/<cwd-slug>/<session-id>/` 这类路径图、`[`crate::…`]` 链接、
     分节破折号行、`hook.pre -> permission gate -> …` 那种流水线图），**没有一条是散文**；
   - `docs/tui-manual-checklist.md` 的 ⑰（以及 ⑩）那两节「**待人工过一遍**」的标注**原样
     保留**，没有改口 —— 本票没有真终端可跑，它们仍然是待你手工验证的两节。
6. **验收**：`check-language.py` OK（且按上面第 4 条确认过它真的会报红）、`cargo test`
   **757 passed / 0 failed**、`cargo clippy --all-targets` 干净、`cargo fmt --check` 零漂移、
   `scripts/tui-startup-check.py` **12/12 GREEN**。本票落地后 `.scratch/language-migration/spec.md`
   的 `Status:` 与两张票的 `Status:` 一并收成 `done`（票面验收第 3 条）。
