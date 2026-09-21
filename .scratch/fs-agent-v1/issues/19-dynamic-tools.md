# 19: 动态工具注册

**What to build:** 用户能在配置里声明一个外部命令当工具，模型用起来和内置工具一样——而它**无法谎报只读**（声明语法里根本没有这个字段）、**也无法注入 shell**（argv 数组、不展开）。

Blocked by: 04

Status: done

**参考:** spec §14（动态工具注册）、§12（规则兜底）

- [x] 声明复用同一套工具声明形状（JSON Schema 原样就是 provider 的线级形状，零翻译），住在 `~/.config/fs-agent/config.toml`
- [x] 声明语法里**没有**副作用类别字段 ⇒「谎报只读」没有地方可说；**`effect()` 恒为 `Exclusive`**（走既有最保守路径，并行判定不需要特例）
- [x] 执行形态 = **argv 数组、不经 shell**；参数按**整个 argv 元素**替换（缺席则省略该元素；数组 / 对象序列化成一个元素、**不展开**）
- [x] 命名空间 `custom__<ns>__<tool>`，内建名**永不含** `__` ⇒「有 `__` ⟺ 自定义工具」是**词法可判定的**谓词
- [x] 启动即全局可见、**组装期固定**（不中途增删工具表——那会废掉前缀缓存）
- [x] 有**超时**与**进程树终止**
- [x] 走同一套权限门、按**声明名**匹配；一条 `Tool("custom__*")` 规则兜底
- [x] 代价写进文档：真正只读的动态工具也全局串行，且 `read-before-edit` 覆盖不到它（`WritePaths` 未知）——所以约束落在权限门上
- [x] 测试：参数里塞 `; rm -rf /`、`$(...)`、反引号之类的内容，断言**没有** shell 解释发生

## Comments

**落地（2026-09-22）。**

- **配置**：`[tools.<命名空间>.<工具>]`（`description` / `command` / `parameters` / `timeout_ms?`）解析成 `config::ToolDeclaration`，线级名 `custom__<ns>__<tool>`；`Config.tools` 按名字稳定排序（工具数组是缓存前缀的一部分）。校验全部发生在**启动期**：命名空间 / 工具名非空、只用 `[A-Za-z0-9_-]`、不含 `__`；`command` 非空且**首个元素是字面量**（程序不能是占位符）；每个 `{p}` 必须在 `parameters.properties` 里声明。超时默认 30s、上限 600s（`DEFAULT_/MAX_CUSTOM_TOOL_TIMEOUT_MS`）。
- **工具**：`tools::custom::CustomTool`——`spec()` 把声明原样交给 provider（零翻译）、`effect()` 恒为 `Exclusive`、`command()` 返回解析后的 argv（`CommandPrefix` 与 `rm` 断路器据此工作）、`call()` 复用**共享进程运行器**。`is_custom_tool(name) = name.contains("__")`；内建名不含 `__`（有测试钉住）。
- **进程运行器**：把 `bash` 的 argv 直启 / 超时 / **进程组 SIGKILL** 抽到 `src/tools/process.rs`，bash 行为不变（票 20 的测试仍绿），动态工具与它共用同一个实现——两处不会漂移。
- **组装期固定**：`tools::with_dynamic(&config.tools)`；`probe` 与交互会话都走它。中途不增删工具表。
- **权限门**：不需要新代码——`Scope::Tool("custom__*")` 与 `CommandPrefix` 本来就按声明名 / argv 匹配。测试两条：端到端（`readonly` 拒绝动态工具）与纯函数（一条 `custom__*` deny 规则覆盖所有动态工具）。
- **文档**：`docs/custom-tools.md`（声明形状、整元素替换表、不经 shell、超时与进程树、校验、词法判定、`Exclusive` 的两项代价）+ `CONTEXT.md` 词条「动态工具（CustomTool）」。
- **测试**：`tests/custom_tools.rs` 18 例——声明解析与命名、内建名不含 `__`、schema 原样、`Exclusive`、整元素 / 缺席省略 / 数组对象单元素 / 部分占位符字面量、四类校验错误、无 shell 注入（`; touch pwned` / `$(...)` / 反引号都只是文本、标记文件未生成）、超时后进程树真的没了、`readonly` 拒绝、`custom__*` 规则。
