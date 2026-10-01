# 种子材料：web 搜索工具

> **这不是 spec，也不是票。** 它是 2026-10-01 在一轮 `/ask-matt` 里记下的意向：给模型一个
> web 搜索工具。
> 还没被访谈、也没有票；想推进时走 `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
> **写就于 2026-10-01**；下面《现状》一节核实于同一天。

## 它要什么

模型遇到「这东西现在是什么样」的问题时能自己去查，而不是伸手问人或者靠记忆。

## 现状

没有：工具表是 `read_file` / `write_file` / `edit_file` / `bash` / `skill` / `repo_map` / `task`
/ `todo` / `ask_user_question` 加动态声明工具（`src/tools/mod.rs:65-79`），里面没有任何联网
工具。provider 层（`src/provider/`）只跟模型 API 说话。

## 待谈的分叉

1. 自建抓取 + 搜索，还是接一个搜索 API，还是用供应商自己的内建检索。
2. 结果怎么进上下文：截断规则（可复用 `src/context.rs` 的 `preview` 与指针约定）、引用格式。
3. 网络策略：沙箱只管文件写边界，[ADR 0006](../../docs/adr/0006-sandbox-by-bubblewrap.md) 明说
   网络不在这层 —— 那 web 工具的出口由谁把关，要不要单独一档开关。
4. 取回的内容是不可信外部数据，提示词层面怎么标（本仓库已有对工具输出不信任的先例吗？要查）。
