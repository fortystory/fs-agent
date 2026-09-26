# 动态工具

动态工具是**在 `config.toml` 里声明的一条命令**（spec §14）。模型看它和内建工具一样：
有名字、有描述、有 JSON Schema，调用它就在事件流上产出一条普通工具结果。不写 Rust 就能
加一个工具而仍然安全，原因在于这份声明**说不出**什么。

## 声明

```toml
# ~/.config/fs-agent/config.toml

[tools.git.status]
description = "Show the working tree status as porcelain."
command = ["git", "status", "--porcelain"]
parameters = { type = "object", properties = {} }

[tools.git.log]
description = "Show recent commits for one path."
command = ["git", "log", "--oneline", "-n", "{count}", "--", "{path}"]
parameters = { type = "object", properties = { count = { type = "integer" }, path = { type = "string" } }, required = ["path"] }
timeout_ms = 10000
```

表是 `[tools.<命名空间>.<工具>]`；线级名是 **`custom__<命名空间>__<工具>`**（上面的
`custom__git__log`）。

- `description` 与 `parameters` 就是 provider 收到的声明，**原样** —— JSON Schema 本身
  就是线级形状，所以没有一个会漂移的翻译层。
- `command` 是 argv 模板。
- `timeout_ms` 可选；默认 30s、上限 600s。

这张表在**组装期**定死，进程活多久就固定多久。没有任何东西会在会话中途增删工具：`tools`
数组是缓存前缀的一部分，改它等于把那张前缀丢掉。

## 没有副作用类别字段

动态工具的 `effect()` **恒为 `Exclusive`**。声明语法里没有副作用类别的字段，所以「这一个
其实是只读的」没有地方可说 —— 模型谎报不了，手滑也写不出。

代价是实的，而且写明：

- **真正只读**的动态工具照样全局串行，不能和任何别的东西并发；
- **`read-before-edit` 覆盖不到它**，因为这个工具从不声明可核对的 `WritePaths` 集合。

剩下的约束落在**权限门**上，它看得见这次调用的声明名与 argv：

```toml
# (illustrative rule shape; rules are evaluated by the gate)
# deny everything declared in configuration
tool = "custom__*"
```

一条 `Tool("custom__*")` 规则就是所有已声明工具的兜底。权限门还会拿 `CommandPrefix` 去
匹配解析后的 argv，与它对 `bash` 的做法一样。

## argv 替换：整个元素，永不过 shell

命令是**直接** spawn 的 —— harness 与程序之间没有 shell —— 而一个参数替换的是**一整个
argv 元素**：

| 模板元素 | 参数 | 结果元素 |
| --- | --- | --- |
| `"{path}"` | `"src/lib.rs"` | `src/lib.rs` |
| `"{path}"` | 缺席或 `null` | （这个元素被省略） |
| `"{tags}"` | `["a", "b"]` | `["a","b"]` —— 一个元素，**不**展开 |
| `"{opts}"` | `{"k": 1}` | `{"k":1}` —— 一个元素 |
| `"--path={path}"` | 任何值 | `--path={path}` —— 字面量，不做局部拼接 |
| `"status"` | — | `status` |

因为值是直接 spawn 的一个元素，参数里的 shell 元字符只是文本：`; rm -rf /`、`$(...)` 与
反引号原样交给程序，永不被重新解析。（一条**自己选择**要跑 shell 的声明 ——
`command = ["bash", "-c", ...]` —— 那是用户自己要的；harness 不额外加一层。）

缺席的参数是**省略**而不是留空，所以一个本来就是旗标的模板元素，在模型没发它时干净地
消失。

## 超时与进程树

每次调用都跑在一个挂钟上限之下。到期时 SIGKILL 打给**整个进程组**，不只是一个直接子
进程，所以把子进程放到后台的命令不会把它们留下。调用被 drop（一次取消）时同一道守卫也
会触发。这就是 `bash` 用的那个 runner（`src/tools/process.rs`），所以两个工具不会各自
漂移。

非零退出是**结果**，不是错误：模型看见退出码、stdout 与 stderr，自己决定怎么办。只有
spawn 或等待失败才算错误，因为只有那时才没有东西可报。

## 校验

声明在**启动时**校验，所以写错是启动期报错，而不是模型第一次调用时的意外：

- 命名空间或工具名为空、用了 `[A-Za-z0-9_-]` 之外的字符、或含 `__`（那会让名字有歧义）
  的，一律拒；
- `command` 不能为空，而且它的**第一个**元素（程序）必须是字面量，不能是 `{placeholder}`；
- 每个 `{name}` 占位符都必须声明在 `parameters.properties` 里。

## 名字谓词

内建名字永不包含 `__`，而每个声明出来的工具都有。所以

> 这个名字含 `__` **当且仅当**它来自配置

光看字符串就能判定 —— 这正是权限门、渲染器与用户的规则能谈论「动态工具」而不必查注册表
的原因。
