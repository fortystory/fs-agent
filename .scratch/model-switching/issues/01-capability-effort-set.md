# 01 — 能力表的档位集合

Type: implement
Status: done
Part of: ../spec.md
Blocked by: —

把 `ModelCaps::supports_reasoning_effort: bool` 换成一张**每个模型各有一份**的档位表，
`ReasoningEffort` 补上 `Medium` 与 `Xhigh`。这是后面每一张票的地基：选择器要拿它列候选。

规格见 [`spec.md` §1](../spec.md)。

## 要做什么

1. `ReasoningEffort`（`src/config.rs:308`）加两个变体：`Medium`、`Xhigh`。`as_str()` 跟着补，
   顺序按强度从低到高排（`Low` / `Medium` / `High` / `Xhigh` / `Max`）—— 选择器与状态行都按这个
   顺序画，所以枚举顺序是有意义的。
2. `ModelCaps` 的 `supports_reasoning_effort: bool` 换成
   `reasoning_efforts: &'static [ReasoningEffort]`。`#[non_exhaustive]` 留着，字段是 pub 的，
   crate 内那几个 `*_caps` 构造器都要改。
3. `caps_for` 的每一条分支填自己的表（下面「档位表」那张）。空表 = 没有这个旋钮
   （`MiniMax-M3`、`kimi-for-coding-highspeed`）。
4. `build_body`（`src/provider/openai.rs:332`）里那个 `if caps.supports_reasoning_effort` 改成
   查表：`caps.reasoning_efforts.contains(effort)` 为真才发 `reasoning_effort`；为假照今天那样
   告警并丢掉。**绝不发给模型一个它不认的值** —— 那是最容易变成 400 的地方。
5. 全部 `match` 与 `supports_reasoning_effort` 的读点改掉（`grep -rn supports_reasoning_effort`
   应该只剩这一条警告文案里的字样）。

## 档位表

| model id | 档位 | 出处 |
| --- | --- | --- |
| `kimi-k3`、`k3`、`k3-256k` | `low` / `high` / `max` | Kimi 开放平台：默认 `max` |
| `kimi-for-coding` | `low` / `high` / `max` | Kimi Code 模型表：K2.8 Preview，默认 `max` |
| `kimi-for-coding-highspeed` | 空 | Kimi Code 模型表：`Thinking:ON`，没有 effort |
| `deepseek-flash`、`deepseek-v4-pro` | `low` / `high` / `max` | DeepSeek thinking mode：默认 `high` |
| `MiniMax-M3.1-Flash-Preview` | `low` / `medium` / `high` / `xhigh` / `max` | MiniMax OpenAI SDK：默认 `max`，不支持 `none` |
| `MiniMax-M3` | 空 | MiniMax：「仅 M3.1-Flash-Preview 生效」 |

## 测试

- `tests/provider_adapter.rs`：`caps_for` 每条 id 的 `reasoning_efforts` 与上面那张表一致（按集合
  比，不按顺序 —— 顺序由选择器那边自己排）。
- `build_body`：`effort` 在表里时 body 里有 `reasoning_effort`；不在表里时**没有**这个字段，且
  返回的告警里有那个档位名。至少覆盖一个「在表里」（`high` on `kimi-k3`）与一个「不在表里」
  （`xhigh` on `kimi-k3`、`medium` on `deepseek-flash`）。
- 既有那两条遍历断言（`every_builtin_model_id_has_a_capability_entry`、
  `every_registered_model_has_a_self_consistent_window`）自动覆盖新条目，不用改。

## 与既有文档的关系

`src/provider/capability.rs` 的模块注释第 7–13 行按厂商列了那些事实，其中
「`reasoning_effort` 取 low/high/max」「只对 M3.1 生效」那两句要改 —— 它们现在漏了
medium/xhigh。spec §1 与 §4 钉的就是这件事。

## 评论

2026-10-08 实现完毕，`Status: done`。

- `ModelCaps::reasoning_efforts: &'static [ReasoningEffort]`；三张静态表（`LOW_HIGH_MAX`、
  `FIVE_EFFORTS`、`NO_EFFORTS`），因为 `ModelCaps` 按值到处传、保持 `Copy`。
- 顺序就是**弱到强**（枚举声明序），选择器与状态行都靠它，所以 `ReasoningEffort` 派生了
  `Ord`（测试按集合比，顺序不是契约，可排序是）。
- `build_body` 查表；不在表里时那句告警**列出它有的那几档**，空表时说「没有档位，思考档位固定」。
- 模块注释里「`reasoning_effort` 取 low/high/max」「只对 M3.1 生效」那两句按票补齐（medium/xhigh）。
- 顺手加了 `ReasoningEffort::from_token`（票 03 的 `/effort` 要它，大小写不敏感）。
