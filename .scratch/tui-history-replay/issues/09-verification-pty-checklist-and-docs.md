# 09 — 验证迁移：pty 路径、手工清单与文档回改

Type: implement
Status: ready-for-agent
Blocked by: 06, 07, 08
Part of: ../map.md

**What to build:** 把这件事**只有真终端能确认**的部分接上，并把描述已实现系统的文档补齐：pty 脚本加一条 `--continue` 路径（同一临时数据目录里先造一个会话、再重开），手工清单新增 ⑭ 与退出项补一条，`docs/render.md` 补一节历史重播。重播**内容**的正确性留在 `cargo test`，本票只收终端归属、手感与文档。

**Blocked by:** 06、07、08 —— 清单里的「历史详情点击」与「分帧中途上滚」要等前两张落地；pty 的收敛断言也要功能完整才有意义。

**来源：** `.scratch/tui-history-replay/spec.md` 的 `Testing Decisions` 分层一节与 `Further Notes` 的文档回改清单。**注**：设计票写的编号是 ⑫，但 `tui-ux` 已占用 ⑫ / ⑬，实际落 **⑭**。

**验收：**

- [ ] pty 脚本新增 `--continue` 路径：先造会话（跑一次普通启动，或直接写最小 `log.jsonl`）再重开；断言**不崩**、重播**收敛**（进度行消失）、退出后终端**交还干净**（alt screen / 鼠标 / 粘贴 / termios）。
- [ ] 重播内容的正确性**不进** pty——那是 `cargo test` 的活。
- [ ] `docs/tui-manual-checklist.md` 新增 **⑭ `--continue` 重开**：真会话开一次看进度观感、历史详情点击、分帧中途上滚（预期无响应 / 吸底）、超大会话的启动手感。
- [ ] 手工清单退出项补一条「**重开后退出**」：`--continue` → `Ctrl-C` / `Ctrl-D` → 终端干净。
- [ ] `docs/render.md` 补一节历史重播：新的前端控制请求、历史行经同一条 apply、分隔行、分帧与进度行。
- [ ] 渲染层文案模块已落新一族（`history_progress` / `history_divider`，见 spec §10）；`docs/render.md` 不必逐字重复文案。
- [ ] spec 的 `Further Notes` 文档回改清单逐条清空（v1 spec 的 §19 / §11 已在 `/to-spec` 阶段折回）。
- [ ] `cargo test --all-targets` 基线复核（开工前与收尾各一次，不低于 **664 passed**）；`cargo clippy --all-targets` 干净。
