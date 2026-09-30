# 探测、配置与 fail closed

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/sandbox/spec.md` §3（探测与可用性）、§7（配置）。
> 依赖票 01：探测的结果要喂给那个 `SandboxSpec`。

## 目标

组装期探一次 bubblewrap 可不可用；把探测结果与 `[sandbox]` 的配置一起变成注入值；不可用时让 `bash` 与动态工具**拒绝运行**并给出出路。

## 落点

`src/config.rs`（`[sandbox]` 节 + `SessionConfig`）、`src/tools/sandbox.rs`（探测）、`src/lib.rs`（组装期调用探测并注入）、`src/render/wording.rs`（失败文案）、`tests/` 下的配置与组装测试。

## 具体行为

1. **配置节**：
   ```toml
   [sandbox]
   mode = "bwrap"                                        # 或 "off"
   writable_roots = ["~/.cargo", "~/.rustup", "~/.cache"]
   ```
   遮罩目录与保护路径**写死**，不给旋钮（spec §7：它们是安全默认，不该被人为了顺手改松）。`~` 展开与相对路径解析走仓库现有的路径处理，不新造一套。
2. **探测**：跑 `bwrap --ro-bind / / --dev /dev --die-with-parent -- /bin/true`，**以退出码为准**。不用 `--version`——装了不等于能用。
   - 在 `PATH` 上找 `bwrap`，但**排除 cwd**（防有人往工作区里放一个假的）；
   - **`/proc` 建不起来也算不可用**（无特权容器里的典型症状）；
   - **组装期一次**，结果进注入值；每条命令不重探，也不理会 PATH 中途变化。
3. **不可用 = fail closed**：`bash` 与动态工具返回**工具错误**（不是命令结果），文案（中文，模型可见）写明「命令没有跑」以及两条出路：装 bubblewrap，或把 `[sandbox] mode` 设成 `"off"`。
4. **`mode = "off"`**：探测根本不跑，`wrap()` 退化成单位函数。这是显式放弃这层，不是降级。
5. 探测结果与配置进 `SessionConfig`，形状与 `bash_timeout_ms` 那批一致：会话配置在组装期变成注入值，工具永不伸手去够会话。

## 测试

- 配置解析：`mode` 两个合法值、缺省值、非法值报错、`writable_roots` 的 `~` 展开；
- 探测两条路：在临时 `PATH` 上放一个退出码可控的假 `bwrap` 脚本，断言可用 / 不可用；
- 不可用时：`bash` 与动态工具返回工具错误，文案里两条出路都在；
- `mode = "off"` 时**探测没有被调用**（假 bwrap 会写一个 sentinel 文件，断言它不存在）。
