# 03: 转录：权限与计划模式

**What to build:** 涉及「我要不要放行」的那一整套出中文：权限询问、裁决（含决定来源的标签）、计划冲突的覆盖 / 追加 / 保留询问，以及这两类询问在输入行上的提示。这是用户真正要读懂才敢按键的地方。

Blocked by: 01

Status: done

**参考:** spec §Implementation Decisions（`.scratch/chinese-ui/spec.md`）、票 01（措辞模块）

- [x] 权限询问的措辞是中文
- [x] 权限裁决的措辞是中文，含**决定来源标签**（决定来源标签本来就有一个共享函数，它正是措辞模块的先例，本票把它收进去）
- [x] 计划模式冲突的覆盖 / 追加 / 保留询问是中文
- [x] 权限询问与计划冲突询问的**输入行提示**是中文
- [x] 工具名、请求 id、路径这类结构化内容保持原样不翻
- [x] 两种画家都覆盖；被碰到的既有断言改成断语义

**顺序:** 与其余 02–08 没有依赖，但都会往票 01 建的同一个措辞模块追加函数；两票同时改它会冲突，按号做或分批做。

## Comments

- 需求侧对应 spec 的 7–8、17 号 user story。
- 「决定来源」那条共享函数是本仓库**已经有**的半个措辞层的证据：它的文档注释就写着「plain 与 TUI 必须对一条裁决的说法达成一致」。本票把同一套做法推广到其余短语。
- 使用中发现：转录里的权限询问只印了 request id 与 tool call id，**没印工具名**——尽管 `PermissionAsked.request` 里一直带着 `tool` 与 `args`。事件流的形状因此补了唯一出处（`events::permission_format`，含读取用的 `tool_name` / `args` / `reason`），`Block::PermissionAsked` 改带 `tool_name` 与 `args`，两家画家、headless 与 `sessions show` 都据此显示 `权限询问：bash（command=…）`。**id 不进给人看的界面**（用户明确说 request id 没用），审计要的 id 留在事件流与 JSON 里；输入行同样改成工具名 + 具体调用内容（TUI 原来只印 request id，bash 的命令因此看不见）。
