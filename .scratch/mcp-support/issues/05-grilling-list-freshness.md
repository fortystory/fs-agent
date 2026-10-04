# grilling：`mcp_list` 的新鲜度与 `listChanged`

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

元工具方案把「工具表冻结 vs `notifications/tools/list_changed`」那个撞点绕开了（冻结项 5）：
表里只有两个固定的工具，server 侧的增删只影响 `mcp_list` 的**返回**。所以问题从「表要不要变」
变成了「**那份清单要多新**」：

- **每次现问**：`mcp_list` 每次都向 server 发一次 `tools/list`。简单、永远最新，代价是每条
  server 一次往返（本地 stdio 便宜，远端 HTTP 不便宜）。
- **订阅 `listChanged`**：2026-07-28 把它收进了 `subscriptions/listen` 的 opt-in 过滤
  （`toolsListChanged` 等）。新鲜度好、往返少，代价是要维持一条订阅流与一份缓存。
- **会话内缓存一次**：第一次 `mcp_list` 拿到就缓存到会话结束 —— 最简单，但 server 中途换工具
  就看不见了（而且没有任何提示）。

顺带要定：缓存（若做）在 **server 崩了 / 重连之后**怎么失效 —— 这与冻结项 14（不自动重连）
是一致的：重连发生在**下一次调用时**，那时清单也就自然重取。

## 作答

**每次现问**：`mcp_list` 每次都向 server 发一次 `tools/list`（`server?` 给了就只问那一台）。

- **不订阅** `subscriptions/listen` 的 `toolsListChanged`：那要维持一条订阅流、一份缓存与一套失效
  规则，而收益只是省掉本地 stdio 的一次往返（远端 HTTP 的那点延迟由这个工具的低频调用吸收）。
- **不做会话内缓存**：server 中途换工具就看不见，而且没有任何提示 —— 与「错误要如实说」的调性不符。

**这条让「缓存失效」那片雾自己散掉**：没有缓存就没有失效。与冻结项 14（不自动重连）也不矛盾：
那条管的是「不替你把崩掉的 server 悄悄拉起来、同会话内也不重试」，而每次现问意味着**它天然会探到
server 的当前状态** —— 连不上就是一条结构化错误，模型看得到。
