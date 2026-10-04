# 真机验收清单与收尾

Type: implement
Status: done
Blocked by: 03

> 规格：`.scratch/sandbox/spec.md` 的 `测试决定` 第 3 层。

## 目标

前两层（纯函数、集成）只能验「我们拼对了参数」；这一票验**内核真的拦住了**，并把这次改动的账收干净。

## 落点

`docs/tui-manual-checklist.md`（新增一节或并入既有节）、`.scratch/sandbox/spec.md`（补落地记）、`.scratch/README.md`（票数）。

## 具体行为

1. **手工 / pty 清单**，每条都要在真机上跑出结果并记下原文：
   - 写工作区、在工作区里新建文件、删文件 → 都成功；
   - 写 `$HOME`、写 `/etc` → 失败，消息是「只读文件系统」；
   - `cat ~/.config/fs-agent/config.toml` → 读不到（目录是空的，不是「不存在」）；
   - `cat ~/.ssh/id_*` → 读不到；
   - 工作区里的 `echo x > .env` → 失败；
   - 工作区里的 `echo x > .env.example` → **成功**（模板后缀不在保护之列）；
   - **`git` 的读写**：`git status`、`git add`、`git commit`、`git log` 都要能跑——`.git/config` 与 `.git/hooks` 只读**不该**影响它们（2026-10-01 已在临时仓库上验过一次，真会话里再确认一次）；`git config user.name x` 与写 `.git/hooks/pre-commit` 应当被拒；
   - **`/tmp` 的语义**：同一条命令内写 `/tmp/x` 再读得到；**跨两条命令读不到**——确认工具描述里那句话与实际行为一致；
   - `cargo test` 能跑（缓存目录可写这条验收）；
   - 把 `PATH` 上的 bwrap 拿掉再起一次 → `bash` 拒绝运行，文案给出两条出路；把 `[sandbox] mode` 设成 `"off"` 再试一次 → 正常跑。
2. **收尾**：`.scratch/README.md` 的 `sandbox` 行从「票待拆」改成实际票数与状态；spec 里补一条**落地记**（哪张票做了什么、与规格的偏差、上面那条 `git commit` 的实测结论）。
3. 逐条跑 `cargo test`、`cargo clippy --all-targets`、`python3 scripts/check-language.py`、`python3 scripts/tui-startup-check.py`。

## 验收

清单里每一条都有实测记录。**跑不出来的如实写「没验到」，不要打勾**——这一票的价值全在「我们真的看过」，而不在清单被勾满。
