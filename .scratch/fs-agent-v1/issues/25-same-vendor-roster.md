# 25: roster 允许同厂商 / 同模型（推翻票 24 的一条校验）

**What to build:** 票 24 把「两个讨论者必须来自不同厂商」做成了**解析期拒绝**。用户的 Kimi 订阅到期后，这条拒绝让讨论在一台只有一个 key 的机器上完全跑不起来——为了验证功能，roster 必须允许同厂商、甚至同一个模型。

Blocked by: 24

Status: done

**参考:** spec §15（讨论协议与轮次）、票 24、`docs/discussion.md`

- [x] **删掉两条拒绝**：同厂商、同一个模型都不再是配置错误（保留「恰好两个」与「模型必须已配置」）
- [x] **异质性降级成一条已解析的事实**：`DiscussionRoster::heterogeneous`（同厂商或同模型即 `false`），前台据此**明说**多样性变弱，而不是拦下来
- [x] **同模型的第二个讨论者要有自己的身份**：`DiscussionRoster::speakers()` 给出 `kimi-k3` / `kimi-k3#2`——`speaker_id` 是投影的函数，一个名字会让每一侧把对方的回答读成自己的
- [x] **库层补一条身份不重复的守卫**：`assemble_discussion` 拒绝两个讨论者共用一个 `speaker_id`（同模型允许，同身份不允许）
- [x] **文档同步**：README 配置示例、`docs/discussion.md`（intro + Running one）、`CONTEXT.md` 的「讨论者」词条、`discuss --help`、缺 roster 的提示文案
- [x] **测试**：同厂商 / 同模型都被接受且 `heterogeneous == false`（配置层）；`speakers()` 去重（配置层）；同身份 roster 被拒（库层）；两条提示文案（措辞层）

## Comments

**2026-09-22（实现）** 为什么是「允许 + 一行提示」而不是「允许但静默」：异构是这套设计最强的那根杠杆（spec §15），静默接受同模型会让人把「同一家的两个样本」当成「两个独立判断」。所以事实被解析进 `heterogeneous`，前台每次运行说一遍。要更安静的话，删掉 `discuss()` 里那一个 `if` 即可。

**代价（已知并接受）**：同模型讨论里，第二轮「定向」的意义被削弱（对方和自己的先验几乎一样），分歧只剩采样噪声；`DiscussionRoster::debaters` 仍记着两个模型 id，`sessions stats` 的逐发言者用量会把 `kimi-k3` 与 `kimi-k3#2` 分开列。

**没做的事**：没有加「给讨论者起名字」的配置（`[[discussion.debater]] name = "甲"`）。`#2` 是自动的、够用的；真要人给名字，那是另一张票（配置 schema 变化 + `debaters` 的两种写法）。

**2026-09-22（后续）** 票 27 推翻了两处：自动 `#2` 命名与 `DiscussionRoster::heterogeneous`。现在讨论者是人设（名字 + 模型）池，重名配置期报错、异质性按抽到的那一对判定（`Config::debaters_share_a_vendor`）。
