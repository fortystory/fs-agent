# 名字从 `fs-agent` 改成衡（`heng`）

程序改名**衡**，命令、crate 名与落盘路径一律 `heng`：`~/.config/heng/config.toml`、`$XDG_DATA_HOME/heng/{sessions,goals}`、技能发现根 `.heng`、环境变量 `HENG_MODEL`。**汉字是正身，`heng` 是它的拼音。**

`Forked Synthesis`（分叉合成）**保留**，但退作**讨论协议**这条机制的框架名，不再是产品名；缩写 `fs` 只留在历史文档里。`.scratch/` 下的历史票与 `docs/research/` 下的一手引文**不追改**——历史即当时的事实；旧 ADR 正文也不改，只在受影响处补注指向本文。

## 为什么改

1. **`fs` 的第一联想是 filesystem。** `fsck`、`node:fs`、`/proc/fs` 都在那一侧。旧名字的解释要读到 `README.md` 第 11 行才成立：`fs` 最初是 **fortystory** 的缩写（仓库所在的那层目录），后来被重新解释成 `Forked Synthesis`；为了让后一个解释站住，README 要写一句「`fs` 只是叙述里的框架名，**不是**命令行或路径的一部分」，`CONTEXT.md` 还要一条 `_Avoid_` 拦住 filesystem 的读法。一处缩写要两句自证。
2. **产品的定位是 harness，不是 agent。** 它编排两个讨论者、若干执行者与一次合成器调用，自己提供事件流、投影、权限、沙箱与 TUI；`CONTEXT.md` 早就规定 `agent` 只作泛称、不作类型名。名字里带 `-agent`，把整套装置说成了其中一个角色。
3. **拼音与项目的语言线同调。** [ADR 0004](0004-prose-in-chinese-identifiers-and-model-text-in-english.md) 与 [ADR 0005](0005-model-visible-text-in-chinese.md) 把散文与模型可见文本收进中文；名字用汉字、命令用拼音，是把同一条线延伸到标识符层。造词顺带让撞名归零：`prism` / `gantry` / `steer` / `muster` 这些候选在英文生态里都已被占（`zhi` 也已有一个配置管理 CLI 在用）。

## 为什么是「衡」

**衡**取「衡量」。合成器**不是裁判**——合成器词条把 judge / 裁判列进 `_Avoid_`，[docs/discussion.md](../discussion.md) 记着这是被证据否掉的方案：它只把两方判断放到秤上，共识 / 分歧 / 未决就是秤上的读数，决定权在人。同时，**衡**是一个名词（衡器、天平），所以这个名字指向一件**装置**——正好是第 2 条理由要的那个词性。

较近的候选里，`鉴`（以史为鉴，贴"复盘"）与 `梭` / `捻`（贴"两叉一茎"）都还在桌上；取 `衡` 是因为它同时接住了"装置"与"只称量、不裁决"这两件事。

## 后果

- **模型可见的身份提示词换了。** [`src/agent.rs`](../../src/agent.rs) 那条从「你是 fs-agent，一个自用的 coding agent CLI……」改成「你是衡（heng），一套自用的 coding agent harness……」。它是缓存前缀的头，所以新会话的前缀缓存换一次；**老会话 `--continue` 重放出来的仍是旧文本**——那正是事件流的正确行为（历史不重算）。[ADR 0001](0001-chinese-ui-frozen-model-text.md) 的冻结面记录的是"改它要记账"，不是"永远不许改"。
- **旧的配置与会话目录要搬一次。** 不做双名兼容：没有 `fs-agent` 别名、不读旧路径、不认 `FS_AGENT_*`。本机 `~/.config/fs-agent` 与 `~/.local/share/fs-agent` 用一条 `mv` 搬过去。
- **标记换了形。** `wording::logo_lines()` 从"拼出 fs-agent 的五行块字"改成"拼音 `héng` 在上、汉字「衡」在下"；宽度与行数的契约（38 列 × 5 行）不变。窄档的 `identity()` 读 `CARGO_PKG_NAME`，跟着包名自动变。
- **GitHub 仓库改名为 `heng`**，旧地址由 GitHub 重定向。
- **历史不追改**：`.scratch/` 的票与 spec、`docs/research/` 的引文、旧 ADR 正文里出现的 `fs-agent` 一律留原样；新写的文档只认新名。这不是漏改，是划界。
