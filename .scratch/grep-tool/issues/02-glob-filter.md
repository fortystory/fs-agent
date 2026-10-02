# 02 — `glob` 过滤

Type: implement
Status: done
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §3（参数面）、§2（范围）。票 01 已经让 `grep(pattern)`
> 跑通；这一票只加宽它的参数面，不碰权限、不碰遍历的忽略规则。

## 目标

同一个 pattern 能只搜匹配 glob 的文件（例如 `*.rs`），而不是让模型把文件名塞进正则里、
或者退回去拼 shell。

## 现状（2026-10-02 核实的形状，改前先复核）

- 工具在 [票 01](01-grep-tool-tracer-bullet.md) 建好：`spec()` 声明 `pattern`、
  `effect()` 恒 `ReadOnly`、`call()` 自己走 `ctx.cwd`。
- 遍历用的是 `ignore` crate（它的 `OverrideBuilder` 就是给「只搜这些 glob」用的，
  参考 `src/context/repo_map.rs` 那类按需取的工具里有没有现成的 glob 用法）。

## 落点

`src/tools/grep.rs`、以及票 01 新增的测试文件。

## 具体行为

1. **`spec()` 加一个可选参数 `glob`**（字符串，例如 `*.rs`），`pattern` 仍是唯一必填项。
2. **`glob` 只影响「搜哪些文件」**，不参与 pattern 的匹配 —— 交给 `ignore` 的覆盖规则处理，
   绝不用字符串拼接把它塞进正则。
3. **描述里补一句 `glob` 的用法**（工具声明进前缀缓存、一次定死，所以这话要和参数同时落）。
4. **非法 glob 报一条清楚的工具错误**（说明哪个 glob 不合法），不是 panic、也不是静默忽略。

## 验证

`cargo test` + `cargo clippy --all-targets`：

1. **过滤生效**：同一个 pattern 在两个后缀的文件里都命中时，带 `glob: "*.rs"` 只回 `.rs` 那条。
2. **不带就不过滤**：同一次调用去掉 `glob` 时两条都回（回归锚，防止顺手改了默认行为）。
3. **非法 glob**：传一个不合法的 glob，得到一条可读的工具错误，且流上是那一条
   `tool_call` 的结果（不 panic、不悬空）。

## 不做什么

- 不做 `path` 参数（范围写死 cwd，那是 §2 的决定）。
- 不做 glob 数组、不做否定 glob（`!`）之类的接口承诺：这一版 `glob` 是一个字符串。
- 不动忽略规则（`.gitignore` 与隐藏文件照旧，`glob` 不是「无视 `.gitignore`」的逃生口）。
