# ticket 02 prototype snapshots —— 选定组合（2026-09-21 用户看图为证）

**选定 = 四个区域各自 Block 边框（header 底边框即分隔横线）+ airy 留白（header 下、输入区上各 1 行）+ 右栏 26% 夹紧 + header 分隔符 ` · ` + 输入续行缩进 2 格。**

每一张都由 `tui-layout-probe`（修订 2）用 ratatui `TestBackend` 渲染成固定尺寸 buffer 后 dump，**不是手绘**。命令：

```sh
CARGO_TARGET_DIR=/tmp/tui-layout-probe-target cargo run --offline --manifest-path \
.scratch/tui-layout/prototype/Cargo.toml -- .scratch/tui-layout/prototype
```

| 文件 | 尺寸 | 回答的问题 |
| --- | --- | --- |
| `chosen-40x10.txt` | 40x10 | 冻结的最小尺寸：四个边框区域都在，但 airy 留白放不下（转录只剩 1 行内容） |
| `chosen-40x12.txt` | 40x12 | 带 airy 的完整新基线真正需要的最小高度：40x12 |
| `chosen-40x24.txt` | 40x24 | 窄屏：无右栏，header 1 行（cwd 被丢），airy 保留 |
| `chosen-60x24.txt` | 60x24 | 右栏仍隐藏（<80）；header 2 行；airy 保留 |
| `chosen-80x16.txt` | 80x16 | 右栏出现的最低高度：中段内容 4 行，只放得下四个核心字段 |
| `chosen-80x24.txt` | 80x24 | 右栏最窄档：外框 25 列 / 内容 23 列，上下文百分比被丢 |
| `chosen-120x24.txt` | 120x24 | 正常尺寸：header 2 行 + airy + 右栏 31 列（内容 29，含上下文 %） |
| `chosen-174x50.txt` | 174x50 | 很宽：右栏封顶 31 列，转录吃满剩余 |
| `chosen-120x24-input-3-lines.txt` | 120x24 | 输入区 3 行：中段从 12 行降到 10 行，右栏仍满字段 |
| `chosen-120x24-input-12-lines.txt` | 120x24 | 草稿 12 行：输入区吃满 10 行上限，中段只剩 3 行 -> 右栏整个被挤掉 |
| `chosen-too-small-39x24.txt` | 39x24 | 宽度 39：低于最小 40，只显示「终端太小」 |
| `chosen-too-small-40x9.txt` | 40x9 | 高度 9：低于最小 10，只显示「终端太小」 |

## 被否决的旧基线（决议前，仅作记录）

`screens-*.txt` 与 `variant-*.txt` 是**第一版探针**的产物，代表被否决的组合：
中左/中右之间 1 列暗色竖线、其余区域无边框、紧凑留白。
**注意：那些图里的提示行含 `shift+enter 换行`。票 04 已决定不启用键盘增强协议，该提示是错的、已从本源码删除；不要据旧图实现。**
当前基线只看 `chosen-*.txt`。
