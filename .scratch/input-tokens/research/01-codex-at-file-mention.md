# research：OpenAI Codex CLI 的 `@` 提文件（file mention）实际怎么做

## 这份文件是什么

`fs-agent` 要在主输入区加一个功能：打字时输入 `@` 触发工作区文件选择，参考 codex 的做法。这份文件只回答一件事：**codex 到底怎么做**——触发条件、候选来源、过滤算法、交互与键位、插入形态、边界处理、以及这套状态机落在哪一层。

**只查事实，不做决定。** 本文件不写设计建议，也不替本仓库拍方案；**结尾那一节只做事实层面的对照**（「codex 的候选来自 X，而本仓库今天没有 X」），不是选型结论。

读者是接下来要就这个功能做访谈的人。**① 结论先行**是九问速答，②–⑩ 逐问给代码出处，**横向对比表**并排看 codex 自己的两条路径（`mentions_v2` 开 / 关），**⑪ 反直觉点**是复用价值最高的一节，**⑫** 写清楚这份文件回答不了什么。

**特别提示**：codex 里 `@` 同时是两样东西——**文件补全的触发符**（TUI 层）与**插件文本提及的 sigil**（`PLUGIN_TEXT_MENTION_SIGIL`，跨 crate 的纯文本语法）。提问里的「`@` 提文件」只覆盖前者；后者的存在会影响插入形态与提交语义（见 ⑥ 与 ⑧）。这是提问里**假设不成立**的第一处。

## 来源、版本与观察时点

**观察时点：2026-10-04（UTC 14:50–15:50 前后）**。

| 对象 | 版本 / 时点 | 取证方式 |
| --- | --- | --- |
| `openai/codex` | commit `afb436df8b70bb5bc57b86d9a3e829968988cd21`（2026-10-04 07:12:06 +0000），subject `Honor server reasoning summary defaults in new TUI threads (#50811)` | `git clone --depth 1 --filter=blob:none --sparse`，sparse 只取 `codex-rs/`、`docs/`；**下文所有 `codex-rs/...:行号` 都是这个 commit 的行号** |
| npm `@openai/codex` | `latest = 0.160.0`（registry 时间 2026-10-01T20:26:19.286Z） | `https://registry.npmjs.org/@openai/codex` 的 `dist-tags` / `time` |
| Codex 官方文档 | 抓取于 2026-10-04 | `learn.chatgpt.com` 的 Markdown 版本（页面 URL 加 `.md`），见下 |

**引用格式**：源码写 `codex-rs/<路径>:<行号>`（同一节内首次出现给全，之后给相对路径）；官方文档给 `[标题](URL)` 并指出小节名。凡是我没有实际读到的行号一律不给；找不到一手说法的写「**一手未验证**」。

我实际读过的官方文档页只有两页：

- [Developer commands](https://learn.chatgpt.com/docs/developer-commands.md?surface=cli)（小节 “Interactive shortcuts”、“Highlight files with `/mention`”）
- [CLI customization](https://learn.chatgpt.com/docs/cli-customization.md)（通读，**没有**讲 `@` 或补全键位）

仓库自带的 `docs/` 我全文搜过 `mention`、`` `@` ``、`@file`，**没有任何讲 `@` 提及的段落**——所以这一节的事实基本只能从源码取。

---

## ① 结论先行（九问速答）

1. **触发**：`@` 必须落在**一个以空白起始的 token 的开头**。判据是「光标前最后一个空白字符之后」或缓冲区起点（`codex-rs/tui/src/bottom_pane/chat_composer/completion_target.rs:140-144`）；换行算空白，所以**行首也触发**，但换行不提供「跨行亲和」（`completion_target_tests.rs:6-22`）。**提问里「任意位置」的假设不成立**：在 `src/` 后面再按 `@`（前一字符是 `/`）**不触发**，因为向后取 token 有一条显式前置条件「前一个字符是空白或不存在」（`completion_target.rs:255-265`），而向前取的 token 必须以 `@` 开头（`:23-26`）。
2. **候选来源**：默认（`mentions_v2` 开启）的弹窗副标题就写着 “Files, directories, plugins, skills and tasks”（`mentions_v2/render.rs:78`），即**文件、目录、插件、技能、任务**五类。文件与目录来自**会话工作目录**（`config.cwd`）的一次 `ignore::WalkBuilder` 遍历（`tui/src/app/startup.rs:820`、`file-search/src/lib.rs:432-455`）；**遵守 `.gitignore`**，且只在 git 仓库内生效（`require_git(true)`）；隐藏文件**不被排除**（`.hidden(false)`），符号链接**被跟随**（`.follow_links(true)`）。**MCP resource 不在候选里**（提问里这一半的假设不成立）。
3. **过滤**：**模糊匹配、大小写不敏感**。文件/目录走 `nucleo`，参数是 `CaseMatching::Ignore` + `Normalization::Smart` + `Config::DEFAULT.match_paths()`（`file-search/src/lib.rs:510`、`:529-535`、`:346-354`）；工具类候选走自带的 `codex_utils_fuzzy_match::fuzzy_match`，它把两侧都 `to_lowercase()` 之后做子序列匹配，并对前缀命中减 100 分加权（`codex-rs/utils/fuzzy-match/src/lib.rs:12-52`）。
4. **交互**：**浮层，渲染在输入区上方**，保留 composer 的 footer（`chat_composer.rs:44`），渲染前先 `Clear`（`mentions_v2/render.rs:46`）——**不是内联在输入行里**，提问里「内联」的假设不成立。键位：`↑`/`Ctrl-P` 上移、`↓`/`Ctrl-N` 下移、`←`/`→` 切过滤 tab、`Esc` 关闭并记住「这个 token 已被否掉」、`Tab` 插入并关弹窗、`Enter` 插入并关弹窗（**没有选中项时**才退化成普通提交）（`chat_composer.rs:2247-2367`；footer 文案在 `mentions_v2/footer.rs:15-20`）。**选中一个候选只是填进文本，绝不立刻发送**。
5. **插入形态**：文件与目录插入的是**纯文本路径**，**不内联文件内容**；路径是**相对搜索根**（即会话 CWD）的（`insert_selected_path`，`chat_composer.rs:2741-2760`；`FileMatch.path` 的文档注释见 `file-search/src/lib.rs:49-50`）。**含空白的路径会被裹上双引号**，但路径自身已含 `"` 时不加引号（`:2745-2750`）。**唯一的例外是图片**：`.png/.jpg/.jpeg/.gif/.webp` 选中后不插路径，而是读尺寸后变成附件（`:2369-2376`、`:2458-2483`）。
6. **目录**：候选里有目录（`MatchType::Directory`）。选中目录 = **插入目录路径**，与文件走同一条代码路径；**没有「展开下一层」这个行为**。
7. **别的触发符**：有 `$`（跨 crate 的 tool mention sigil，`utils/plugins/src/mention_syntax.rs:4`），列 skill 与 app；**没有 `#`**（`chat_composer.rs` 里没有把 `'#'` 当 sigil 的分支）。`@` 在 codex 内部的名字是 `PLUGIN_TEXT_MENTION_SIGIL`（同文件 `:7`），TUI 让它在文件场景下兼作文件补全符。
8. **边界**：搜索在**独立 crate + 两个后台线程**里做，nucleo 增量匹配、默认 `limit=20`、`threads=2`、10ms tick，取消标志每 1024 个条目检查一次（`file-search/src/lib.rs:126-137`、`:216-219`、`:509`、`:460-497`）；TUI 只留前 **8** 行（`popup_consts.rs:14`）。**walker 不按大小或二进制过滤**，唯一的类型特判是图片。光标处只有一个活动 token；`Esc` 的「否掉」按 **token 文本 + 出现序号**记账，所以同一行里相同文本的第二个 `@` 不会被前一个的 `Esc` 连坐（`chat_composer/popup_state.rs:12-45`）。**删掉 `@`** 之后无 token，弹窗在 `sync_popups` 的收尾分支被关掉，同时发一条空查询把搜索 session 拆掉（`chat_composer.rs:3906-3917`；`tui/src/file_search.rs:62-65`）。
9. **分层**：状态机在 **TUI 的 `bottom_pane` 这一层**——`ChatComposer` 持有 `ActivePopup::MentionV2`，token 解析在同目录的 `completion_target.rs`，弹窗模型在 `mentions_v2/`，原子元素由 `bottom_pane/textarea.rs` 提供；文件搜索被切到**独立 crate `codex-rs/file-search`**，TUI 侧只留一个 `FileSearchManager` 经由 `AppEvent::StartFileSearch` / `AppEvent::FileSearchResult` 与 app 层通信（`tui/src/file_search.rs:1-6`、`:120-123`）。**弹窗键位是硬编码的**，没有走 `tui.keymap`——`tui.keymap` 里确实有一个 `list` context（`config/src/tui_keymap.rs:454-479`），但它服务的是 analytics、agents overview 那类列表视图，不是这个弹窗（见 ⑩）。

---

## ② 触发：`@` 在什么条件下打开补全

### 判定只有一处

补全的「当前 token」由 `current_prefixed_token_range(textarea, prefix, allow_empty)` 唯一决定（`codex-rs/tui/src/bottom_pane/chat_composer/completion_target.rs:72-267`）。`@` 与 `$` 共用它，只有前缀字符不同。`ChatComposer` 的 `current_at_token` 就是它固定 `'@'`（`chat_composer.rs:2624-2629`）。

### 规则（按代码顺序）

- **token 由「任意空白」界定**：向左取时，起点是 `text[..end_left]` 里**最后一个 `char::is_whitespace()` 之后**——没有空白就是 `0`（缓冲区起点）（`completion_target.rs:140-144`）。这里的 `is_whitespace()` **包含 `\n`**，所以**行首的 `@` 会被识别**。
- **分隔位置另有一套「水平空白」定义**：`is_horizontal_whitespace` 显式排除 `\n`、`\r`、`\u{000B}`、`\u{000C}`、`\u{0085}`、`\u{2028}`、`\u{2029}`（`:109-115`）。它只用于判断「光标是不是停在分隔符上」（`at_separator`，`:126`），不用于切 token 边界。
- **光标紧跟水平空白、右边是非空白**时叫 `cursor_starts_token`，此时取**右侧** token（`:118-122`、`:179-198`）。
- **光标后面正好跟着 `@`（或 `$`）**时，有一条显式准入：只有「前一个字符是空白或不存在」才向右取，否则只看向左（`:255-265`）。这就是 **`src/@` 不触发**的直接原因。
- **向左的候选必须以 `@` 开头**：`prefixed_candidate_range` 里 `token.starts_with(prefix)` 是 `filter` 条件（`:23-26`）——所以哪怕光标在 `src/@` 的末尾，左 token `src/@` 不以 `@` 开头，取不到。
- **原子元素是硬边界**：token 遇到 text element 会被切开（`:32-45`）。
- **换行不提供跨行亲和**：`@file\n  continue` 把光标放在行尾缩进里返回 `None`（`completion_target_tests.rs:6-22`）——注意这条约束的是「光标停在缩进里时不要乱认」，**不是**「行首不触发」。

### 什么时候整个补全被禁用

`sync_popups` 开头有几条前置条件，任一命中都会把弹窗清掉（`chat_composer.rs:3813-3829`）：

- 正在做 Ctrl-R 历史搜索，或正在 Vim 搜索（`:3815-3825`）；
- `popups_enabled()` 为假——它要求草稿非空且 composer 可用（`:1108` 起）；
- 输入区有鼠标选区（`:3826`）。

浏览输入历史（↑/↓ 召回旧 prompt）时也跳过全部弹窗同步（`:3836-3849`）。

### `@` 与 `$` 抢同一个光标位

同一位置若两种 token 都成立，**取起点更靠左的那个**：如果 `@` token 的起点比 `$` token 的起点更靠右，就把 `$` 目标丢掉，反之丢掉 `@`（`chat_composer.rs:3850-3865`）。这条是为了让 `$skill` 和 `@file` 相邻时不互相盖掉。

---

## ③ 候选来源：工作区文件系统 + 四类工具

### 五类候选，一张表

| 类别 | 显示标签 | 插入文本 | 绑定的 path | 来源 | 出处 |
| --- | --- | --- | --- | --- | --- |
| 文件 | `File` | 路径纯文本 | 无（不进 binding） | `codex-file-search` | `mentions_v2/candidate.rs:43-51`、`chat_composer.rs:2349-2354` |
| 目录 | `Dir` | 路径纯文本 | 无 | 同上（`MatchType::Directory`） | `file-search/src/lib.rs:68-73`、`mentions_v2/filter.rs:95-110` |
| 插件 | `Plugin` | `@<mention_name>` | `plugin://<config_name>` | `PluginCapabilitySummary` | `search_catalog.rs:88-112` |
| 技能 | `Skill` | `$<skill_name>` | skill 的文件路径 | `SkillMetadata` | `search_catalog.rs:67-86` |
| 任务 | `Task` | `@<title>` | `thread://<thread_id>` | 本会话的 task 列表 | `search_catalog.rs:45-62` |

技能有**去重规则**：若某个 skill 的 `plugin_id` 出现在已有 plugin 列表里，这个 skill **不单独列**（`search_catalog.rs:26-39`）——因为插件已经代表它了。弹窗副标题也明说这一点（`chat_composer.rs:26-27`）。

`MentionType` 的完整枚举就是这五个（`candidate.rs:18-25`）；**没有 MCP resource、没有 app 的 `@` 入口**（app/connector 走 `$`，见 ⑧）。

### 文件系统的遍历

TUI 建 session 时只给一个根：`vec![self.search_dir.clone()]`，而这个 `search_dir` 就是 `config.cwd`（`tui/src/file_search.rs:83-88`；构造点 `tui/src/app/startup.rs:820` 与 `tui/src/app/reconnect.rs:328`）。选项是 `FileSearchOptions { compute_indices: true, ..Default::default() }`（`tui/src/file_search.rs:85-88`），于是另外三个值取默认：`limit = 20`、`threads = 2`、`respect_gitignore = true`（`file-search/src/lib.rs:126-137`）。

walker 的配置逐条是（`file-search/src/lib.rs:432-455`）：

- `.hidden(false)` —— **允许隐藏条目**（注释原文 “Allow hidden entries.”）；
- `.follow_links(true)` —— **跟随符号链接**去看内容；
- `.require_git(true)` —— 只在存在 git 上下文时应用 gitignore 规则；
- `respect_gitignore` 为假时才会把 `.gitignore`、git global/exclude、`.ignore`、父目录扫描一次性全关掉（`:445-452`）。

`require_git(true)` 的理由写在函数文档注释里：git 自己从不读仓库根之上的 `.gitignore`，而 `ignore` crate 默认会读**所有祖先目录**——那是为「非 git 场景」有意做的偏离，会让一条宽泛的父级 ignore（例如 `~/.gitignore` 里写 `*`）**静默吞掉整次遍历**（`:410-421`）。测试侧有两个对应的用例：`parent_gitignore_outside_repo_does_not_hide_repo_files`、`git_repo_still_respects_local_gitignore_when_enabled`（`:1054`、`:1114`）。

**进入候选的是文件与目录两种**：遍历时按 `entry.file_type().is_dir()` 打标签，其余全记成 `File`（`:477-480`）。路径会先剥掉搜索根、转成相对路径再喂给匹配器（`:391-408`）。**没有 MCP resource、没有符号表、没有 skill 之外的任何 workspace 概念**。

### 排序

工具的排序是硬编码的等级 + 组内规则：`Plugin(0) < Skill(1) < Task(2) < File|Dir(3)`（`filter.rs:62-79`）；同组内文件/目录按匹配分降序，其余按「命中 indices 有没有」再按分数、再按名字（`:81-93`）。Task 之间不排序，保持原序（`:71-73`）。

---

## ④ 过滤：模糊匹配、大小写不敏感

**文件与目录**走 `nucleo`。匹配器线程收到查询后调用：

```rust
nucleo.pattern.reparse(
    0, &query,
    CaseMatching::Ignore,
    Normalization::Smart,
    append,          // append = query.starts_with(&last_query)
);
```

（`file-search/src/lib.rs:527-539`。）`Config::DEFAULT.match_paths()` 在同一函数里取出（`:510`），`create_pattern` 的测试辅助函数把等价的参数写成 `CaseMatching::Ignore` + `Normalization::Smart` + `AtomKind::Fuzzy`（`:346-354`）。三个结论：**模糊（子序列）匹配**、**大小写不敏感**、**按路径匹配**。`append` 让「继续打字」走增量重解析而不是从头匹配。

**工具类候选**（plugin/skill/task）走另一套：

- 先拿显示名做 `fuzzy_match`，命中就带上高亮 indices（`filter.rs:48-52`）；
- 显示名不中，再拿 `search_terms` 里其余的别名逐个模糊匹配，只取分数不取高亮（`:53-59`）——所以插件可以用 plugin 名、`config_name`、marketplace 名来搜（`search_catalog.rs:94-100`）；
- 查询为空时（裸 `@`）**所有工具候选全部入选、分数 0**（`filter.rs:25-28`）。

`fuzzy_match` 本身是「两边都 `to_lowercase()` 之后做子序列匹配」（`codex-rs/utils/fuzzy-match/src/lib.rs:17-46`），完全没有大小写敏感的分支。

---

## ⑤ 交互：浮层、键位、选中之后

### 是浮层，且在输入区上方

模块文档写死了布局：「All completion suggestions **render above the composer** and preserve its footer.」（`chat_composer.rs:44`）在自有 transcript 的框架下，建议还会以「上下各留一行空白」的方式**覆盖**内容，而不占布局空间（`:45-47`）。绘制实现先对整个区域做 `ratatui::widgets::Clear`（`mentions_v2/render.rs:46`）。

**提问里「内联在输入区里还是浮层」的两种假设，命中的是后者，但要注意它是「贴在上方的一块覆盖层」而不是独立弹窗框**：弹窗自带标题行 `Mentions` 与一行副标题（`render.rs:76-92`），下面依次是过滤 tab、查询行、结果列表、上下溢出指示与 footer（`:34-35` 的 `POPUP_HEIGHT = MAX_POPUP_ROWS + 9`）。

### 过滤 tab

三个模式循环切换：`All Results` / `Filesystem Only` / `Plugins`（`search_mode.rs:37-43`），各自接受哪几类候选写在 `SearchMode::accepts`（`:27-35`）。注意 `Filesystem Only` **只留文件与目录**，`Plugins` **只留 Plugin 与 Skill**——Task 只在 `All Results` 里出现。切换方向由 `previous()` / `next()` 定义（`:11-25`）。

### 键位表（默认路径，`mentions_v2` 开启）

| 键 | 行为 | 出处 |
| --- | --- | --- |
| `↑` 或 `Ctrl-P` | 选中项上移（循环） | `chat_composer.rs:2266-2276` |
| `↓` 或 `Ctrl-N` | 选中项下移（循环） | `:2277-2288` |
| `←`（无修饰） | 切到上一个过滤 tab；**没有活动 token 时**退回普通左移 | `:2289-2300` |
| `→`（无修饰） | 切到下一个过滤 tab；同上 | `:2301-2312` |
| `Esc` | 记下「这个 token 已被否掉」并关弹窗 | `:2313-2322` |
| `Tab` | 取选中项插入、关弹窗 | `:2323-2329` |
| `Enter`（无修饰） | 取选中项插入、关弹窗；**没有选中项时**退回普通提交路径 | `:2330-2339`、`:2361-2363` |
| 其余按键 | 交给 `handle_input_basic`，即照常编辑文本 | `:2340` |

footer 给用户的提示是 `Enter/Tab insert · Esc close · Up/Down select · Left/Right filter`（`mentions_v2/footer.rs:15-20`），与代码一致。

**提问里 `Ctrl-N`/`Ctrl-P` 的猜测成立**，`Tab`/`Enter`/`Esc`/`↑`/`↓` 也都成立；`←`/`→` 是提问里没提到的额外键位。

### 选中之后发生什么

`close_popup` 为真时，先重新解析一次当前 token 的范围，再按选中项的类型分派（`chat_composer.rs:2343-2359`）：

- `MentionV2Selection::File(path)` → `insert_selected_file_path`（插路径，或图片变附件）；
- `MentionV2Selection::Tool { insert_text, path }` → `insert_selected_mention`（插原子元素 + 记账 binding）。

两条路径都**只改文本**。**只有 `Enter` 且当时没有选中项**，才把这次按键转交 `handle_key_event_without_popup`（`:2337`、`:2361-2363`）——也就是说**「有候选时按 Enter」绝不会发出消息**。

### 旧路径（`mentions_v2` 关闭）

键位几乎一样（`handle_key_event_with_file_popup`，`chat_composer.rs:2082-2165`）：`↑`/`Ctrl-P`、`↓`/`Ctrl-N`、`Esc`、`Tab` 或 `Enter` 插入。差别是：**没有左右切 tab**，且 `Enter` 在无选中项时同样退回普通提交（`:2145-2152`）。插入走的是同一条 `insert_selected_file_path`。

---

## ⑥ 插入形态：纯文本路径，不是文件内容

### 文件与目录

`insert_selected_path` 的全部逻辑（`chat_composer.rs:2741-2760`）：

```rust
let needs_quotes = path.chars().any(char::is_whitespace);
let inserted = if needs_quotes && !path.contains('"') {
    format!("\"{path}\"")
} else {
    path.to_string()
};
```

代码注释写明加引号的理由：**「so the local prompt arg parser treats it as a single argument」**（`:2742-2744`）。注意两个细节：

- 判据是 `char::is_whitespace()`，所以**含换行的路径也会被引号裹**（虽然路径里一般没有）；
- 路径**自身已含 `"` 时不加引号**——注释说这是为了「保持行为简单」（`:2744`）。也就是说含双引号的路径今天**没有转义方案**。

插入后光标会走过一个水平分隔符：已有分隔符且其后紧跟非空白，就再补一个空格；没有分隔符就插一个空格（`advance_past_completion_separator`，`:2383-2415`）。**行尾换行不会被当成可复用的分隔符**（`:2389-2400`、`:2412-2414`）。

**路径是相对搜索根（会话 CWD）的**，不是绝对路径：`FileMatch.path` 的文档注释写着 “Path to the matched entry (file or directory), **relative to the search directory**”（`file-search/src/lib.rs:49-50`），而 TUI 直接把 `path.to_string_lossy()` 交给插入函数（`chat_composer.rs:2350-2353`）。

### 图片是唯一例外

`insert_selected_file_path` 先按扩展名判断是不是图片——`.png`、`.jpg`、`.jpeg`、`.gif`、`.webp` 且大小写不敏感（`chat_composer.rs:2369-2376`）。是图片就读尺寸，成功就**删掉刚敲的 token，改为 `attach_image`**；读尺寸失败则退回插路径（`:2458-2483`）。所以「`@` 只插文本」这句话**对图片不成立**。

### 工具类候选是原子元素 + binding

`insert_selected_mention` 走的是另一条路：先在 `TextArea` 里 `insert_element(insert_text)` 造一个**原子元素**，再往 `mention_bindings` 里记一条 `{sigil, mention, path}`（`chat_composer.rs:2762-2811`）。提交时这些 binding 会被翻译成结构化输入：

- `plugin://...` 的 binding → `UserInput::Mention { name: 显示名, path }`（`chatwidget/input_submission.rs:333-355`）；
- `app://...` 的 binding → 同样是 `UserInput::Mention`（`:357-394`）；
- skill 的 binding → `UserInput::Skill { name, path }`（`:319-330`）。

历史/回放的编码另有形状：`@name` 会被写成 `[@name](path)` 的链接（`tui/src/mention_codec.rs:102-107`），`$` 同理（`:78-101`）。**注意这条编码只接受 `$` 与 `@` 且要求 binding 存在**（`:37-49`）——文件路径没有 binding，所以**不会**被编码成链接。

### 结论

**文件与目录是纯文本引用，内容不被内联。**模型看到的就是 prompt 里那一串（可能带引号的）相对路径，要不要读文件由模型自己用工具决定。

---

## ⑦ 目录

- 目录**确实进候选**：`MatchType` 只有 `File` 与 `Directory` 两个值（`file-search/src/lib.rs:68-73`），遍历时用 `entry.file_type().is_dir()` 判（`:477-480`），结果显示成 `Dir` 标签（`candidate.rs:43-51`）。
- **选中目录的结果是「插入目录路径」**，与文件完全同一条代码路径（`MentionV2Selection::File(path)` → `insert_selected_file_path` → `insert_selected_path`，`chat_composer.rs:2349-2354`、`:2741-2760`）。
- **没有「展开下一层」这个行为**：整个 `mentions_v2` 目录里没有按选中项重新定根的代码；查询始终打到同一个 `search_dir`（`tui/src/file_search.rs:83-88`），唯一会换根的是会话 CWD 变化（resume）时的 `update_search_dir`（`:44-50`）。
- 路径**不带尾随分隔符**：相对路径由 `strip_prefix(root)` 得到（`file-search/src/lib.rs:391-408`），没有补 `/` 的步骤。

**提问里「展开下一层还是插入目录路径」的二选一，命中的是后者。**

---

## ⑧ 别的触发符：`$` 有，`#` 没有

### 两个 sigil 的名字与归属

```rust
/// Default plaintext sigil for tools.
pub const TOOL_MENTION_SIGIL: char = '$';

/// Plugins use `@` in linked plaintext outside TUI.
pub const PLUGIN_TEXT_MENTION_SIGIL: char = '@';
```

（`codex-rs/utils/plugins/src/mention_syntax.rs:1-7`。）这两个常量经 `codex-rs/plugin` 与 `codex-rs/core/src/mention_syntax.rs` 转出去给全仓用（`plugin/src/lib.rs:5`、`core/src/mention_syntax.rs:1-2`）。

**所以 `@` 在 codex 的领域语言里首先不是「文件」，而是「插件的纯文本提及符」**；TUI 让 `@` 同时开文件补全，`$` 开技能/应用补全。模块文档把默认分工写得很清楚：

> By default, `@` lists plugins, filesystem entries, and skills. Skills are hidden when their owning plugin is listed. `$` lists individual skills and apps, but not plugins.
> Disabling `mentions_v2` restores file-only `@` search and adds plugins back to `$`.

（`chat_composer.rs:26-28`。）

### `#` 不存在

我在 `chat_composer.rs` 里搜 `'#'` **没有任何 sigil 分支**。TUI 里的其它前缀是 `/`（slash command，`sync_command_popup`，`:3923`）与 `!`（shell 模式，`:3868`）。**提问里「有没有 `#` 之类」的答案是没有。**

### `$` 的候选与 shell 语法的仲裁

`$` 的候选存在两套：`mentions_v2` 关闭时是「legacy skill popup」（`ActivePopup::Skill`，`sync_mention_popup`，`:4030-4062`），开启时 `$` 也走同一个 `MentionsV2` 弹窗但与 `@` 不同时激活（`:3885-3898`）。候选来自 `mention_items()`（`:4111` 起）。

`$` 有一条 `@` 没有的负担：**必须把 shell 变量语法让开**。`dollar_query_kind` 把查询分成五类——`Completable`、`ShellVariable`、`DefiniteShellParameter`、`AmbiguousShellParameter`、`Invalid`（`completion_target.rs:311-320`）；`$1`、`$_`、`$HOME` 这类不走补全，`$12factor` 这种「数字开头但含非数字」的算歧义，要看有没有同名 skill 才决定（`:322-354`）。对应的测试就在 `chat_composer.rs:7147` 一带（`file_popup_ignores_shell_positional_parameter_snapshot` 等）。

**这条对 `@` 的启示是负面的**：`@` 今天不需要处理这种歧义，因为 shell 里没有 `@` 的特殊语义。

---

## ⑨ 边界处理

### 性能：独立 crate、两个后台线程、增量匹配

- **两个线程**：matcher 线程 + walker 线程，在 `create_session` 里各起一个（`file-search/src/lib.rs:216-219`）。
- **nucleo**：`Nucleo::new(Config::DEFAULT.match_paths(), notify, Some(threads.get()), 1)`（`:193-198`），`threads` 默认 2、`limit` 默认 20（`:126-137`）。
- **增量**：`append = query.starts_with(&last_query)` 决定 `reparse` 是否走增量（`:528-536`）。
- **节流**：matcher 循环里 `TICK_TIMEOUT_MS = 10`，`nucleo.tick(10)` 之后只在 `status.changed` 时才向 reporter 报快照（`:509`、`:558-575`）。
- **取消**：walker 每处理 1024 个条目检查一次 `cancelled` / `shutdown`，命中就 `WalkState::Quit`（`:460-497`）。
- **UI 侧只留 8 行**：`MAX_POPUP_ROWS = 8`（`bottom_pane/popup_consts.rs:14`），两处 `set_matches` 都 `.take(MAX_POPUP_ROWS)`（`mentions_v2/popup.rs:157`；旧路径 `file_search_popup.rs:78`）。
- **查询变空就拆 session**：`on_user_query("")` 直接 `st.session.take()`，把后台搜索停掉（`tui/src/file_search.rs:62-65`）。

### 二进制 / 大文件

**没有按大小或二进制内容的过滤。**walker 的判定只有 `is_dir`（`file-search/src/lib.rs:477-480`），没有扩展名白名单、没有 `metadata().len()` 检查。唯一与「文件类型」有关的行为是**选中之后的图片特判**（见 ⑥）——而且它是在选中时读尺寸，不是在候选阶段筛掉。

### 被忽略的文件

`respect_gitignore = true`（默认）时，`.gitignore` 命中的条目**不进候选**。关掉它的唯一途径是把 `respect_gitignore` 设成 `false`，那会连带关掉 `.ignore`、git global/exclude 与父目录扫描（`file-search/src/lib.rs:445-452`）。TUI 今天用的是默认值，所以**默认遵守**（`tui/src/file_search.rs:85-88`）。

### 一行里多次 `@`

光标处只有**一个**活动 token（`current_prefixed_token_range` 返回单个 `(Range, String)`）。`Esc` 的「否掉」记账不是记位置，而是记 **token 文本 + 在草稿里的出现序号**：`DismissedToken { query, token, occurrence }`，`matches()` 要求查询相同、range 处的文本相同、且「出现在它之前的相同 token 个数」也相同（`chat_composer/popup_state.rs:12-45`）。好处是**偏移量变化不会误开**，同时**后面一个相同的 token 不会被前一个的 `Esc` 连坐**（文档注释 `:34-37`）。计数时「空白与原子元素边界算分隔，嵌套 sigil 不算」（`:47-51`）。

同一条思路也用在插入之后：`dismiss_completed_prefixed_token` 只在 range 与文本都对得上时才记账，防止「插入的那个 token」的关闭状态被后文相同文本继承（`chat_composer.rs:2417-2446`）。

### 删掉 `@` 之后

没有活动 token 时，`sync_popups` 走到收尾分支：若有在跑的查询就发 `StartFileSearch(String::new())` 把它停掉，然后仅在当前弹窗是 `File` / `Skill` / `MentionV2` 时把它置为 `None`（`chat_composer.rs:3906-3917`）。`FileSearchManager` 收到空查询后清空并 drop session（`tui/src/file_search.rs:62-65`）。**下拉是关掉的**，不是留着上一次的结果。

### 鼠标与粘贴

- 输入区有鼠标选区时全部弹窗不同步（`chat_composer.rs:3826-3829`）。
- 非 ASCII / IME 输入走单独通道，不触发 paste-burst 的「回吞」（`:1996-2015`）。
- 粘贴是原子元素，token 解析遇到元素会切开（`completion_target.rs:32-45`）；若补全的 token 紧贴一个原子元素，插入时会先补一个空格并平移 range，避免文本粘在一起（`separate_completion_from_adjacent_element`，`chat_composer.rs:2813-2833`）。

---

## ⑩ 分层：状态机在哪一层、键位是否可配

### 三层职责

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 输入编辑器（bottom pane） | `codex-rs/tui/src/bottom_pane/chat_composer.rs`（12647 行）+ 同目录 `chat_composer/` 子模块 | 光标邻域解析、弹窗状态、键位分派、插入、binding 记账 |
| 弹窗与渲染 | `codex-rs/tui/src/bottom_pane/mentions_v2/`（candidate / filter / footer / popup / render / search_catalog / search_mode，约 1300 行） | 候选模型、过滤、排序、绘制 |
| 文件搜索 | **独立 crate** `codex-rs/file-search`（1196 行 + CLI + snapshot） | walker、nucleo 匹配、增量更新、取消 |

`TextArea` 提供原子元素与光标（`bottom_pane/textarea.rs`）；TUI 侧的胶水是 `FileSearchManager`，它与 app 层之间只走两个事件：`AppEvent::StartFileSearch(query)` 与 `AppEvent::FileSearchResult { query, matches }`（`tui/src/file_search.rs:1-6`、`:120-123`）。**搜索 crate 不依赖 TUI**，它自己也能当 CLI 跑（`file-search/src/main.rs`、`cli.rs`）。

### 键位是硬编码的

`handle_key_event` 的分派链是：vim 特殊键 → 历史搜索 → …… → `handle_key_event_inner`（`chat_composer.rs:1910-1960` 一带）；**没有任何 keymap 查表**。到了弹窗层，`handle_key_event_with_mentions_v2_popup` 直接用 `match key_event` 认 `KeyCode::Up` / `KeyCode::Char('p') + CONTROL` 等字面量（`:2265-2341`）。旧路径 `handle_key_event_with_file_popup` 同样（`:2099-2163`）。

`tui.keymap` 里**确实**有一个 `list` context，含 `move_up` / `move_down` / `move_left` / `move_right` / `page_up` / `page_down` / `jump_top` / `jump_bottom` / `accept` / `cancel`（`codex-rs/config/src/tui_keymap.rs:454-479`，文档注释 “List selection context keybindings for popup-style selectable lists.”）。但我追了它的使用点，落在 `analytics`、`agents_overview` 这类视图上（例如 `analytics` 的测试直接 `view.keymap = keymap.list`），**没有一处把它接到 mention 弹窗**。

**所以：这个弹窗的键位今天不可配，`/keymap` 改不动它。**

### feature flag

`mentions_v2` 是 `Stage::Stable` + `default_enabled: true`（`codex-rs/features/src/lib.rs:1690-1694`），列在 `Feature::MentionsV2`（`:343`）。关掉它会回退到「文件专用 `@` + `$` 里的插件」的旧分工（`mentions_v2/mod.rs:3-4`、`chat_composer.rs:28`），运行时由 `self.config.features.enabled(Feature::MentionsV2)` 喂给 `set_mentions_v2_enabled`（`tui/src/app/event_dispatch.rs:1684` 一带、`bottom_pane/mod.rs:513`）。

**给后续访谈用的一句话**：`@` 的文件补全不是「文件搜索功能」，而是**统一 mention 弹窗里的一个 tab**（默认 tab 是 `All Results`，文件只是其中一类）；关掉 flag 才是纯文件搜索。

---

## ⑪ 几个反直觉、容易踩的点

1. **光标停在空白上、右边紧跟着 `@`，会立刻认出右边那个 token**（`cursor_starts_token` 分支，`completion_target.rs:118-122`、`:179-198`）。所以「敲完空格再敲 `@`」和「敲完 `@` 再往回退一格」都可能触发，但**前者才是正常路径**。
2. **`@` 后面必须是空 query 才算「裸 `@`」**：`mentions_v2` 用 `allow_empty = true`（`:2665`），所以裸 `@` 是有弹窗的——但那时**只列工具候选（plugin/skill/task），文件一个都不列**，因为空查询会先发一条空搜索把 file search 停掉（`chat_composer.rs:4074-4077`；`mentions_v2/popup.rs:139-149`）。文件候选要等打了第一个字符才来。
3. **同一行里两个 `@` 是两件独立的事**：`Esc` 的否掉按「文本 + 序号」记账，不是按位置（`popup_state.rs:12-45`）。
4. **换行是 token 分隔符但不是「可复用的分隔符」**：向左切 token 时 `\n` 算空白（`completion_target.rs:140-144`），插入后光标前进时分隔符判定又显式排除 `\n`、`\r` 等，宁可多插一个空格也不跨行（`chat_composer.rs:2389-2400`、`:2412-2414`）。
5. **含双引号的路径没有转义，只是不加引号**（`chat_composer.rs:2742-2750`）。`a "b".txt` 这种路径插入后会是一个裸的、带空格的 token。
6. **图片走另一条路**：选中 `.png` 之类不插路径而是变附件（`:2369-2376`、`:2458-2483`）。所以「`@` 插的是纯文本」这条规则**有例外**，而例外是按扩展名判的。
7. **`@` 与 `$` 会在同一光标位竞争**，胜者是起点更靠左的那个（`:3850-3865`）。
8. **搜索根是会话 CWD，不是「项目根」**：`config.cwd`（`tui/src/app/startup.rs:820`）。resume 到别的工作区时会 `update_search_dir` 换根并丢掉当前 session（`tui/src/file_search.rs:44-50`）。
9. **技能会被插件吃掉**：`plugin_id` 已在插件列表里的 skill 不单独显示（`search_catalog.rs:26-39`）。

---

## 横向对比表：codex 自己的两条路径

| 维度 | `mentions_v2` 开启（默认） | `mentions_v2` 关闭（回退路径） |
| --- | --- | --- |
| `@` 能列什么 | Plugin、文件、目录、Skill、Task（`render.rs:78`） | 只有文件与目录（`chat_composer.rs:28`） |
| `$` 能列什么 | Skill、app；不含 plugin（`:26-27`） | Skill/app，且 plugin 被移回 `$`（`:28`） |
| 过滤 tab | 有，三档 `All Results` / `Filesystem Only` / `Plugins`，`←`/`→` 切（`search_mode.rs:37-43`、`chat_composer.rs:2289-2312`） | 无 |
| 弹窗实现 | `bottom_pane/mentions_v2/` | `bottom_pane/file_search_popup.rs` |
| 文件搜索代码 | **同一条**：`AppEvent::StartFileSearch` → `FileSearchManager` → `codex-file-search` | 同左 |
| 插入函数 | `insert_selected_file_path`（`:2458`） | `insert_selected_file_path`（`:2158`） |
| `Esc` 否掉 | `dismissed_mention_token`（`:2317-2318`） | `dismissed_file_token`（`:2131-2132`） |
| 裸 `@` | `allow_empty = true`，有弹窗但只有工具候选（`:2665`、`:4074-4077`） | `allow_empty = false`，空 token 不算（`:3834`） |

**两条路径共享的部分比差异大**：搜索 crate、`insert_selected_path`、引号规则、光标前进规则、`Esc` 的记账结构都是同一份。

---

## 对 fs-agent 的启示

**这一节只写事实层面的对照，不替本仓库做决定。**

| codex 今天有这个 | fs-agent 今天有这个吗 |
| --- | --- |
| `@` 触发点由「光标邻域解析」一个函数决定，`@` 与 `$` 共用（`completion_target.rs:72-267`） | 没有。`src/render/input.rs` 是输入区，全仓搜 `mention` / `file_search` / `completion` / `popup` **没有任何候选弹窗或补全状态机** |
| 独立的文件搜索 crate `codex-rs/file-search`，walker + nucleo + 双线程 + 增量（`file-search/src/lib.rs`） | **没有这个 crate**。`Cargo.toml` 里有 `ignore = "0.4"`（`src/tools/grep.rs` 用来遍历）与 `globset`，但**没有 `nucleo`、没有任何模糊匹配库** |
| 候选来源是 `config.cwd` 一次遍历，遵守 `.gitignore`（`require_git(true)`）、允许隐藏文件、跟随符号链接（`file-search/src/lib.rs:432-455`） | `grep` 工具也用 `ignore::WalkBuilder`，但那是工具侧、按模型给的 glob 搜内容；**输入区没有任何遍历** |
| 五类候选：文件 / 目录 / plugin / skill / task（`candidate.rs:18-25`） | fs-agent 有 skill 与 MCP 的概念（`.scratch/mcp-support/`、skills），**但没有任何「提及」语法把它们插进 prompt** |
| 插入的是**相对路径纯文本**，含空白加双引号，图片变附件（`chat_composer.rs:2741-2760`、`:2369-2376`） | 无对应物 |
| 浮层渲染在输入区**上方**，`Clear` 后自绘（`render.rs:46`；`chat_composer.rs:44`） | fs-agent 的输入区在 `src/render/input.rs`，**没有覆盖层机制**给输入区用（详情覆盖层走的是另一套） |
| 键位硬编码在 popup handler；`tui.keymap` 的 `list` context 服务其它列表视图（`chat_composer.rs:2265-2341`；`tui_keymap.rs:454-479`） | fs-agent 有问卷区的 `j`/`k`/`Ctrl-N`/`Ctrl-P` 分派（`.scratch/questionnaire-keys/`，[ADR 0010](../../../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)），**与本功能无共用代码** |
| 搜索根随会话 CWD 变，resume 时换根（`tui/src/file_search.rs:44-50`） | fs-agent 的工作目录是会话属性（`continue-by-id` 的 spec 里处理过「命中别的工作区时工作目录跟着那场会话走」） |

另外两条纯事实：

- **codex 的 `@` 候选里没有 MCP resource**，尽管 codex 有 `codex-mcp` crate（`codex-rs/codex-mcp/`）与 MCP 支持。**「MCP resource 能不能被 `@`」在 codex 今天的答案是「不在候选里」**。
- **codex 的 `@` 插入不内联文件内容**，也不在提交时把路径替换成文件正文。我追过的提交路径（`chatwidget/input_submission.rs`）只把 `plugin://` / `app://` / skill 三类 binding 转成结构化 `UserInput`，**文件路径原样留在文本里**。

---

## 边界：这份文件回答不了什么

- **不是穷尽。** 我读了 `codex-rs/tui/src/bottom_pane/chat_composer.rs` 的关键函数与 `mentions_v2/` 全部文件、`codex-rs/file-search/src/lib.rs` 的主体、以及提交路径 `chatwidget/input_submission.rs` 的一节。**没读**：`chat_composer.rs` 剩下的一万行（Vim、粘贴、历史、多 agent 相关的分支）、`bottom_pane/textarea.rs` 的原子元素实现、`codex-rs/skills` 的 `extract_tool_mentions_with_sigil`、app-server 侧的 `AppEvent` 投递链、`/mention` slash command 的完整实现。
- **没有实测。** 本文件全部来自源码与官方文档阅读；**没有在本机跑过 codex**，也没有验证过任何一条候选排序、引号规则、按键行为在真终端里的观感。
- **`!` shell 模式与 `@` 的交互我没有核实。** 我看到 `sync_command_popup` 会因 `is_bash_mode` 被禁（`chat_composer.rs:3868`），但**没有找到** `is_bash_mode` 对 mention 弹窗的同类禁用；有相关测试（`chat_composer.rs:7147-7192` 一带）但那些测的是 `$` 与旧路径。**「在 `!` 命令里打 `@` 会怎样」我不给结论。**
- **官方文档对 `@` 只有一句话。** [Developer commands](https://learn.chatgpt.com/docs/developer-commands.md?surface=cli) 的 “Interactive shortcuts” 一节原文是「Type `@` to search for a file in the workspace and add its path to the prompt.」；[CLI customization](https://learn.chatgpt.com/docs/cli-customization.md) 通篇没提 `@`。**所以「codex 官方是怎么描述这个功能的」这个问题，答案就是这一句加 `/mention` 命令的那一段**，其余全部来自源码。
- **版本会漂。** codex 迭代很快（仓库里 `mentions_v2` 刚落到 `Stage::Stable`，npm `latest` 是 2026-10-01 的 `0.160.0`，而本文件钉的源码是 2026-10-04 的 `afb436df`——**两者不是同一个版本**）。引用本文件时请连同 commit 一起引用。
- **`mentions_v2` 的这个 flag 名字本身说明它正在收敛**（`features/src/lib.rs:1690-1694` 是 `Stable`，但 `mentions_v2/mod.rs:3-4` 说 flag「temporarily as a rollback path」）。**这条回退路径可能在下几个 commit 里消失**，届时 ⑧ 与横向对比表里「旧路径」的那一列会失效。
