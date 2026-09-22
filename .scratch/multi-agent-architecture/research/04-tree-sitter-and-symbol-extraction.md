# fs-agent：tree-sitter 的 Rust 绑定、符号抽取与图排序（事实简报）

> 目的：为**票 12（repo map / 符号检索）**提供外部事实输入。本文件只报告**事实与来源**，不推荐方案、不选赢家、不下结论。
>
> 抓取日期：**2026-09-12（UTC）**。全部为 crates.io API 元数据、docs.rs、官方仓库源码 / README / Cargo.toml、官方文档站与官方 issue 的阅读，外加一组**明确标注的本地测量**；**未调用任何 LLM API**。
>
> 来源分级：只使用 ✅ 一手来源（项目官方文档、官方仓库源码、官方 issue、crates.io / docs.rs 元数据）。凡一手来源未写明者一律标 **⚪ 未证实**，**不做推断**。
>
> 测量方法：依赖闭包照票 08 的方法 —— `cargo tree -e normal -q --prefix none`，**默认特性**，去重后统计包数（含传递依赖，**不含 root、不含 dev/build 边**）。本仓库没有 `Cargo.toml`，因此所有 cargo 探针 crate 都建在 `/tmp` 下（仓库外），测完即删；仓库内除本文件外**未新增/修改任何文件**。测量环境：`cargo 1.94.0 (85eff7c80 2026-01-15)` / `rustc 1.94.0 (4a4ef493e 2026-03-02)`，`x86_64-unknown-linux-gnu`。
>
> 本地测量值（第 3.2、5 节中标为「本文实测」的部分）**不是一手来源声明**，只作量级参考；票面要求的一手数据点若在官方来源中不存在，已在第 6 节显式列为 ⚪。

---

## 0. 来源清单与来源 ID

后文用 ID 引用；完整 URL 见第 7 节。

### tree-sitter 本体（官方文档 / 元数据 / 仓库）

| ID | 来源 | 用途 |
| --- | --- | --- |
| TS-INTRO | `docs/src/index.md`（tree-sitter 官方文档首页） | 项目定位与设计目标 |
| TS-ADV | `docs/src/using-parsers/3-advanced-parsing.md` | **增量解析**语义、多语言、并发 |
| TS-QSYN | `docs/src/using-parsers/queries/1-syntax.md` | Query 的 S-expression 语法 |
| TS-QPRED | `docs/src/using-parsers/queries/3-predicates-and-directives.md` | 谓词 / 指令、CLI 与 binding 的分工 |
| TS-QAPI | `docs/src/using-parsers/queries/4-api.md` | Query 执行 API（C 面） |
| TS-NAV | `docs/src/4-code-navigation.md` | **`tags.scm` 约定（唯一一手文档）** |
| TS-ABI | `docs/src/using-parsers/7-abi-versions.md` | parser ABI 与运行时的兼容表 |
| TS-IMPL | `docs/src/5-implementation.md` | 生成器 / 运行时的分工 |
| TS-TAGCLI | `docs/src/cli/tags.md` | `tree-sitter tags` 命令与选项 |
| TS-RSAPI | docs.rs `tree-sitter` 0.27.0 各类型页 + tag `v0.27.0` 的 `lib/binding_rust/lib.rs` | **Rust API 形状（签名逐条）** |
| TS-RSBIND | `lib/binding_rust/README.md` | Rust 绑定基本用法 |
| TS-CRATE | crates.io API `/api/v1/crates/tree-sitter` | 版本、发布日期、MSRV、依赖边 |
| TS-CARGO | docs.rs 源码视图 `crate/tree-sitter/0.27.0/source/Cargo.toml` | `rust-version` / edition / links |
| TS-DEPS | docs.rs `tree-sitter` 0.27.0 crate 页依赖面板与 `/features` | 直接依赖边、feature 与默认特性 |
| TS-COMMITS | `github.com/tree-sitter/tree-sitter/commits/master.atom` | 仓库最后提交时间 |
| TS-I584 | tree-sitter issue **#584**（官方仓库 issue） | **parser.c 的 C 编译时间实测** |
| TS-I1799 | tree-sitter issue **#1799**（官方仓库 issue） | **生成 parser.c 的体积 / 生成耗时** |
| TS-I4042 | tree-sitter issue **#4042**（官方仓库 issue） | **Query 编译耗时**的极端数据点 |
| TS-RUST-README | `tree-sitter-rust` README | **Rust grammar 的解析速度一手数字** |
| TS-RUST-TAGS | tree-sitter-rust `queries/tags.scm` | Rust 的 tags 查询实际内容 |
| TS-RUST-CARGO | tree-sitter-rust `Cargo.toml` | 运行时依赖 / dev 依赖 |
| TS-RUST-LIB | `bindings/rust/lib.rs` | `LANGUAGE` / `TAGS_QUERY` 常量 |
| TS-RUST-JSON | tree-sitter-rust `tree-sitter.json`（发布包内） | scope / file-types / tags 路径 |
| TS-RUST-CRATE | crates.io API + `static.crates.io` 发布包 | grammar 版本、产物尺寸 |
| TSL-CRATE | crates.io API `/api/v1/crates/tree-sitter-language` | ABI 中间层 crate 的版本/MSRV |
| TS-RUST-COMMITS | `github.com/tree-sitter/tree-sitter-rust/commits/master.atom` | grammar 仓库最后提交时间 |

### 对照方案

| ID | 来源 | 用途 |
| --- | --- | --- |
| SY-CRATE | crates.io API `/api/v1/crates/syn` | 版本 / MSRV / 维护 |
| SY-README | `dtolnay/syn` README | 定位与能力声明 |
| SY-DOCS | docs.rs `syn` 3.0.5 crate 页 | 定位与能力声明（同 README） |
| SY-FILE | docs.rs `syn::File`、`syn::parse_file` | 全文件解析入口与 feature 约束 |
| SY-ITEM | docs.rs `syn::Item`、`syn::ItemFn` | **AST 粒度** |
| SY-FEAT | docs.rs `syn` 3.0.5 `/features` | feature 与默认特性 |
| SY-COMMITS | `github.com/dtolnay/syn/commits/master.atom` | 仓库最后提交时间 |
| RA-CLI | rust-analyzer `crates/rust-analyzer/src/cli.rs` | 批量命令与内存统计 |
| RA-SCIP | rust-analyzer `crates/rust-analyzer/src/cli/scip.rs` | **SCIP 输出实现** |
| RA-FLAGS | rust-analyzer `crates/rust-analyzer/src/cli/flags.rs` | `scip` / `analysis-stats` 子命令与参数 |
| RA-ARCH | `docs/book/src/contributing/architecture.md` | 索引/语义模型的官方描述 |
| RA-COMMITS | `github.com/rust-lang/rust-analyzer/commits/master.atom` | 仓库最后提交时间 |
| SCIP-README | `github.com/sourcegraph/scip` README | SCIP 协议定位与 indexer 列表 |
| CT-DOCS | `docs.ctags.io/en/latest/`（Universal Ctags 官方文档） | 项目定位与输出物 |
| CT-RUST | `universal-ctags/ctags` `parsers/rust.c` | Rust parser 抽取的 kind 列表 |
| CT-TAGS5 | docs.ctags.io `man/tags.5` | tags 文件格式（Vi tags 扩展） |
| CT-RELEASES | `github.com/universal-ctags/ctags/releases.atom` | 最新发布 |
| CT-COMMITS | `github.com/universal-ctags/ctags/commits/master.atom` | 仓库最后提交时间 |

### aider（repo map 的实现事实）

| ID | 来源 | 用途 |
| --- | --- | --- |
| AID-DOC | `aider.chat/docs/repomap.html`（官方文档） | repo map 的官方描述、`--map-tokens` 默认值 |
| AID-SRC | `Aider-AI/aider` `aider/repomap.py` | **图排序算法、tag 抽取、token 预算的源码事实** |
| AID-ARGS | `aider/args.py` | `--map-tokens` / `--map-refresh` CLI 定义 |
| AID-BC | `aider/coders/base_coder.py` | `map_tokens` 落地与 `map_mul_no_files` 传参 |
| AID-MODELS | `aider/models.py` | `get_repo_map_tokens()` 计算 |
| AID-MAIN | `aider/main.py` | 未显式给 `--map-tokens` 时的解析路径 |
| AID-COMMITS | `github.com/Aider-AI/aider/commits/main.atom` | 仓库最后提交时间 |

### petgraph

| ID | 来源 | 用途 |
| --- | --- | --- |
| PG-CRATE | crates.io API `/api/v1/crates/petgraph` | 版本 / MSRV / 维护 |
| PG-DOCS | docs.rs `petgraph` 0.8.3 crate 页 | 定位 |
| PG-ALGO | docs.rs `petgraph::algo` 模块索引 | **算法清单（是否有中心性排序）** |
| PG-PR | docs.rs `petgraph::algo::page_rank::page_rank` | **函数签名与文档** |
| PG-PPR | docs.rs `petgraph::algo::page_rank::parallel_page_rank` | 并行变体签名 |
| PG-COMMITS | `github.com/petgraph/petgraph/commits/master.atom` | 仓库最后提交时间 |

---

## 1. tree-sitter 的 Rust 绑定现状

### 1.1 crates.io 元数据（含 MSRV 与仓库最后提交）

| crate / 仓库 | 最新版本 | 发布日期（UTC） | `rust-version`(MSRV) | edition | license | 仓库最后提交（主分支） |
| --- | --- | --- | --- | --- | --- | --- |
| `tree-sitter` | **0.27.0** | **2026-08-30** | **1.90** | **2024** | MIT | **2026-09-11** |
| `tree-sitter`（上一线：0.26 系列末版） | 0.26.13 | 2026-08-23 | 1.77 | 2021 | MIT | — |
| `tree-sitter-language` | 0.1.8 | 2026-08-30 | 1.90 | — | — | 同 tree-sitter 仓库 |
| `tree-sitter-rust`（grammar） | **0.24.2** | **2026-03-27** | **未声明（`rust_version: None`）** | 2021 | MIT | **2026-03-27** |

来源：TS-CRATE（`max_version` / `versions[].created_at` / `versions[].rust_version` / `updated_at`）、TS-CARGO（`rust-version = "1.90"`、`edition = "2024"`、`links = "tree-sitter"`）、TSL-CRATE、TS-RUST-CRATE（`rust_version: None`）、TS-COMMITS / TS-RUST-COMMITS（GitHub commits Atom feed 的首条 `<updated>`）。

事实要点（不推断）：

- `tree-sitter` 0.27.0 的 **MSRV 跳到 1.90、edition 跳到 2024**；0.26 线最后一个版本 0.26.13 仍是 MSRV 1.77 / edition 2021。两条线的 MSRV 差 13 个 minor。
- `tree-sitter-rust` 0.24.2 **未声明 MSRV**（crates.io 元数据为空值），但其 `tree-sitter.json`/`Cargo.toml` 的 `edition = "2021"`。
- `tree-sitter-rust` 仓库的最后一次提交就是 `0.24.2` 版本提交本身（2026-03-27）；此后至抓取日（2026-09-12）约 5.5 个月无新提交。
- `tree-sitter` 仓库在抓取前一日（2026-09-11）仍有提交（`build(deps): bump wasmtime to v48.0.2`）。

### 1.2 依赖闭包（本文实测，方法见文首）

| 依赖目标（默认特性） | 闭包包数 | 闭包内包列表 |
| --- | --- | --- |
| `tree-sitter 0.27.0` | **8** | `tree-sitter`, `tree-sitter-language`, `regex`, `regex-automata`, `regex-syntax`, `aho-corasick`, `memchr`, `streaming-iterator` |
| `tree-sitter-rust 0.24.2` | **2** | `tree-sitter-rust`, `tree-sitter-language` |
| `tree-sitter 0.27.0` + `tree-sitter-rust 0.24.2` | **9** | 上面两者的并集（`tree-sitter-language` 共享，去重后 9） |
| `syn 3.0.5`（对照，见 2.1） | **4** | `syn`, `proc-macro2`, `quote`, `unicode-ident` |
| `petgraph 0.8.3`（对照，见 4.2） | **7** | `petgraph`, `fixedbitset`, `indexmap`, `equivalent`, `hashbrown 0.15.5`, `hashbrown 0.17.1`, `foldhash` |

方法：在 `/tmp` 下各建一个只含该依赖的探针 crate，`cargo tree -e normal -q --prefix none | sort -u`（排除 root），版本用 `=x.y.z` 固定。**这是测量值**，会随特性选择与时间变化。

**直接依赖边**（docs.rs 生成的 crate 页，非闭包）：TS-DEPS 列出 `tree-sitter 0.27.0` 的 normal 依赖为 `regex ^1.12.3`、`streaming-iterator ^0.1.9`、`tree-sitter-language ^0.1.8`，外加 **optional** 的 `wasmtime-c-api-impl ^48.0.1`；build 依赖为 `cc ^1.2.63`（必需）、`serde_json ^1.0.150`（build）、`bindgen ^0.72.1`（build，optional）。TS-DEPS 的 features 页显示：`default = ["std"]`，共 4 个 feature（`default`、`std`、`bindgen`、`wasm`、`wasmtime-c-api`），默认只启用 `std`。

- 事实：**默认特性下不引入 wasmtime**；`wasm` 才会拉 `wasmtime-c-api-impl`。⚪ `--features wasm` 的闭包大小本次**未测量**（见第 6 节）。
- 事实：`-e normal` 不计入 build 依赖，因此 `cc` / `serde_json` / `bindgen` 不出现在上表；但它们会在实际构建时下载与编译（TS-DEPS）。

### 1.3 Rust API 形状（`tree-sitter 0.27.0`，逐条来自源码）

下表签名逐字取自 TS-RSAPI（tag `v0.27.0` 的 `lib/binding_rust/lib.rs`）；docs.rs 上同名页面给出同样的公开签名。

**Parser（TS-RSAPI）**

| 方法 | 签名（节选） |
| --- | --- |
| 构造 | `pub fn new() -> Self` |
| 绑定语言 | `pub fn set_language(&mut self, language: &Language) -> Result<(), LanguageError>` |
| 取回语言 | `pub fn language(&self) -> Option<LanguageRef<'_>>` |
| 解析 | `pub fn parse(&mut self, text: impl AsRef<[u8]>, old_tree: Option<&Tree>) -> Option<Tree>` |
| 带回调解析 | `pub fn parse_with_options<T: AsRef<[u8]>, F: FnMut(usize, Point) -> T>(&mut self, callback: &mut F, old_tree: Option<&Tree>, options: Option<ParseOptions>) -> Option<Tree>` |
| 局部范围解析 | `pub fn set_included_ranges(&mut self, ranges: &[Range]) -> Result<(), IncludedRangesError>` |

**Language**

| 项 | 签名 / 事实 | 来源 |
| --- | --- | --- |
| 从 `LanguageFn` 构造 | `pub fn new(builder: LanguageFn) -> Self`（另有 `impl From<LanguageFn> for Language` 的用法） | TS-RSAPI |
| ABI 版本 | `pub fn abi_version(&self) -> usize` | TS-RSAPI |
| 节点类型名表 | `pub fn node_kind_for_id(&self, id: u16) -> Option<&str>`；`pub fn id_for_node_kind(&self, kind: &str, named: bool) -> u16` | TS-RSAPI |
| 字段名表 | `pub fn field_name_for_id(&self, field_id: u16) -> Option<&str>`；`pub fn field_id_for_name(&self, field_name: impl AsRef<[u8]>) -> Option<FieldId>` | TS-RSAPI |
| 超类型 | `pub fn subtypes_for_supertype(&self, supertype: u16) -> &[u16]` | TS-RSAPI |

**grammar crate 的导出与绑定用法**（TS-RUST-LIB，逐字）：

```rust
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_rust) };
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
pub const TAGS_QUERY: &str = include_str!("../../queries/tags.scm");
```

用法（TS-RUST-LIB 的文档注释）：`Parser::new()` → `parser.set_language(&tree_sitter_rust::LANGUAGE.into())` → `parser.parse(code, None)`。

**Node / TreeCursor（手工遍历面）**

| 项 | 签名（节选） | 来源 |
| --- | --- | --- |
| 节点类型 | `pub fn kind(&self) -> &'tree str`；`pub fn kind_id(&self) -> u16` | TS-RSAPI |
| 子节点 | `pub fn child(&self, i: u32) -> Option<Self>`；`pub fn named_child(&self, i: u32) -> Option<Self>`；`pub fn child_count(&self) -> u32` | TS-RSAPI |
| 按字段取子节点 | `pub fn child_by_field_name(&self, field_name: impl AsRef<[u8]>) -> Option<Self>`；`pub fn children_by_field_name<'cursor>(&self, field_name: &str, cursor: &'cursor mut TreeCursor<'tree>) -> impl Iterator<Item = Node<'tree>> + 'cursor` | TS-RSAPI |
| 取源码文本 | `pub fn byte_range(&self) -> core::ops::Range<usize>`；`pub fn utf8_text<'a>(&self, source: &'a [u8]) -> Result<&'a str, str::Utf8Error>` | TS-RSAPI |
| 游标 | `pub fn walk(&self) -> TreeCursor<'tree>`；`TreeCursor::goto_first_child/goto_next_sibling/goto_parent/field_name` 等 | TS-RSAPI |
| 错误标记 | `pub fn has_error(&self) -> bool`；`pub fn is_error(&self) -> bool`；`pub fn is_missing(&self) -> bool` | TS-RSAPI |

**Query / QueryCursor（S-expression 面）**

| 项 | 签名（节选） | 来源 |
| --- | --- | --- |
| 编译查询 | `pub fn new(language: &Language, source: &str) -> Result<Self, QueryError>` | TS-RSAPI |
| 捕获名表 | `pub const fn capture_names(&self) -> &[&str]`；`pub fn capture_index_for_name(&self, name: &str) -> Option<u32>` | TS-RSAPI |
| 谓词/设置 | `pub const fn property_predicates(&self, index: usize) -> &[(QueryProperty, bool)]`；`pub const fn property_settings(&self, index: usize) -> &[QueryProperty]`；`pub const fn general_predicates(&self, index: usize) -> &[QueryPredicate]` | TS-RSAPI |
| 关掉某捕获 | `pub fn disable_capture(&mut self, name: &str)`；`pub fn disable_pattern(&mut self, index: usize)` | TS-RSAPI |
| 执行查询 | `QueryCursor::matches<'query,'cursor:'query,'tree,T: TextProvider<I>, I: AsRef<[u8]>>(&'cursor mut self, query: &'query Query, node: Node<'tree>, text_provider: T) -> impl StreamingIterator<Item = QueryMatch<'cursor,'tree>>` | TS-RSAPI |
| 只要捕获 | `QueryCursor::captures(...)`（同参数形状） | TS-RSAPI |
| 范围限制 | `set_byte_range` / `set_point_range` / `set_containing_byte_range` / `set_containing_point_range`；`set_max_start_depth` | TS-RSAPI |
| 匹配限流 | `pub fn set_match_limit(&mut self, limit: u32)`；`pub fn did_exceed_match_limit(&self) -> bool` | TS-RSAPI |
| 匹配结果 | `QueryMatch::captures(&self) -> &[QueryCapture<'tree>]`；`nodes_for_capture_index(&self, capture_ix: u32)` | TS-RSAPI |

**两条路都存在，官方并未二选一**：Query 是「一对多匹配 + 捕获名」；Node/TreeCursor 是「按 kind/field 结构化下钻」。关于「取函数/类/方法签名」的**事实边界**：

- 官方 `tags.scm` 约定只抽取 **名字节点**（`@name`）与 **角色/种类捕获**（`@role.kind`），并可选 `@doc`；**没有任何一手文档定义「签名」这个捕获概念**（TS-NAV 的标准化词汇表见 1.4）。
- 取「签名文本」在 API 上等价于取某个 item 节点的源码切片：`Node::byte_range()` + `Node::utf8_text(source)`（TS-RSAPI）。官方未文档化任何「签名字符串」API。⚪ 是否有官方推荐做法属未文档化（第 6 节）。
- Query 的谓词/指令中，`#eq?` / `#match?` / `#any-of?` 等按捕获文本过滤；`set!`、`#select-adjacent!`、`#strip!` 是**指令**。TS-QPRED 明确说明：谓词与指令**不由 C 库处理**，「higher-level bindings to Tree-sitter like the Rust Crate or the WebAssembly binding **do implement a few common predicates**」。

### 1.4 官方 `tags.scm` 约定（唯一一手文档）

TS-NAV（`docs/src/4-code-navigation.md`，标题 "Code Navigation Systems"）是官方对 tag/符号抽取约定的文档。逐字要点：

- 「_Tagging_ is the act of identifying the entities that can be named in a program. We use Tree-sitter queries to find those entities.」
- 「The essence of a given tag lies in two pieces of data: the _role_ of the entity that is matched (i.e. whether it is a definition or a reference) and the _kind_ of that entity … Our convention is to use a syntax capture following the `@role.kind` capture name format, and another inner capture, always called `@name`, that pulls out the name of a given identifier.」
- 「You may optionally include a capture named `@doc` to bind a docstring.」
- 标准化词汇表（TS-NAV 原文表格）：`@definition.class`、`@definition.function`、`@definition.interface`、`@definition.method`、`@definition.module`、`@reference.call`、`@reference.class`、`@reference.implementation`；并注明「New applications may extend (or only recognize a subset of) these capture names」。
- **文件位置约定**：「It is expected that tag queries for a given language are located at `queries/tags.scm` in that language's repository.」（即约定，非强制机制）
- 测试约定：`tree-sitter test` 会用 `test/tags/` 下的文件，注释形如 `# ^ definition.module`（TS-NAV）。

CLI 侧（TS-TAGCLI）：`tree-sitter tags [OPTIONS] [PATHS]...`，选项含 `--scope`、`-t/--time`（"Print the time taken to generate tags for the file"）、`-q/--quiet`、`--paths`、`-p/--grammar-path`、`-r/--rebuild`。

**Rust grammar 实际抽取的内容**（TS-RUST-TAGS，1,194 bytes 的 `queries/tags.scm` 全文要点）：

| Rust 语法节点 | 捕获 | 备注 |
| --- | --- | --- |
| `struct_item` / `enum_item` / `union_item` / `type_item` | `@definition.class` + `@name: (type_identifier)` | 四类都映射成 `definition.class`（含 type alias） |
| `function_item`（顶层） | `@definition.function` | |
| `declaration_list` 内的 `function_item` | `@definition.method` | 即 impl/trait 块内的方法 |
| `trait_item` | `@definition.interface` | |
| `mod_item` | `@definition.module` | |
| `macro_definition` | `@definition.macro` | **注意：`@definition.macro` 不在 TS-NAV 的官方词汇表里** |
| `call_expression`（identifier / field_expression） | `@reference.call` | |
| `macro_invocation` | `@reference.call` | |
| `impl_item`（带 trait / 不带 trait） | `@reference.implementation` | |

事实边界：该文件**没有** `@reference.class`、没有局部变量/作用域相关捕获、没有签名相关捕获。TS-QPRED 说明 `#is?` / `#is-not? local` 这类属性由 **CLI** 用来判断「given node is a local variable or not」，即局部性判定不在查询文件本身。

**Query 编译耗时的一手数据点**（TS-I4042）：一个简单查询在大型 grammar 上「takes upwards of 4min to compile」，报告值 **249298.728ms（约 249 秒）**，查询为 `((while) (end) @end)`，触发因素是 grammar 里大量 node type 与 field 的组合；同一 issue 引用了另一例 `4000ms`。这是**极端个例**，不是典型分布。

### 1.5 增量解析：`Tree::edit` + 重新 parse 的官方语义

TS-ADV（官方文档）逐字要点：

- 「First, you must _edit_ the syntax tree, which adjusts the ranges of its nodes so that they stay in sync with the code.」（C 面：`void ts_tree_edit(TSTree *, const TSInputEdit *);`）
- 「Then, you can call `ts_parser_parse` again, **passing in the old tree**. This will create a new tree that **internally shares structure with the old tree**.」
- 「If you have stored any `TSNode` instances outside of the `TSTree`, you must update their positions separately, using the same `TSInputEdit` value」（C 面：`ts_node_edit`；文档同时说「Often, you'll just want to re-fetch nodes from the edited tree, in which case `ts_node_edit` is not needed.」）
- 并发：「Tree-sitter supports multi-threaded use cases by making syntax trees very cheap to copy.」；「copying a syntax tree just entails incrementing an atomic reference count」。**CAUTION 原文**：「Individual `TSTree` instances are _not_ thread safe; you must copy a tree if you want to use it on multiple threads simultaneously.」

Rust 侧对应 API（TS-RSAPI）：

| 语义 | Rust API |
| --- | --- |
| 编辑树 | `Tree::edit(&mut self, edit: &InputEdit)` —— 文档：「Edit the syntax tree to keep it in sync with source code that has been edited. You must describe the edit both in terms of byte offsets and in terms of row/column coordinates.」 |
| 重新解析 | `Parser::parse(text, Some(&old_tree))` |
| 变化区间 | `Tree::changed_ranges(&self, other: &Self) -> impl ExactSizeIterator<Item = Range>` —— 文档：「Compare this old edited syntax tree to a new syntax tree representing the same document, returning a sequence of ranges whose syntactic structure has changed. … Generally, you'll want to call this method right after calling one of the `Parser::parse` functions. Call it on the old tree that was passed to parse, and pass the new tree that was returned from `parse`.」 |
| 多语言范围 | `Parser::set_included_ranges(&[Range])`（TS-ADV 的多语言章节给了 ERB/HTML/Ruby 的完整示例） |

### 1.6 grammar 与运行时的版本兼容（ABI）

TS-ABI 原文兼容表（与本次相关部分）：

| tree-sitter 版本 | Min parser ABI | Max parser ABI |
| --- | --- | --- |
| `>=0.20.3, <=0.24` | 13 | 14 |
| `>=0.25` | 13 | **15** |

- `tree-sitter-rust 0.24.2` 的 `src/parser.c` 第 9 行：`#define LANGUAGE_VERSION 15`（TS-RUST-CRATE，发布包实测），`STATE_COUNT 3825`。
- 因此 `tree-sitter-rust 0.24.2`（ABI 15）可由 `tree-sitter 0.27.0`（支持 13–15）加载；这是两张一手事实的直接交集，非推断（TS-ABI + TS-RUST-CRATE）。
- `tree-sitter-rust` 的 `Cargo.toml` 里 **dev-dependencies** 是 `tree-sitter = "0.25"`（TS-RUST-CARGO）；运行依赖只有 `tree-sitter-language = "0.1"`。dev 边不计入 1.2 的闭包统计。

---

## 2. 对照方案（只报事实，不比较优劣）

### 2.1 `syn`

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 版本 / 日期 | 3.0.5，2026-09-04 | SY-CRATE |
| MSRV / edition | `rust-version = 1.71`，edition 2021 | SY-CRATE |
| 仓库最后提交 | 2026-09-06（`Resolve non_kebab_case_bins warning`） | SY-COMMITS |
| 定位（官方原文） | 「Syn is a parsing library for parsing a stream of Rust tokens into a syntax tree of Rust source code.」；「Currently this library is geared toward use in Rust procedural macros, but contains some APIs that may be useful more generally.」 | SY-README / SY-DOCS |
| 覆盖范围（官方原文） | 「Syn provides a syntax tree that can represent **most stable Rust source code and some unstable syntax**.」 | SY-README / SY-DOCS |
| 语言数 | 仅 Rust（crate 描述「Parser for Rust source code」） | SY-CRATE |
| 全文件入口 | `pub fn parse_file(content: & str) -> Result<File>`，**Available on crate features `full` and `parsing` only**；与 `parse_str::<File>` 的差别是处理 BOM 与 shebang | SY-FILE |
| AST 粒度 | `syn::File { shebang, frontmatter, attrs, items: Vec<Item> }`；`Item` 为 16 变体 enum（`Const`/`Enum`/`ExternCrate`/`Fn`/`ForeignMod`/`Impl`/`Macro`/`Mod`/`Static`/`Struct`/`Trait`/`TraitAlias`/`Type`/`Union`/`Use`/`Verbatim`），**`Available on crate feature full only`** | SY-FILE / SY-ITEM |
| 函数粒度示例 | `ItemFn { attrs, vis, modifiers, sig: Signature, block: Box<Block> }`（即签名与体都结构化，不只是名字） | SY-ITEM |
| feature | 11 个 feature，默认 5 个：`clone-impls`、`derive`、`parsing`、`printing`、`proc-macro`；`full`、`visit`、`visit-mut`、`fold`、`extra-traits`、`test` 非默认 | SY-FEAT |
| 依赖闭包（本文实测） | 4：`syn`, `proc-macro2`, `quote`, `unicode-ident` | 本文实测 |

事实边界：`parse_file` 返回 `Result`；官方文档**没有描述任何语法错误恢复机制**（对照 TS-QSYN / TS-RSAPI 中 tree-sitter 明确定义的 `ERROR` / `MISSING` 节点与 `has_error()`）。syn 在语法错误输入上的具体行为、以及是否做名字解析/跨文件引用，**官方文档未声明** → ⚪（第 6 节）。

### 2.2 rust-analyzer 的索引 / SCIP 输出

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 仓库最后提交 | 2026-09-11 | RA-COMMITS |
| 批量子命令 | `RustAnalyzerCmd` 枚举含 `LspServer`、`Parse`、`Symbols`、`Highlight`、`AnalysisStats`、`RunTests`、`RustcTests`、`Diagnostics`、`UnresolvedReferences`、`PrimeCaches`、`Ssr`、`Search`、`Lsif`、**`Scip`** | RA-FLAGS |
| `scip` 参数 | `Scip { path, output: Option<PathBuf>, config_path: Option<PathBuf>, exclude_vendored_libraries: bool, num_threads: Option<usize> }` | RA-FLAGS |
| `analysis-stats` 参数 | `AnalysisStats { path, output, randomize, parallel, only, with_deps, no_sysroot, no_test, disable_build_scripts, disable_proc_macros, proc_macro_srv, skip_lang_items, skip_lowering, skip_inference, skip_mir_stats, skip_data_layout, skip_const_eval, run_all_ide_things, run_term_search, … }` | RA-FLAGS |
| SCIP 生成路径（源码事实） | `let si = StaticIndex::compute(&analysis, vendored_libs_config);`；`LoadCargoConfig { load_out_dirs_from_check: true, with_proc_macro_server: ProcMacroServerChoice::Sysroot, prefill_caches: true, num_worker_threads: … }`；`load_workspace_at(root, &cargo_config, &load_cargo_config, …)` | RA-SCIP |
| 索引 / 语义模型（官方文档原文） | 「input data consists of a set of test files … and information about project structure, captured in the so called `CrateGraph`」；「The analyzer keeps all this input data **in memory and never does any IO**. Because the input data is source code, which typically measures in tens of megabytes at most, keeping everything in memory is OK.」；「This representation is fully "resolved": all expressions have types, all references are bound to declarations, etc.」；「The underlying engine makes sure that model is computed lazily (on-demand) and can be quickly updated for small modifications.」 | RA-ARCH |
| 内存统计能力 | `cli.rs` 内有 `print_memory_usage(host, vfs)`，输出 `per_query_memory_usage()` 与 `VFS` / `Unaccounted` / `Remaining` 行；另有 `RA_METRICS` 环境变量开关的 `METRIC:...` 输出 | RA-CLI |
| SCIP 协议定位 | 「SCIP (pronunciation: "skip") is a language-agnostic protocol for indexing source code, which can be used to power code navigation functionality such as Go to definition, Find references, and Find implementations.」；indexer 列表中包含「rust-analyzer: Rust」 | SCIP-README |

事实边界：本次核查的官方来源**没有给出** rust-analyzer 的索引耗时/内存实测数字，也没有给出「以可复用库形式发布（如 `ra_ap_*` crate）及 MSRV」的声明 → ⚪（第 6 节）。

### 2.3 `universal-ctags`

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 项目定位（官方原文） | 「Universal Ctags (abbreviated as u-ctags) is a maintained implementation of ctags. ctags generates an index (or tag) file of language objects found in source files for programming languages. This index makes it easy for text editors and other tools to locate the indexed items.」 | CT-DOCS |
| 实现形态 | C 项目：官方文档章节为 "Building ctags / Building with configure"；Rust 支持在 `parsers/rust.c`（含 `#include "general.h"` 等 C 头） | CT-DOCS / CT-RUST |
| 最新发布 | `p6.2.20260906.0`（releases feed 日期 2026-09-03） | CT-RELEASES |
| 仓库最后提交 | 2026-09-08 | CT-COMMITS |
| Rust 抽取的 kind（源码事实） | `K_MOD, K_STRUCT, K_TRAIT, K_IMPL, K_FN, K_ENUM, K_TYPE, K_STATIC, K_MACRO, K_FIELD, K_VARIANT, K_METHOD, K_CONST` | CT-RUST |
| 输出格式 | Vi tags 文件格式的扩展；官方 man `tags(5)` 页自述为「Vi tags file format extended in ctags projects」（内容为 Exuberant Ctags `FORMAT` 文件副本 + Universal Ctags 的 EXCEPTION/COMMENT 标记） | CT-TAGS5 |
| 扩展机制 | 官方文档列出 "fully extended optlib (a feature to define a new language parser from a command line)" 与 "Extending ctags with Regex parser (optlib)" | CT-DOCS |

事实边界：本次**没有**在官方来源中找到「ctags 对 Rust 的引用（reference）标签支持」「跨文件引用解析能力」「生成 tags 的耗时/内存」的一手声明 → ⚪（第 6 节）。

---

## 3. grammar crate（`tree-sitter-rust` 等）

### 3.1 版本、日期与维护状态

| grammar crate | 最新版本 | 最近发布（UTC） | `rust-version` | edition |
| --- | --- | --- | --- | --- |
| **tree-sitter-rust** | **0.24.2** | **2026-03-27** | 未声明 | 2021 |
| tree-sitter-c | 0.24.2 | 2026-04-22 | 未声明 | 2021 |
| tree-sitter-python | 0.25.0 | 2025-09-11 | 未声明 | 2021 |
| tree-sitter-javascript | 0.25.0 | 2025-09-01 | 未声明 | 2021 |
| tree-sitter-go | 0.25.0 | 2025-08-29 | 未声明 | 2021 |
| tree-sitter-typescript | 0.23.2 | 2024-11-11 | 未声明 | 2021 |

来源：crates.io API（`max_version` / `updated_at` / `versions[0].rust_version` / `versions[0].edition`）。**注**：`updated_at` 是「该 crate 最近一次发布任何版本」的时间。

`tree-sitter-rust` 的其他一手事实：

| 项 | 值 | 来源 |
| --- | --- | --- |
| 运行依赖 | `tree-sitter-language = "0.1"`（**不依赖 `tree-sitter`**） | TS-RUST-CARGO |
| build 依赖 | `cc = "1.1"`（编译 `src/parser.c`） | TS-RUST-CARGO |
| dev 依赖 | `tree-sitter = "0.25"` | TS-RUST-CARGO |
| lib 入口 | `[lib] path = "bindings/rust/lib.rs"` | TS-RUST-CARGO |
| grammar scope | `"scope": "source.rust"`，`"file-types": ["rs"]`，`"tags": ["queries/tags.scm"]` | TS-RUST-JSON |
| ABI | `LANGUAGE_VERSION 15`，`STATE_COUNT 3825` | TS-RUST-CRATE |
| 仓库最后提交 | 2026-03-27（即 0.24.2 发布提交） | TS-RUST-COMMITS |

### 3.2 编译时间与产物体积

**一手数据点（来自官方仓库 issue，均为第三方贡献者在官方 issue 中贴出的实测）**

| 数据点 | 数值 | 来源 |
| --- | --- | --- |
| 某 grammar（tree-sitter-wake）`parser.c` 的 C 编译时间 | ASCII：clang 0.5s / gcc 0.6s，parser.c **1.6 MiB**；启用 Unicode 标识符：clang 11.6s / gcc 18.4s，parser.c 2.3 MiB；引入单字符 lexer bug：clang **196.7s** / gcc **436.8s**，parser.c 4.5 MiB | TS-I584（2020-03-21 提交） |
| tree-sitter-sql 的 parser 生成 | 「it takes longer and longer to generate the parser (about **42s** on the latest commit), and the size of the generated `parser.c` file reaches **83M**」；表中多列为 commit 级实测（parser.c 从 3.95M 涨到 62.51M+） | TS-I1799 |
| Query 编译时间 | 单个查询在大 grammar 上 **249298.728ms**（≈249s）；处已说明触发条件 | TS-I4042 |

**`tree-sitter-rust 0.24.2` 发布包内的产物体积（`static.crates.io` 下载的 `.crate` 实测）**

| 文件 | 字节数 |
| --- | --- |
| `src/parser.c` | **6,505,510**（约 6.20 MiB） |
| `src/node-types.json` | 99,023 |
| `src/scanner.c` | 12,588 |
| `queries/tags.scm` | 1,194 |
| 整个 `.crate`（压缩包） | 368,844 |

**本文实测（本机数值，非一手来源声明）**

| 场景 | 数值 |
| --- | --- |
| 只依赖 `tree-sitter-rust 0.24.2`，`cargo build --release`（冷 `CARGO_HOME`，含下载 + build script + `cc` 编译 parser.c） | **5.268 s** |
| 只依赖 `tree-sitter 0.27.0`，`cargo build --release`（冷 `CARGO_HOME`，含 C 库编译） | **18.024 s** |
| 同时依赖 tree-sitter + grammar + syn + regex + petgraph 的 release 构建 | **21.013 s** |
| `tree-sitter-rust` 的产物 `libtree-sitter-rust.a` / `parser.o` | 1,155,394 B / 1,149,752 B |
| `tree-sitter-rust` 的 Rust `rlib` | 1,384,156 B |

- 事实：**一手来源没有给出 `tree-sitter-rust` 的编译时间数字**（TS-I584 是别的 grammar，TS-I1799 是 tree-sitter-sql 的生成时间与体积）→ 官方数字 ⚪（第 6 节）。
- 上表「本文实测」使用 release profile、单机、冷 `CARGO_HOME`，数字随机器与并行度变化，仅作量级参考。

---

## 4. 图排序要不要自己实现（只报事实）

### 4.1 aider 的实现事实（官方仓库源码 + 官方文档）

**算法与输入（AID-SRC `aider/repomap.py`）**：

| 项 | 源码事实 |
| --- | --- |
| 图库 | `import networkx as nx`（AID-SRC 顶部 import 位于 `get_ranked_tags` 内） |
| 图类型 | `G = nx.MultiDiGraph()` |
| 节点 | 文件（`rel_fname`） |
| 边方向 | `G.add_edge(referencer, definer, weight=…, ident=ident)` —— 由「引用者文件」指向「定义者文件」 |
| 边权 | `use_mul * num_refs`，其中 `num_refs = math.sqrt(num_refs)`（「scale down so high freq (low value) mentions don't dominate」）；若 `referencer in chat_rel_fnames` 则 `use_mul *= 50` |
| 无引用定义的自环 | `G.add_edge(definer, definer, weight=0.1, ident=ident)`，注释说明是为 tree-sitter 0.23.2 的 ruby 行为兜底 |
| 个性化向量 | `personalization` = 对 chat 文件 / 被提及文件 / 路径组件命中被提及标识符的文件，累加 `personalize`；`personalize = 100 / len(fnames)` |
| 排序调用（原文） | `ranked = nx.pagerank(G, weight="weight", personalization=personalization, dangling=personalization)`（无 personalization 时退化为 `nx.pagerank(G, weight="weight")`；`ZeroDivisionError` 时再退化一次） |
| 是否为整图迭代 | 是：直接对整张 `G` 调 `nx.pagerank`（PageRank 全图迭代），随后把每个源节点的 rank 按出边权重分摊到 `(dst, ident)` 上：`ranked_definitions[(dst, ident)] += data["rank"]`，再按 rank 逆序挑定义 |
| 是否「个人排序」 | 存在 per-file `personalization` 向量（偏向 chat / 被提及文件），但排序本身是全图 PageRank 迭代，不是逐文件独立打分 |
| tag 抽取 | `from grep_ast import TreeContext, filename_to_lang`、`from grep_ast.tsl import USING_TSL_PACK, get_language, get_parser`、`from tree_sitter import Query`；`get_parser(lang).parse(bytes(code,"utf-8"))` 后 `Query(language, query_scm)` + `QueryCursor`（代码显式兼容 tree-sitter 0.23.2 的旧 `query.captures` 与新 `QueryCursor` API） |
| tags 文件来源 | `query_scm = get_scm_fname(lang)`（来自 `grep_ast`），读该语言的 `tags.scm` 文本 |
| 捕获名约定 | 以 `name.definition.` / `name.reference.` 前缀判定 `def` / `ref` |
| defs-only 兜底 | 若看到了 defs 但没有任何 refs，用 `pygments` 的 `guess_lexer_for_filename` + `Token.Name` 词法回填 refs（注释举例 cpp） |
| tags 缓存 | `diskcache.Cache`（SQLite），缓存目录 `.aider.tags.cache.v{CACHE_VERSION}`，key 为文件路径、以 mtime 判失效；`CACHE_VERSION = 3`，`USING_TSL_PACK` 时为 `4` |
| 选择/裁剪 | 先按 rank 排序 definitions，再用**对 token 数的二分搜索**（`lower_bound/upper_bound/middle`，容差 `ok_err = 0.15`）挑前缀；`special_fnames`（`filter_important_files`）前置 |
| 刷新策略 | `refresh` 取值 `auto`（默认）/ `always` / `files` / `manual`；`auto` 下 `use_cache = self.map_processing_time > 1.0` |

**`--map-tokens` 默认值与「没加文件时扩张」的原文出处**：

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 官方文档原文 | 「The token budget is influenced by the `--map-tokens` switch, **which defaults to 1k tokens**. Aider adjusts the size of the repo map dynamically based on the state of the chat. It will usually stay within that setting's value. But **it does expand the repo map significantly at times, especially when no files have been added to the chat and aider needs to understand the entire repo as best as possible.**」 | AID-DOC |
| CLI 定义 | `--map-tokens`，`type=int`，`default=None`，help「Suggested number of tokens to use for repo map, use 0 to disable」 | AID-ARGS |
| CLI 默认解析路径 | `if args.map_tokens is None: map_tokens = main_model.get_repo_map_tokens()` | AID-MAIN |
| `get_repo_map_tokens()` | `map_tokens = 1024`；若有 `max_input_tokens`：`map_tokens = max_input_tokens / 8`，`min(map_tokens, 4096)`，`max(map_tokens, 1024)` | AID-MODELS |
| Coder 内兜底 | `if map_tokens is None: use_repo_map = main_model.use_repo_map; map_tokens = 1024` | AID-BC |
| 「无 chat 文件时扩张」的代码 | `RepoMap.__init__(map_tokens=1024, …, map_mul_no_files=8, refresh="auto")`；`padding = 4096`；`target = min(int(max_map_tokens * self.map_mul_no_files), self.max_context_window - padding)`；**`if not chat_files and self.max_context_window and target > 0: max_map_tokens = target`** | AID-SRC |
| 「过大」告警阈值 | `max_map_tokens = self.main_model.get_repo_map_tokens() * 2`，超过则告警「map-tokens > … is not recommended. Too much irrelevant code can confuse LLMs.」 | AID-BC |
| 仓库最后提交 | 2026-05-22 | AID-COMMITS |

事实边界：官方**文档**说默认 1k，而源码在未显式传参时用 `clamp(max_input_tokens/8, 1024, 4096)`——两者在 `max_input_tokens ≤ 8192` 时一致，大上下文模型下源码路径可以到 4096。文档未解释 4096 上限的由来 → ⚪（第 6 节）。

### 4.2 Rust 侧现成构件：`petgraph`

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 版本 / 日期 | 0.8.3，2025-09-30 | PG-CRATE |
| MSRV / edition | `rust-version = 1.64`，edition 2021 | PG-CRATE |
| 仓库最后提交 | 2026-09-06（`ci: Fix clippy (#1037)`） | PG-COMMITS |
| 定位（官方原文） | 「petgraph is a graph data structure library. … petgraph provides several graph types …, algorithms on those graphs, and functionality to output graphs in Graphviz format. **Both nodes and edges can have arbitrary associated data**, and edges may be either directed or undirected.」 | PG-DOCS |
| 依赖闭包（本文实测） | 7（见 1.2） | 本文实测 |
| **重要性/中心性排序 API** | `petgraph::algo::page_rank::page_rank` 与 `petgraph::algo::page_rank::parallel_page_rank` | PG-ALGO |
| `page_rank` 签名（逐字） | `pub fn page_rank<G, D>(graph: G, damping_factor: D, nb_iter: usize) -> Vec<D> where G: NodeCount + IntoEdges + NodeIndexable, D: UnitMeasure + Copy` | PG-PR |
| `page_rank` 文档要点 | 「Computes the ranks of every node in a graph using the Page Rank algorithm.」；参数 `damping_factor`（0.0–1.0）、`nb_iter`（number of iterations of the main loop）；返回「A `Vec` mapping each node index to its rank」；Panics 条件为 damping factor 不在 [0,1]；**Complexity：Time `O(n|V|²|E|)`，Auxiliary space `O(|V| + |E|)`** | PG-PR |
| 是否带权 / 是否个性化 | 文档签名**没有** `weight` 参数、**没有** `personalization` 参数；`IntoEdges` 只提供出边迭代，rank 计算不读边数据 | PG-PR |
| 并行变体 | `pub fn parallel_page_rank<G, D>(graph: G, damping_factor: D, nb_iter: usize, tol: Option<D>) -> Vec<D> where G: … + Sync, D: UnitMeasure + Copy + Send + Sync`（文档：「Parallel Page Rank algorithm. See `page_rank`.」） | PG-PPR |
| `algo` 模块的其余条目（索引实测） | 函数：`astar`、`bellman_ford`、`find_negative_cycle`、`bridges`、`dsatur_coloring`、`bidirectional_dijkstra`、`dijkstra`、`greedy_feedback_arc_set`、`floyd_warshall`、`is_isomorphic*`、`subgraph_isomorphisms_iter`、`johnson`（+`parallel_johnson`）、`k_shortest_path`、`greedy_matching`、`maximum_matching`、`maximal_cliques`、`dinics`、`ford_fulkerson`、`min_spanning_tree`、`min_spanning_tree_prim`、`page_rank`、`scc`、`kosaraju_scc`、`tarjan_scc`、`all_simple_paths`、`all_simple_paths_multi`、`spfa`、`steiner_tree`、`condensation`、`toposort`；子模块：`astar`、`bridges`、`coloring`、`dijkstra`、`dominators`、`isomorphism`、`johnson`、`matching`、`scc`、`spfa`、`tred`；struct：`Matching`、`TarjanScc`、`Cycle`、`DfsSpace` | PG-ALGO |

事实边界：`petgraph::algo` 模块索引中，**与「重要性/中心性排序」同类的只有 `page_rank` 与 `parallel_page_rank`**；未见 betweenness / closeness 等其他中心性函数。其文档化的 PageRank 形状**不含边权与个性化向量参数**（PG-PR / PG-PPR）。⚪ Rust 生态中是否存在其他带权/个性化 PageRank 构件，本次**未核查** → 第 6 节。

---

## 5. 整仓解析的代价（时间 / 内存）

**一手数据点（官方来源中能找到的全部）**

| 数据点 | 内容 | 来源 |
| --- | --- | --- |
| 单文件解析速度（Rust） | `tree-sitter-rust` README 原文：「**Speed** — When initially parsing a file, `tree-sitter-rust` takes around **two to three times** as long as rustc's hand-written parser.」示例：`examples/ast.rs` 共 2157 行；`rustc -Z unpretty=ast-tree -Z time-passes` 的 `parse_crate` 为 `0.002`；`tree-sitter parse examples/ast.rs --quiet --time` 为 **6.48 ms / 9908 bytes/ms** | TS-RUST-README |
| 增量更新 | 同页原文：「But if you _edit_ the file after parsing it, tree-sitter can generally _update_ the previous existing syntax tree to reflect your edit **in less than a millisecond**, thanks to its incremental parsing system.」 | TS-RUST-README |
| 解析速度定位 | 「**Fast** enough to parse on every keystroke in a text editor」 | TS-INTRO |
| parser 生成耗时（另一个 grammar） | tree-sitter-sql：约 42s 生成、parser.c 83MB（见 3.2） | TS-I1799 |
| parser.c 的 C 编译耗时 | 见 3.2 的 TS-I584 表 | TS-I584 |

**⚪ 一手来源中未找到的**：任何官方文档 / 基准 / issue 给出「解析一个中小型 Rust 仓库需要多久 / 多少内存」的**整仓**数字。上表只有**单文件**（2157 行 → 6.48ms）与**增量 <1ms** 两个官方数字，整仓口径 → ⚪（第 6 节）。

**本文实测（本机数值，非一手来源声明）**

探针程序：`tree-sitter 0.27.0` + `tree-sitter-rust 0.24.2`（release），语料 = 从 crates.io 取到的三个真实 crate 源码 `syn 3.0.5` + `petgraph 0.8.3` + `regex 1.13.1`；先用 `Parser::parse` 解析全部文件并**保留所有 Tree**（内存最坏情况），再对每棵树跑 `TAGS_QUERY`。

| 指标 | 第 1 次 | 第 2 次 |
| --- | --- | --- |
| 文件数 / 字节 / 行数 | 237 / 3,968,090 / 120,205 | 同 |
| 解析失败文件数 | 0 | 0 |
| 仅解析耗时（含读文件、保留全部 Tree） | **466 ms** | **395 ms** |
| 跑 `TAGS_QUERY` 耗时 | **228 ms** | **226 ms** |
| 抽到的 def / ref 数 | 11,388 / 31,179 | 同 |
| 进程峰值内存 `VmHWM` | **135,860 kB**（≈132.7 MiB） | 135,780 kB |

口径：`VmHWM` 含「全部 Tree 常驻 + 每次读入的文件字节 + 二进制本身」；语料是单个 crate 树，不含 git 历史、不含依赖展开。数字随机器、语料与是否保留 Tree 变化，仅作量级参考。

---

## 6. ⚪ 无法从主来源验证的清单

以下条目**一手来源没有写明**，本文不推断、不作为事实使用：

1. **`tree-sitter-rust` 自身的编译时间 / 产物体积数字** —— 官方仓库、文档与 issue 中未见该 grammar 的编译耗时数字（TS-I584 是别的 grammar，TS-I1799 是 tree-sitter-sql 的生成产物）。第 3.2 节的 5.268s 等数字是本文本机测量，**不是一手来源声明**。
2. **整仓解析的时间 / 内存** —— 官方只有单文件解析与增量更新的数字（TS-RUST-README），没有任何「解析 N 个文件/中小型仓库」的官方基准；第 5 节的整仓数字是本文本机测量。
3. **`tree-sitter --features wasm` 的依赖闭包** —— 只证实 `wasmtime-c-api-impl` 是 optional 依赖与 `wasm` feature（TS-DEPS），闭包大小未测。
4. **syn 在语法错误输入上的行为** —— `parse_file` 返回 `Result`（SY-FILE），但官方文档未描述任何错误恢复 / 部分 AST 机制；是否产出部分 AST **未证实**。
5. **syn 是否做名字解析 / 跨文件引用** —— 文档只描述「syntax tree」（SY-README / SY-DOCS），未声明名字解析能力（也无「不做」的声明）；**未证实**。
6. **rust-analyzer 是否以可复用库 crate 形式发布、其 MSRV 与索引实测耗时/内存** —— 本次来源（RA-CLI / RA-SCIP / RA-FLAGS / RA-ARCH）均未给出；**未证实**。
7. **universal-ctags 对 Rust 的引用标签 / 跨文件引用解析** —— 官方文档未见 Rust 专属页（`ctags-lang-rust` 7 页 404），`parsers/rust.c` 只列出 kind 列表；引用能力**未证实**。
8. **ctags 生成 tags 的耗时 / 内存** —— 官方来源未见任何实测数字；**未证实**。
9. **aider `get_repo_map_tokens()` 的 4096 上限由来，以及各模型实际走哪条默认路径** —— 源码事实为 `clamp(max_input/8, 1024, 4096)`（AID-MODELS），但官方文档只写「defaults to 1k tokens」（AID-DOC），理由与逐模型生效值**未文档化**。
10. **官方是否对「符号签名文本」「作用域/局部变量归属」提供 tags.scm 之外的抽取约定** —— TS-NAV 的词汇表只有 class/function/interface/method/module 与三类 reference，未见签名或作用域约定；**不存在**（在本次核查的官方文档范围内）即无法证实存在。
11. **Query 编译耗时的一般分布** —— 只有 TS-I4042 的极端个例（249s）与同 issue 引用的 4000ms；典型值**未证实**。
12. **除 petgraph 之外 Rust 生态中的带权 / 个性化 PageRank 构件** —— 本次只核查了 petgraph（PG-ALGO / PG-PR / PG-PPR）；其他 crate **未核查**。
13. **ABI 15 的 grammar 在后续 tree-sitter 版本中的支持时限** —— TS-ABI 只给当前 min/max 表与「older versions will be phased out over a deprecation period」的定性说明，没有时间表。
14. **`tree-sitter-rust` 是否「停止维护」** —— 只有提交时间事实（最后提交 2026-03-27，TS-RUST-COMMITS），仓库/文档**没有**维护状态声明；不得据此推断。

---

## 7. 来源一览（全部一手来源，抓取于 2026-09-12 UTC）

### tree-sitter 官方文档（`tree-sitter.github.io` / 仓库 `docs/src`）

| ID | URL |
| --- | --- |
| TS-INTRO | <https://tree-sitter.github.io/tree-sitter/> · <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/index.md> |
| TS-ADV | <https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html> · <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/using-parsers/3-advanced-parsing.md> |
| TS-QSYN | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/using-parsers/queries/1-syntax.md> |
| TS-QPRED | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/using-parsers/queries/3-predicates-and-directives.md> |
| TS-QAPI | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/using-parsers/queries/4-api.md> |
| TS-NAV | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/4-code-navigation.md> · <https://tree-sitter.github.io/tree-sitter/4-code-navigation.html> |
| TS-ABI | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/using-parsers/7-abi-versions.md> |
| TS-IMPL | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/5-implementation.md> |
| TS-TAGCLI | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/cli/tags.md> |
| TS-RSAPI | <https://docs.rs/tree-sitter/0.27.0/tree_sitter/>（`struct.Parser` / `struct.Language` / `struct.Query` / `struct.QueryCursor` / `struct.Tree` / `struct.Node` / `struct.TreeCursor`） · <https://raw.githubusercontent.com/tree-sitter/tree-sitter/v0.27.0/lib/binding_rust/lib.rs> |
| TS-RSBIND | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/lib/binding_rust/README.md> |
| TS-CRATE | <https://crates.io/api/v1/crates/tree-sitter> |
| TS-CARGO | <https://docs.rs/crate/tree-sitter/0.27.0/source/Cargo.toml> |
| TS-DEPS | <https://docs.rs/crate/tree-sitter/0.27.0> · <https://docs.rs/crate/tree-sitter/0.27.0/features> |
| TS-COMMITS | <https://github.com/tree-sitter/tree-sitter/commits/master.atom> |
| TS-I584 | <https://github.com/tree-sitter/tree-sitter/issues/584> |
| TS-I1799 | <https://github.com/tree-sitter/tree-sitter/issues/1799> |
| TS-I4042 | <https://github.com/tree-sitter/tree-sitter/issues/4042> |
| TS-RUST-README | <https://raw.githubusercontent.com/tree-sitter/tree-sitter-rust/master/README.md> |
| TS-RUST-TAGS | <https://raw.githubusercontent.com/tree-sitter/tree-sitter-rust/master/queries/tags.scm> |
| TS-RUST-CARGO | <https://raw.githubusercontent.com/tree-sitter/tree-sitter-rust/master/Cargo.toml> |
| TS-RUST-LIB | <https://raw.githubusercontent.com/tree-sitter/tree-sitter-rust/master/bindings/rust/lib.rs> |
| TS-RUST-JSON | <https://static.crates.io/crates/tree-sitter-rust/tree-sitter-rust-0.24.2.crate>（包内 `tree-sitter.json`） |
| TS-RUST-CRATE | <https://crates.io/api/v1/crates/tree-sitter-rust> · <https://static.crates.io/crates/tree-sitter-rust/tree-sitter-rust-0.24.2.crate> |
| TSL-CRATE | <https://crates.io/api/v1/crates/tree-sitter-language> |
| TS-RUST-COMMITS | <https://github.com/tree-sitter/tree-sitter-rust/commits/master.atom> |

### 对照方案

| ID | URL |
| --- | --- |
| SY-CRATE | <https://crates.io/api/v1/crates/syn> |
| SY-README | <https://raw.githubusercontent.com/dtolnay/syn/master/README.md> |
| SY-DOCS | <https://docs.rs/syn/3.0.5/syn/> |
| SY-FILE | <https://docs.rs/syn/3.0.5/syn/struct.File.html> · <https://docs.rs/syn/3.0.5/syn/fn.parse_file.html> |
| SY-ITEM | <https://docs.rs/syn/3.0.5/syn/enum.Item.html> · <https://docs.rs/syn/3.0.5/syn/struct.ItemFn.html> |
| SY-FEAT | <https://docs.rs/crate/syn/3.0.5/features> |
| SY-COMMITS | <https://github.com/dtolnay/syn/commits/master.atom> |
| RA-CLI | <https://raw.githubusercontent.com/rust-lang/rust-analyzer/master/crates/rust-analyzer/src/cli.rs> |
| RA-SCIP | <https://raw.githubusercontent.com/rust-lang/rust-analyzer/master/crates/rust-analyzer/src/cli/scip.rs> |
| RA-FLAGS | <https://raw.githubusercontent.com/rust-lang/rust-analyzer/master/crates/rust-analyzer/src/cli/flags.rs> |
| RA-ARCH | <https://raw.githubusercontent.com/rust-lang/rust-analyzer/master/docs/book/src/contributing/architecture.md> |
| RA-COMMITS | <https://github.com/rust-lang/rust-analyzer/commits/master.atom> |
| SCIP-README | <https://raw.githubusercontent.com/sourcegraph/scip/main/README.md> |
| CT-DOCS | <https://docs.ctags.io/en/latest/> |
| CT-RUST | <https://raw.githubusercontent.com/universal-ctags/ctags/master/parsers/rust.c> |
| CT-TAGS5 | <https://docs.ctags.io/en/latest/man/tags.5.html> |
| CT-RELEASES | <https://github.com/universal-ctags/ctags/releases.atom> |
| CT-COMMITS | <https://github.com/universal-ctags/ctags/commits/master.atom> |

### aider

| ID | URL |
| --- | --- |
| AID-DOC | <https://aider.chat/docs/repomap.html> |
| AID-SRC | <https://raw.githubusercontent.com/Aider-AI/aider/main/aider/repomap.py> |
| AID-ARGS | <https://raw.githubusercontent.com/Aider-AI/aider/main/aider/args.py> |
| AID-BC | <https://raw.githubusercontent.com/Aider-AI/aider/main/aider/coders/base_coder.py> |
| AID-MODELS | <https://raw.githubusercontent.com/Aider-AI/aider/main/aider/models.py> |
| AID-MAIN | <https://raw.githubusercontent.com/Aider-AI/aider/main/aider/main.py> |
| AID-COMMITS | <https://github.com/Aider-AI/aider/commits/main.atom> |

### petgraph

| ID | URL |
| --- | --- |
| PG-CRATE | <https://crates.io/api/v1/crates/petgraph> |
| PG-DOCS | <https://docs.rs/petgraph/0.8.3/petgraph/> |
| PG-ALGO | <https://docs.rs/petgraph/0.8.3/petgraph/algo/index.html> |
| PG-PR | <https://docs.rs/petgraph/0.8.3/petgraph/algo/page_rank/fn.page_rank.html> |
| PG-PPR | <https://docs.rs/petgraph/0.8.3/petgraph/algo/page_rank/fn.parallel_page_rank.html> |
| PG-COMMITS | <https://github.com/petgraph/petgraph/commits/master.atom> |

---

## 8. 数字速查（全部事实，不含结论）

| 事实 | 值 | 来源 |
| --- | --- | --- |
| `tree-sitter` 最新版 / 日期 / MSRV | 0.27.0 / 2026-08-30 / 1.90（edition 2024） | TS-CRATE, TS-CARGO |
| `tree-sitter` 上一线 MSRV | 0.26.13 → 1.77（edition 2021） | TS-CRATE |
| `tree-sitter` 默认特性依赖闭包 | 8 包 | 本文实测 |
| `tree-sitter` + `tree-sitter-rust` 闭包 | 9 包 | 本文实测 |
| `tree-sitter-rust` 最新版 / 日期 / 最后提交 | 0.24.2 / 2026-03-27 / 2026-03-27 | TS-RUST-CRATE, TS-RUST-COMMITS |
| `tree-sitter-rust` 运行依赖 | 只有 `tree-sitter-language 0.1` | TS-RUST-CARGO |
| `tree-sitter-rust` parser ABI / parser.c 体积 | ABI 15 / 6,505,510 B | TS-RUST-CRATE |
| `tags.scm`（Rust）体积 | 1,194 B | TS-RUST-CRATE |
| Rust 单文件解析（官方示例） | 2157 行 → 6.48 ms（9908 bytes/ms） | TS-RUST-README |
| Rust 增量更新（官方表述） | 「less than a millisecond」 | TS-RUST-README |
| 整仓解析（本文实测） | 237 文件 / 3.97 MB / 120k 行 → 解析 395–466 ms；tags 查询 226–228 ms；VmHWM ≈133 MiB | 本文实测 |
| grammar 编译（本文实测） | `tree-sitter-rust` release 构建 5.268 s，产物 `libtree-sitter-rust.a` 1,155,394 B | 本文实测 |
| syn | 3.0.5 / MSRV 1.71 / 闭包 4 包 | SY-CRATE, 本文实测 |
| petgraph | 0.8.3 / MSRV 1.64 / 闭包 7 包；`algo::page_rank(graph, damping_factor, nb_iter) -> Vec<D>`（无 weight / personalization 参数） | PG-CRATE, PG-PR, 本文实测 |
| aider 排序 | `nx.pagerank(G, weight="weight", personalization=…, dangling=…)`，`G = nx.MultiDiGraph()` | AID-SRC |
| aider `--map-tokens` | 文档「defaults to 1k tokens」；源码未传参时 `clamp(max_input/8, 1024, 4096)`；`map_mul_no_files = 8` | AID-DOC, AID-MODELS, AID-SRC |
