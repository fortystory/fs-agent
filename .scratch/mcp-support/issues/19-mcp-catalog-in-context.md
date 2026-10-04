# 19 — MCP 加载结果进上下文，并且那条记录可点开详情

Type: implement
Status: done
Part of: ../map.md
Blocked by: 18

> 来源：维护者 2026-10-03 的直接要求（原话：「把 mcp 也加一条类似 `[上下文注入：技能清单]` 的记录，
> 并且可以点击查看详情，列出加载的 mcp 数据」）。规格参照 [spec.md](../spec.md) §7 与技能清单那条
> 注入的先例。

## 目标

组装期连上的 MCP server（以及没连上的那几台与原因）以**一条上下文注入**的形式进流 —— 与
`[上下文注入：技能清单]` 同形 —— 而且那行在 TUI 里**点得开**，详情列出这次会话加载的 MCP 数据。

## 具体行为

1. **新的事件来源**：`ContextSource::McpCatalog`（措辞「MCP 加载」）。
2. **内容**（服务层生成，模型看到的就是它）：
   - 每台已配置 server 的名字与 transport；
   - 连上的：标「已连接」+ 三处配置事实（过不过沙箱 / 两个信任位 / 可写根条数）；
   - 没连上的：标「连不上」+ 结构化错误码与人话原因；
   - `[mcp] enabled = false` 或一台 server 都没配时**不注入**（零影响）。
3. **注入时机**：组装之后、第一个回合之前（与技能清单同一条路：`Harness::inject_context`）。
4. **可点开详情**：那行是 `RenderedLine::linked(...)`，详情是新的 `DetailKind::Context`，主体是同一段
   数据（按宽度折行）。
5. 不列 `env` / `headers` 的**值**（那是秘密，值级打码是另一条线）；只列键名与条数。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt` + `python3 scripts/check-language.py`：

1. 服务层的文本：两台（一好一坏）时两边都在，坏的那台带 `MCP_SERVER_UNAVAILABLE` 与原因；
   关掉开关时是 `None`。
2. 事件流：注入确实落成一条 `ContextInjected { source: McpCatalog }`，内容是那段文本。
3. 转录：`Block::ContextInjected` 带上内容（详情要用）。
4. 详情：点那一行能打开，主体里含 server 名。
5. 回归：既有注入（AGENTS.md / 技能清单 / 目标清单）一条不变。

## 不做什么

- 不在组装期问每台 server 的工具清单（那是 `mcp_list` 现问的事，问了既贵又会漂）；
- 不列 `env` / `headers` 的值；
- 不为它新增第二个详情视图（复用既有的那一个）。

## 评论

- 2026-10-03 落地（`Status: done`）。落点：`src/events.rs`（`ContextSource::McpCatalog`）、
  `src/mcp/mod.rs`（`McpService::catalog_text`）、`src/cli.rs`（组装之后、第一个回合之前
  `inject_context`）、`src/render/wording.rs`（「MCP 加载」）、`src/context.rs`（模型侧的
  `[注入] MCP 加载`）、`src/render/transcript.rs`（`Block::ContextInjected` 带上 `content`）、
  `src/render/tui.rs`（那行改成 `RenderedLine::linked(...)`、新的 `DetailKind::Context`、
  `detail_body` 的分支）、`tests/mcp_catalog.rs`（新，3 条）、`src/render/tui.rs` 的单元测试
  （+1 条）、`docs/mcp.md`（新增「组装期的加载记录」一节）。
- **代价取向**：注入的内容只讲**加载事实**（server 名、transport、连接状态、三处可信配置的
  档位），不问工具清单 —— 那是 `mcp_list` 现问的事，问了既贵又会漂；`env` / `headers` 只报
  键名与条数，值一个字不列（那是秘密，值级打码是另一条线）。
- **可点那一步**：转录那一行只画来源名，详情主体是同一段文本。单元测试钉住「这行带链接」
  与「详情正文里有 server 名」；在真终端里点一下看那一眼仍属真机走查。
- 验证：`cargo test`（1,146 passed / 62 个 test binary / 0 failed）· `cargo clippy --all-targets`
  零 warning · `cargo fmt` · `python3 scripts/check-language.py` OK。
