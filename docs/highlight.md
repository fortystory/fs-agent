# `src/render/highlight.rs`：语法高亮与 diff 着色的现状

**结论先行：这个模块现在没有生产消费者。** 它是**有意留下的**，不是漏删。这份文档记录它为什么
还在、什么会让它回来、什么会让它消失——以便将来读代码的人不必重新推一遍。

## 它是什么

`src/render/highlight.rs` 提供两层相互独立的着色：

- **diff 层**（`DiffTag` / `diff_tag`）：这一行是新增、删除、hunk 头还是上下文？
- **语法层**（`Class` / `highlight_rust` / `highlight_diff`）：这段代码是什么语法元素？

两层可以同时成立（一个新增行里也可以有关键字），调用方把 `Class::style()` patch 到
`DiffTag::style()` 上，而不是二选一。语法层走 `tree-sitter-highlight`（已经是本仓库的依赖；
注意 `Cargo.toml` 的注释说明 tree-sitter 的 Rust grammar 会编译一个 C parser，所以"纯 Rust
构建"这句话不要在这里重复）。

## 为什么现在没人用

两次相邻的决定叠加，把它的最后一个调用点删掉了：

1. **曾经**：TUI 的转录里直接显示工具输出，并用这个模块给输出上色（`src/render/tui.rs`
   里有一个 `highlighted()` 组合函数，调用 `highlight_diff`）。
2. **`.scratch/tui-ux/` 的 `grilling：折叠与详情覆盖层的交互契约`（票 02）** 决定：工具输出
   **不再直接显示**——转录里只留一行调用行，正文进**详情覆盖层**，而详情层显示的是**纯文本**
   （`── 输出 ──` 分节 + 换行，见 `src/render/tui.rs` 的 `detail_body`）。
3. 于是 `highlighted()` 被删除（tui-ux 实现的提交 `940cd43`），`highlight_diff` / `ansi_line`
   再也没有调用者。

现在只有两处引用它：模块自己的 `#[cfg(test)]`，以及 `tests/render_highlight.rs`。

## 事实核对（想自己确认时）

```sh
# 除模块自身外，src/ 里没有任何调用点：
grep -rn 'ansi_line\|highlight_rust\|highlight_diff\|Class::\|DiffTag::\|diff_tag' src/ \
  | grep -v '^src/render/highlight.rs'
# 空输出 = 确实没有生产消费者。

# 模块仍然被导出、仍然参与编译：
grep -n 'highlight' src/render/mod.rs
```

## 什么会让它回来

- **详情覆盖层里想要语法高亮**：那是把它接回去最自然的地方——`detail_body` 现在是
  `pane::wrap_text`，换成"先按语言高亮、再按宽度折行"即可。注意详情层的换行是按**显示列**
  折的（`pane::wrap_text`），高亮返回的是**按行**的 span 列表，接回去时要处理这个口径差。
- **live 的工具输出预览**：如果将来决定在转录里重新显示输出正文（那会推翻 tui-ux 票 02 的
  折叠决定），它会再次需要。

## 什么会让它消失

- **接受"详情层就是纯文本"**：那么语法高亮在这个产品里没有位置，模块连同
  `tree-sitter-highlight` 依赖一起删掉，`tests/render_highlight.rs` 一并删。
- 那时要注意：`tree-sitter` / `tree-sitter-rust` **不能一起删**——`src/context/repo_map.rs`
  在读侧用它们做符号抽取，那是另一条独立的用途。只有 `tree-sitter-highlight` 这一个
  依赖是这个模块独有的。

## 现在的状态是"暂时不管"

维护者的决定（2026-09-23）：**留注释、留这份文档、不动代码**。所以：

- 不要因为"没人用"就顺手删它——那是一个需要决定的事，不是清理。
- 也不要因为它还在就给新代码加调用——详情层显示纯文本是当前的决定。
- 如果它坏了（例如 tree-sitter 升级导致编译失败），修它的理由是"它仍在编译"，而不是"它在被使用"。
