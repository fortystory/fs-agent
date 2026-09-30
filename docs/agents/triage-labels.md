# 分诊标签

各个技能都用五个规范的分诊角色来说话。这份文件把这五个角色映射到本仓库 issue tracker
里实际使用的那几个标签串上。

| `mattpocock/skills` 的标签 | 我们 tracker 的标签 | 含义 |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | 需要维护者来评估这条 issue |
| `needs-info` | `needs-info` | 在等报告者补充更多的信息 |
| `ready-for-agent` | `ready-for-agent` | 已经完整写明，可以交给一个 AFK agent 去跑 |
| `ready-for-human` | `ready-for-human` | 必须由人来动手实现 |
| `wontfix` | `wontfix` | 不会去处理 |

某个技能提到某一个角色时（例如「打上 AFK-ready 那一档分诊标签」），就用这张表里与之对应
的那个标签串。

## 颜色

[`label-colors.json`](label-colors.json) 是远程 tracker 的那份「标签 → 颜色」映射，用的是
GitHub 的 `rrggbb` 写法。它还带着 wayfinder 技能据以归档自己的票的那五个 `wayfinder:*`
标签，于是这一整套词汇能一次就建齐：

```sh
gh label create "ready-for-agent" --color "$(jq -r '."ready-for-agent"' docs/agents/label-colors.json)"
```

运行时没有任何东西会读这个文件 —— 它只是「这些标签长什么样」的一份记录，
好让这里的改名与上面那张表里的改名，都发生在同一次改动里，而不是两边各自漂开。
