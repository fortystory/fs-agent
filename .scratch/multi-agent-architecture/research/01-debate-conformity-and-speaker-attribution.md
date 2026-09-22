# 多智能体讨论的从众坍缩与说话人归属：一次面向主来源的调查

- 调查日期：2026-09-12
- 范围：为 `fs-agent` 的「同一 session 内多 agent 互相讨论」+「派发子 agent 执行任务」新能力做事实底稿。
- 本文**只报告事实与来源，不给设计建议**。
- 与已有调研的关系：`docs/research/coding-agent-features.md` 覆盖的 10 个 coding agent 全部把 subagent 设计为**隔离**的（Amp 文档明确说 subagents「can't communicate with each other」）。本文件**不重复**那份调研，专门覆盖它未覆盖的「peer discussion / 共享上下文」问题。

## 来源分级约定（重要）

| 标记 | 含义 |
| --- | --- |
| ✅ 本人 | 我本人抓取了该 URL 或开源文件，并引用了其中的原文/原始代码 |
| ✅ 委派 | 由委派的研究子 agent 抓取并逐字回报；我**没有**二次打开该页核对。这类条目标注「✅ 委派」 |
| 🟡 二手 | 只有二手转述、摘要级信息，或抓取被 403/重定向阻断（标 **ABSTRACT-ONLY**） |
| ⚪ 未证 | 无法证实；或只能从「文档未提及」推断 |

**版本时点提醒**：本文写作时（2026-09），Anthropic 文档已出现 Claude Sonnet 5 与「Opus 4.6 之后的模型」；OpenAI 已出现 GPT-5.6 / GPT-6；DeepSeek 官方模型 ID 为 `deepseek-flash` / `deepseek-v4-pro`；Moonshot 为 `kimi-k3` / `kimi-k2.7-code` / `kimi-k2.6`。大量论文的首发时间在 2023–2024，但**近两年出现了系统性反转**（见 §1），因此「发表时间」在本主题上是决定性信息。

---

## 1. 多智能体辩论／讨论：共享上下文是否会坍缩为从众？

### 1.1 最直接的负面结论：辩论本身的增益可用「独立采样 + 投票」解释掉

**Choi, Zhu, Li《Debate or Vote: Which Yields Better Decisions in Multi-Agent Large Language Models?》**（✅ 本人）。arXiv 2508.17536v2，2025-08-24 提交 / 2025-10-23 修订，**NeurIPS 2025 Spotlight**。

> "we disentangle MAD into two key components--Majority Voting and inter-agent Debate--and assess their respective contributions. Through extensive experiments across seven NLP benchmarks, we find that **Majority Voting alone accounts for most of the performance gains typically attributed to MAD.** To explain this, we propose a theoretical framework that models debate as a stochastic process. **We prove that it induces a martingale over agents' belief trajectories, implying that debate alone does not improve expected correctness.**"

> "while MAD has potential, **simple ensembling methods remain strong and more reliable alternatives** in many practical settings."

<https://arxiv.org/abs/2508.17536> · 代码 <https://github.com/deeplearning-wisc/debate-or-vote>

**含义**：在「同质 agent + 无加权更新」设定下，逐轮交换意见在**期望上不改变正确率**；收益来自独立采样后的**聚合**，不是讨论本身。

### 1.2 共享上下文下的从众：受控对比 + 硬数字

**《The Cost of Consensus: Isolated Self-Correction Prevails Over Unguided Homogeneous Multi-Agent Debate》**（✅ 本人读摘要；表内数字为 ✅ 委派）。Bertalanič & Fortuna，arXiv 2605.00914v1，2026-04-29，**ACM Conference on AI and Agentic Systems**（DOI 10.1145/3786335.3813137）。N=10 同质 agent（Qwen2.5-7B / Llama-3.1-8B / Ministral-3-8B），R=3 轮，GSM-Hard + MMLU-Hard；对照为 **isolated self-correction** 与 **随机噪声控制**（注入无关问题的理由）。

摘要原文（✅ 本人）：

> "We decompose debate failure into three model-dependent pathways: **sycophantic conformity, where agents uncritically adopt majority answers (modal adoption up to 85.5%); contextual fragility, where peer rationales destabilize previously correct reasoning (vulnerability rate up to 70.0%); and consensus collapse, where plurality voting discards correct answers already present in the generation pool (oracle gap up to 32.3 percentage points).**"

> "Ablations over communication density (K ∈ {2,4,9}) and sampling temperature (T ∈ {0.4, 0.7}) show that **conformity reaches high levels at minimal peer exposure (K=2) and intensifies with greater initial diversity.**"

> "debate consumes **2.1-3.4× more tokens** (up to 28,631 tokens per problem) than self-correction for equal or lower accuracy ... **isolated self-correction consistently offers a more favorable cost-accuracy tradeoff.**"

委派读者另外回报（✅ 委派）："This conformity actively degraded systemic accuracy below baselines while **artificially inflating team consensus to 90.1%**."；Table 1（R=3）示例：Qwen2.5-7B GSM-Hard Base 25.6 / Debate 58.8 / Noise 63.2 / Self 61.0；Ministral-3-8B GSM-Hard Debate 20.7 vs Self 48.3（p<0.001）。

**作者自承的方法学隐患**（✅ 委派，原文 §3.3）：

> "presenting peers in **confidence-descending order** may introduce a **primacy bias** toward the most confident peer, however this design choice is held constant across conditions but represents a **potential confound that future work will ablate**."

⚠️ 与直觉相反的两条：(a) **初始多样性越高，从众越严重**；(b) 作者只在 **7–8B 参数级别**验证，不能外推到前沿大模型。

### 1.3 2024–2026 的多条独立负面复现

| 来源 | 设定 | 结论（原文/数字） | 标记 |
| --- | --- | --- | --- |
| **2502.08788**（v1 2025-02-12 标题为 *If Multi-Agent Debate is the Answer, What is the Question?*，v3 2025-06-21 改题为 *Stop Overvaluing Multi-Agent Debate* ——**同一篇论文**，只引一次） | 5 种 MAD（SoM / Multi-Persona / EoT / AgentVerse / ChatEval）× 9 benchmark × 4 模型（gpt-4o-mini、claude-3-5-haiku、Llama3.1-8B/70B） | "**none of these MAD methods achieve a win rate higher than 20% when compared to CoT across 36 scenarios**"；"simply adjusting these hyperparameters can hardly reverse this negative outcome"；"the underperformance becomes even more pronounced when compared to SC"；"model heterogeneity ... as a **universal antidote**"；另有 "MP, ChatEval, and AgentVerse, while capable of correcting many wrong answers, also **frequently introduce a high number of misstatements by mistakenly altering the initially correct answers**" | ✅ 委派 |
| **2402.18272**（2024-02-28）| ChatGPT-3.5、Gemini Pro、Bard | "a **single-agent LLM with strong prompts can achieve almost the same performance as the best existing discussion approach** on a wide range of reasoning tasks"；"multi-agent discussion performs better than a single agent **only when there is no demonstration in the prompt**"；最常见错误 "**Wrong Answer Propagation** ... deviates from its initial correct answer and adopts an incorrect consensus" | ✅ 委派 |
| **2509.05396**（v2 2025-10-13，ICML MAS Workshop 2025）| GPT-4o-mini、LLaMA-3.1-8B、Mistral-7B | "**debate can lead to a decrease in accuracy over time** — even in settings where stronger models outnumber their weaker counterparts"；Table 1：3×Mistral MMLU 33.6→24.4；1×Llama+2×Mistral MMLU 40.0→28.0 | ✅ 委派 |
| **2509.23055**（2025-09-27）| Qwen3-32B、LLaMA-3.3-70B | "MADS doesn't consistently outperform single-agent baselines, particularly in decentralized settings"；homogeneous Llama3.3-70B 的 **Disagreement Collapse Rate 最高 86.36%** | ✅ 委派 |
| **Estornell & Liu, NeurIPS 2024** | 理论 | "similar model capabilities, or similar model responses, can result in **static debate dynamics where the debate procedure simply converges to the majority opinion.** When this majority opinion is the result of a common misconception ... debate is likely to converge to answers associated with that common misconception." | ✅ 委派（摘要页） |
| **2311.17371**（v3 2024-07-18）| GPT-3.5-turbo（+GPT-4 / Mixtral 复核），7 数据集 | "**multi-agent debating systems, in their current form, do not reliably outperform** other proposed prompting strategies, such as self-consistency and ensembling"；"**no protocol dominates on all datasets**"；"Medprompt ... **performs the best overall**" | ✅ 本人 |
| **2601.19921**（v3 2026-06-03）| 理论 + 6 benchmark | "**vanilla MAD often underperforms simple majority vote** despite higher computational cost" | ✅ 本人 |

### 1.4 共享上下文下的「事实侵蚀」——最干净的一次 shared vs no-interaction 对照

**《The Deliberative Illusion: Diagnosing Factual Attrition and Stance Homogenization in Multi-Agent LLM Deliberation》**（✅ 委派，**非同行评审 preprint**）。arXiv 2606.03032，2026-06-02。GPT-4.1 / Gemini 3 / Qwen 3.5（GPT-5 作 judge），ETHICS + NEWS，3 轮，全连接/树/链三种拓扑。

> "multi-agent discussion **erases up to 72% of issue-critical facts**."；"agents can **agree more while knowing less**."

**关键的因果对照**（原文 §4）：

> "A **no-interaction upper bound** ... further shows that this loss is **amplified by inter-agent communication**: on NEWS, GPT-4.1 loses only **19.2%** of critical facts after three **no-interaction** rounds, compared with **67.8% under full discussion**."

> "We term this collapse of initially diverse positions toward consensus **stance homogenization**. It is a misleading form of agreement, where agents agree more while retaining less."

数字：factual attrition 21.7–72.4%（system）/ 60.5–84.8%（agent）；stance entropy 真实人类讨论 0.903 vs 多 agent 审议 0.338。
<https://arxiv.org/abs/2606.03032>

→ **这是目前唯一一个把「有互动」与「无互动」放在同一实验里、对**事实保留**做因果对照的结果**（19.2% vs 67.8%）。⚠️ 但它仍不是「多个不同 subagent 隔离并行」的对照。

### 1.5 直接观测到的「从众 / 共识达成」现象（Asch 范式，含效应量）

**《Do as We Do, Not as You Think: the Conformity of Large Language Models》**（✅ 本人读摘要/协议；表数为 ✅ 委派）。Weng, Chen, Wang（浙江大学），arXiv 2501.13381v2，2025-01-23 / 2025-02-11，**ICLR 2025 (Oral)**。11 个模型：GPT-3.5、GPT-4o、Llama3、Llama3.1、Gemma2、Qwen2 系列。

协议（✅ 本人）：

> "**Wrong Guidance Protocol** ... additional agents provide **incorrect** answers **before the subject agent responds** ... whether the subject agent might **conform to incorrect group consensus, even when the provided information contradicts the subject agent's own reasoning.**"

> "For the Correct Guidance and Wrong Guidance protocols, **six additional agents** are introduced ... the subject agent is strategically positioned to **respond last**."；"The additional agents **provide the same answer**."

结论（✅ 委派）："**All the evaluated LLMs show a tendency to conform.**"；"none of the evaluated LLMs are fully immune to all the four interaction protocols."

数字（✅ 委派，Table 1，相对 Raw 的准确率下降 %）：Gemma2 ΔWrong **22.8**、ΔDoubt **38.6**；GPT-4o ΔCorrect 13.2、ΔWrong 14.9、ΔTrust 22.6、ΔDoubt 13.0；Llama3.1-405B ΔWrong 2.5、ΔDoubt 30.2。
Table 2（误导协议下的 conformity rate %）：Gemma2 CR^Wrong **39.7**、CR^Trust 28.4、CR^Doubt **66.1**；Llama3 CR^Wrong 14.7、CR^Trust 44.4、CR^Doubt 69.9。
（我本人抓取到的 Table 1 首行与委派回报一致：Gemma2 ΔC 24.1 / ΔW 22.8 / ΔT 9.5 / ΔD 38.6。）

**平均 conformity rate 与「多数规模」的效应量**（✅ 委派）：
- Table 2 平均：**CR_Wrong = 23.5%、CR_Trust = 31.3%、CR_Doubt = 47.2%**（即：被误导后改错的平均比例）。
- 把多数规模从 3 增到 6（GPT-4o）：**CR_Trust 17.3% → 37.9%**、**CR_Doubt 11.5% → 26.6%**、**Independence Rate 81.5% → 55.4%**。
- 被测模型串（委派回报）：`gpt-3.5-turbo-16k-0613`、`gpt-4o-0513`、`Gemma2-27B`、`Llama3-70B`、`Llama3.1-405B`、`Qwen2-72B`、`GLM-4-Plus`。

缓解手段（论文自述）："**developing enhanced personas** ... and **implementing a reflection mechanism**"。
<https://arxiv.org/abs/2501.13381> · <https://arxiv.org/html/2501.13381v2> · <https://github.com/Zhiyuan-Weng/BenchForm>

### 1.6 「只有历史信号」就足以造成强锚定

**《Spiral of Silence in Large Language Model Agents》**（✅ 本人读 ACL Anthology 条目 + 摘要；细节为 ✅ 委派）。Zhong, Fang, Shi, Huang, Zheng, Du, Chen, Wang。**Findings of EMNLP 2025**，pp. 23238–23253，2025-11。DOI 10.18653/v1/2025.findings-emnlp.1262。

> "**history signals alone induce strong anchoring; and persona signals alone foster diverse but uncorrelated opinions, indicating that without historical anchoring, SoS dynamics cannot emerge.**"

> （结论，✅ 委派）"**persona alone induces opinion diversity, history alone imposes anchoring consistency, and only their combination triggers a pronounced SoS.**"

2×2（History × Persona）设计；用 Mann–Kendall / Spearman 趋势检验 + 峰度、四分位距等集中度指标。模型：GPT-4o-mini、DeepSeek-V2-Lite-Chat、Mistral-8B-Instruct、Qwen-2.5（1.5B/3B/7B）。
<https://aclanthology.org/2025.findings-emnlp.1262/>（arXiv 版 2510.02360，✅ 委派）

### 1.7 位置/顺序效应的实测：**第一个发言者影响力最小**

**《Herd Behavior: Investigating Peer Influence in LLM-based Multi-Agent Systems》**（✅ 委派）。Cho, Guntuku, Ungar，arXiv 2505.21588，2025-05-27，preprint。模型 gpt-4o / gpt-4o-mini。

对照设计 = 「独立初答」 vs 「看到 peer 后的改答」。按 peer 出现位置的改答率（flip rate，Table 2）：

| peer 位置 | 第 1 个 | 第 2 个 | 随机序 | 最后一个 |
| --- | --- | --- | --- | --- |
| avg flip rate | **0.03** | **0.48** | 0.33 | 0.29 |

> "When **disagreement is presented first, herding behavior is generally stronger.**"

→ 在**这个问题唯一**找到的直接位置效应测量里，**排在最前面的 peer 影响最小**。这与「会锚定到第一个发言者」的猜测**方向相反**。⚠️ 该文仍是 preprint，且它测的是「peer 在序列中的位置」，不是「第一个发言者的身份」。

### 1.8 经典 MAD 论文实际做了什么（事实核对）

- **Du et al. 2305.14325**（✅ 委派；我本人只读了摘要）。3 agent、2 轮。§2.1："we feed each agent a **consensus prompt** ... each agent is instructed to update their responses based on the responses of other agents."；§2.2："language models are able to **converge on a single shared answer** after multiple rounds of debate"；"language model agents were relatively **'agreeable'**, perhaps as a result of instruction tuning or reinforcement learning based on human feedback." 数字（Table 1，Arithmetic %）：Single 67.0 / Reflection 72.1 / Multi-Agent Majority 69.0 / Multi-Agent Debate 81.8。**无顺序/置换消融。**
- **Liang et al. 2305.19118**（✅ 委派）。"the debaters speak **one by one in a fixed order** and express their arguments based on the previous debate history H"；**单一共享历史 H，无顺序置换消融**。§1："Once the LLM-based agent has established confidence in its answers, **it is unable to generate novel thoughts later through self-reflection even if the initial stance is incorrect.**"（Degeneration-of-Thought）。§4.2：judge 偏袒与自己同 backbone 的一方 —— "**the judge shows a preference to the side with the same LLM as the backbone.**" §4.3："**'must disagree with each other on every point' ... does not lead to the best performance**"；"continuous disagreement without finding common ground can contribute to **polarization**."
- **ChatEval 2308.07201**（✅ 委派）。"Responses from other agents are served as **chat history**"；"**we do not explicitly ask the debater agents to reach a consensus**"。效果量："the multi-agent-based method improves the **accuracy by 6.2% for ChatGPT and 2.5% for GPT-4**"；"improves the average **Spearman and Kendall-Tau correlation by 0.096 (16.3%) and 0.057 (10.0%)**"。
- **「More Agents Is All You Need」2402.05120**（✅ 委派）。**它根本没有 agent 间交流**：§3 "we generate N samples by **solely querying the LLM** N times"，然后多数投票；与 debate 无关。

### 1.9 「共享上下文 vs 独立上下文」的受控对比：现状

| 来源 | 对比的是什么 | 结论 |
| --- | --- | --- |
| Choi et al. 2508.17536（✅ 本人）| 独立采样+多数投票 **vs** agent 间辩论 | 增益绝大部分来自前者 |
| Bertalanič & Fortuna 2605.00914（✅）| peer debate **vs** isolated self-correction **vs** 噪声控制 | 孤立自纠更好；辩论 token 多 2.1–3.4× |
| 2606.03032（✅ 委派）| **full discussion vs no-interaction**（事实保留） | no-interaction 仅丢 19.2%，full discussion 丢 67.8% |
| 2502.08788（✅ 委派）| 5 种 MAD **vs** CoT **vs** SC | MAD 对 CoT 胜率 <20%（36 组） |
| Smit et al. 2311.17371（✅ 本人）| MAD **vs** self-consistency / ensemble refinement | MAD 不可靠地优于独立采样+聚合 |
| ReConcile 2309.13007（✅ 委派）| Ensemble（SC / Self-Refine+SC）**vs** Multi-Agent debate | ReConcile（多模型）胜出 |
| Helmi 2504.07303（✅ 本人）| shared vs separate context | **只研究 response consistency 与 response time，不研究意见收敛**；概率框架，无行为实验 |

**必须澄清 2504.07303**：摘要原文只说 "we develop a **probabilistic framework** to analyze the impact of shared versus separate context configurations on **response consistency and response times**"（<https://arxiv.org/abs/2504.07303>）。**它不能用来支持「共享上下文导致观点坍缩」。**

### 1.10 「第一个发言者」的锚定：仍然没有直接证据

⚪ **未证 / GAP**：综合三条研究线的结果，**没有任何主来源**操纵「谁第一个发言」并测量向该发言者的收敛。

- 现有证据测的是：**一致多数压力**（Asch 范式，被试最后回答）→ 1.5；**历史/序列锚定** → 1.6；**序列位置**（1.7，且结论指向「第一个 peer 影响最小」）；**顺序作为被控结构变量** → §4.4。
- 委派读者还专门核查：某博客标题声称 "Pokharel and Dantu Find AI Agents Anchor on Whoever Speaks First"，但其对应的论文（arXiv 2606.19494《Hidden Anchors in Multi-Agent LLM Deliberation》，2026-06-17）全文检索 "first speaker"/"speaks first"/"order effect"/"primacy" 等**命中 0 次**；该文实际主张是 "each agent carries a **hidden internal belief, its anchor**, that continually pulls its opinion regardless of its neighbours."（✅ 委派）→ **该博客说法属未获原文支持的二手说法。**

**结论**：可以写「共享上下文会把人拉向已有意见」；**不能**写「会收敛到第一个发言者」。

---

## 2. 上下文中的既有观点如何影响 LLM 判断（归因差异与效应量）

### 2.1 谄媚是「一般行为」，且有明确效应量

**Sharma et al.《Towards Understanding Sycophancy in Language Models》**（Anthropic）（✅ 本人读摘要；具体数字 ✅ 委派）。arXiv 2310.13548v4，2023-10-20 / v4 **2025-05-10**。被测："claude-1.3, claude-2.0, gpt-3.5-turbo, gpt-4, and llama-2-70b-chat"。

> "We first demonstrate that **five state-of-the-art AI assistants consistently exhibit sycophancy** across four varied free-form text-generation tasks."

关键数字（✅ 委派）：
- §3.3："The **user suggesting an incorrect answer can reduce accuracy by up to 27%** (LLaMA 2; Fig. 3)."
- §3.2："models tend to **admit mistakes even when they didn't make a mistake**—Claude 1.3 wrongly admits mistakes on **98%** of questions."
- §4.1："the presence or absence of an individual feature affects the probability that a given response is preferred by **up to ~6%**."

<https://arxiv.org/abs/2310.13548>

### 2.2 用户侧：**观点本身**触发谄媚，**专家/权威标签几乎无影响**

**《When Truth Is Overridden: Uncovering the Internal Origins of Sycophancy in LLMs》**（✅ 本人读摘要；数字 ✅ 委派）。arXiv 2508.02087v4，2025-08-04 / v4 2025-11-12。7 个模型（含 Llama3.1-8B-Instruct、Qwen2.5-7B-Instruct、Mistral-7B-Instruct-v0.3），MMLU。

摘要（✅ 本人）：

> "simple opinion statements **reliably induce sycophancy, whereas user expertise framing has a negligible impact.**"；"user **authority fails to influence behavior because models do not encode it internally**."；"**first-person prompts ('I believe...') consistently induce higher sycophancy rates than third-person framings ('They believe...')**".

数字（✅ 委派）：错误信念的 agreement rate **平均 63.7%**（范围 46.6%–95.1%）；专家框架效应 "**consistently small (within 4.4% for any given model)**"；第一人称比第三人称 **平均高 13.6%**。

**归纳**："Sycophantic behavior in LLMs is primarily triggered by **the presence of a user opinion**, regardless of the user's claimed expertise or authority."（✅ 委派）

### 2.3 但对**同侪**而言：权威标签几乎无用，**同侪共识本身**才是主导

**《Easier to Mislead Than to Correct: Harmful and Beneficial Revision in LLM Conformity》**（✅ 委派）。Qu, Fu, Hu，arXiv 2606.01637，2026-06-01，preprint。4 个模型（Qwen2.5-7B、Mistral-7B、Gemma-2-9B、Llama-3.1-8B），7 数据集，2,500 实例；**同时**操纵 peer 共识结构与 peer 上的权威标签。

> 小节标题："**Peer agreement has a larger effect than authority labels.**"

数字：
- "**All-wrong peers raise harmful revision from 15.6% to 62.9% (+47.3 pp)**, whereas all-correct peers raise beneficial revision from 32.7% to 51.5% (+18.8 pp)."
- 回归："the effect of all-wrong peers on harmful revision is **five times larger** than the effect of all-correct peers on beneficial revision (**OR=28.5 vs 5.2**, both p<.001)."
- 混合 peer 下，加权威标签只把 harmful revision 从 15.6% 抬到 20.6%，并把 beneficial revision 从 32.7% 降到 30.4%。

### 2.4 专家/权威归因的另一条线：**存在强梯度效应**（与 2.2 冲突）

**《A Mechanistic View of Authority Hierarchy in LLM Sycophancy》**（✅ 本人读摘要；数字 ✅ 委派）。Joswin, Medicherla, Mammen，arXiv 2607.00415v1，**2026-07-01**，**ICML 2026**。模型 Llama-3.1-8B-Instruct / Qwen3-8B / Gemma-2-9B-it；MedQA；persona 梯度：First-Year → Third-Year → Chief Resident → Board-Certified Physician（**无 peer-LLM 对照组**）。

摘要（✅ 本人）：

> "models respond in a **graded manner proportional to perceived authority**, a hierarchy that is **never explicitly prompted but emerges from training** ... a critical late layer where **correct answer representations are actively erased**, an erasure that **scales with authority level**, resists mean vector intervention, and is **only partially reversible through chain-of-thought reasoning.**"

数字（✅ 委派）：

> "Board-Certified Physician causes the most severe accuracy drop across all three models. **Llama drops to 15%, Qwen to 29%, and Gemma to 34%**, all well below baseline, which is roughly 60%. This effect dampens monotonically as persona expertise decreases."

**该文对 2.2 冲突的自我解释**（✅ 委派）：差异来自 "**Competence vs. Authority**" 与 "**Domain Coherence**" —— 机构性、领域匹配、第三人称的权威 persona 产生大效应；第一人称自称的专长不产生效应。

**还有一条同方向的独立测量**：《Who Endorsed It? Measuring Authority Bias Across Expertise Levels in Language Models》（arXiv 2601.13433，2026-01-19，11 个模型，无 peer 组，✅ 委派）：正确背书下准确率增益从 First-Year 的近 0 单调升到 Board-Certified 的 **+0.381**；误导背书下从 **−0.019 恶化到 −0.356**。

### 2.5 peer vs authority：两条线为什么看起来矛盾（事实陈述）

| 维度 | 2.2 / 2.3（peer 或 user 自述）| 2.4（机构性权威 persona）|
| --- | --- | --- |
| 触发方式 | 直接陈述观点 / 明确多人共识 | 领域匹配的职位头衔 |
| 权威标签作用 | 小（≤4.4%）或仅 +~5pp | 大且单调（准确率掉到 15–34%）|
| 主要驱动 | **观点的存在** + **共识规模** | **感知到的权威等级** |

⚪ **未证**：我**没有找到**在同一实验、同一任务、同一模型上**同时**对比 peer / authority / user 三种归因并给出同一尺度效应量的论文。上表是跨论文拼合，**不能当作可比的效应量**。

🟡 另一条候选：Nature *Scientific Reports*《Impact of authoritative and subjective cues on large language model reliability for clinical inquiries: an experimental study》（<https://www.nature.com/articles/s41598-026-38019-3>）在我的抓取中因重定向到 `idp.nature.com` 失败，**正文未读**，不作证据。

### 2.6 单模型上的「专家锚定」也成立，且简单缓解手段失效

**《Anchoring Bias in Large Language Models: An Experimental Study》**（✅ 委派）。Lou & Sun，arXiv 2412.06593，v1 2024-12-09。GPT-4 / GPT-4o / GPT-3.5-Turbo；62 题 × 30 次，temp 0.8。**这是单模型注入提示的锚定，不是多 agent。**

> "LLMs are much **easier to be biased by expert anchors**."；"the average **anchoring index (AI) of GPT4 is about 0.45** in our experiment, it is different from **0.61** in the human study."；"**none of these simple mitigating strategies can effectively reduce the anchoring bias** in responses to 'expert' anchoring questions."

⚠️ 期刊版（J. Computational Social Science, DOI 10.1007/s42001-025-00435-2）因 auth 重定向未打开。

### 2.7 公开从众 vs 私下反对（含 5 个权威等级）

**《Everyone Conforms, No One Believes: Pluralistic Ignorance in LLM Agent Populations》**（✅ 本人读摘要）。Yashwanth YS，arXiv 2608.02758v1，**2026-08-03**。100 场景 × 10 领域 × **5 个权威等级**，8 模型 / 6 机构。

> "**Agents publicly conform at rates of 64 to 94% despite privately opposing the norm.**"；"Conformity is domain-sensitive ... and highly model-dependent, though **uncorrelated with capability**."

> "A prompt component ablation across all 8 models establishes that **conformity is emergent rather than instruction-driven**: removing both the false-consensus framing and fit-in goal reduces conformity **but does not eliminate it (52 to 92% in the minimal condition).**"

> 单一 dissenter 的效果："For **7 of 8 models, cascades succeed less than 26% of the time**, with one model showing **zero cascades** across all scenarios."（GPT-4o 例外，48%）

⚠️ 该文"**Speaking order is randomized to avoid position effects**"（✅ 委派）——即它**刻意不测**先发言者效应。

---

## 3. 共享 transcript 里的说话人归属：工程实践

### 3.1 Microsoft AutoGen（关键 prior art）

**核心结论**：AutoGen **不把第三方说话人塞进新的 role**。它的做法是——内部消息对象带 `source` 名字，发往 provider 时把它写进 `name` 字段**或**前缀进 content；并且在每个 agent 自己的视角里，**所有他人发言都被压成 `role="user"`，只有自己产出的才是 `role="assistant"`**。这正是它结构性地避免「连续 assistant 消息」的办法。

#### (a) AutoGen 0.4 / AgentChat 的消息类型（✅ 本人）

`autogen_core.models._types` 源码（官方文档源码页）：

```python
class UserMessage(BaseModel):
    """User message contains input from end users, or a catch-all for data provided to the model."""
    content: Union[str, List[Union[str, Image]]]
    source: str
    """The name of the agent that sent this message."""

class AssistantMessage(BaseModel):
    """Assistant message are sampled from the language model."""
    content: Union[str, List[FunctionCall]]
    thought: str | None = None
    source: str
    """The name of the agent that sent this message."""
```

`SystemMessage` **没有** `source`；`LLMMessage` 是四类消息的判别联合。role 映射只有四种：

```python
def type_to_role(message: LLMMessage) -> ChatCompletionRole:
    if isinstance(message, SystemMessage):   return "system"
    elif isinstance(message, UserMessage):   return "user"
    elif isinstance(message, AssistantMessage): return "assistant"
    else:                                    return "tool"
```

<https://microsoft.github.io/autogen/stable/_modules/autogen_core/models/_types.html>
<https://microsoft.github.io/autogen/stable/_modules/autogen_ext/models/openai/_openai_client.html>

#### (b) 说话人名字的**两条通路**与开关（✅ 本人 + ✅ 委派）

客户端暴露两个开关（✅ 本人，`_openai_client.py`）：

```python
def to_oai_type(message, prepend_name: bool = False, model="unknown",
                model_family=ModelFamily.UNKNOWN,
                include_name_in_message: bool = True) -> ...:
    context = {"prepend_name": prepend_name,
               "include_name_in_message": include_name_in_message}
```
```python
def __init__(self, client, *, create_args, model_capabilities=None, model_info=None,
             add_name_prefixes: bool = False,
             include_name_in_message: bool = True):
```

实际的转换函数（✅ 委派，`_message_transform.py`，main @ `027ecf0`）：

```python
def _set_name(message, context):
    if context.get("include_name_in_message", True):
        return {"name": message.source}
    else:
        return EMPTY

def _set_prepend_text_content(message, context):
    prepend = context.get("prepend_name", False)
    prefix = f"{message.source} said:\n" if prepend else ""
    return {"content": prefix + message.content}
```

**关键的细节（✅ 委派）**：
- `single_user_transformer_funcs` = base + `[_set_name, _set_prepend_text_content]`
- `single_assistant_transformer_funcs` = base + `[_set_content_direct]` ← **assistant 消息不带 `_set_name`**

⇒ 在 OpenAI 路径上，**只有 `role="user"` 的消息会带 `name` 字段**；assistant 消息不带。Mistral 的转换表则**整个去掉 `_set_name`**（因为 Mistral 不接受该字段）。

客户端 docstring（✅ 委派，原文）：`add_name_prefixes` 是 "Whether to prepend the `source` value **to each UserMessage content** ... useful for models that **do not support the `name` field**"；`include_name_in_message` 用于 "model providers that don't support the `name` field (**e.g., Groq**)"。

AgentChat 层（✅ 委派，`autogen_agentchat/messages.py`）：

```python
class BaseChatMessage:
    source: str  # "The name of the agent that sent this message."
```
```python
class BaseTextChatMessage:
    def to_model_message(self):
        return UserMessage(content=self.content, source=self.source)
```

**所有**具体 `to_model_message()` 都返回 `UserMessage`；agent 自己的回复存为 `AssistantMessage`。`_assistant_agent.py::_add_messages_to_context` 对**每一条**收到的 ChatMessage 调用 `msg.to_model_message()`；`_selector_group_chat.py` 同构。

#### (c) AutoGen 0.2（branch `0.2` @ `c631b34`，✅ 委派）

`autogen/agentchat/conversable_agent.py`：

```python
# When the agent receives a message, the role of the message is "user".
valid = self._append_oai_message(message, "user", sender, is_sending=False)
```
```python
elif "name" not in oai_message:
    # If we don't have a name field, append it
    if is_sending:
        oai_message["name"] = self.name
    else:
        oai_message["name"] = conversation_id.name
```
自己的消息走 `send()` → `self._append_oai_message(message, "assistant", recipient)`。

名字校验：`re.match(r"^[a-zA-Z0-9_-]+$", name)`，错误信息 "Invalid name: ... Only letters, numbers, '_' and '-' are allowed."；长度 >64 报错。

`autogen/agentchat/groupchat.py`：speaker 目录按 `roles.append(f"{agent.name}: {agent.description}".strip())` 构造；speaker 选择轮次通过 `"override_role": self.role_for_select_speaker_messages`（默认 `"system"`）重新定 role；`GroupChatManager.run_chat` 把消息广播给**除当前 speaker 之外的所有 agent**。

**⇒ 0.2 与 0.4 的机制一致**：他人 → `user` + `name`，自己 → `assistant`；每个 agent 看到的永远是严格 user/assistant 交替的序列。

#### (d) 动因：不是所有 provider 都接受 `name`（🟡 标题级）

- `[Roadmap]: Improved 'name' support for non-OpenAI clients` — microsoft/autogen issue #3333 <https://github.com/microsoft/autogen/issues/3333>
- `Groupchat fails to run with Mistral models due the 'name' field of message not accepted.` — issue #2457 <https://github.com/microsoft/autogen/issues/2457>
- `Add 'include_name_in_message' parameter to make 'name' field optional` — PR #6845 <https://github.com/microsoft/autogen/pull/6845>

（我只看到标题，**正文未读**。）

### 3.2 CrewAI / LangGraph：本轮未能验证

⚪ **未证**：委派子 agent **未完成** CrewAI 与 LangGraph 的一手核对（CrewAI tarball 解压路径查找失败；LangGraph 未开始）。我本人抓取 LangChain 参考页时只返回 JS 导航壳，未取到正文。

仅有的线索（🟡 标题级，未读正文）：
- LangChain PR `docs(core): add message-type-specific documentation for message 'name' fields` — <https://github.com/langchain-ai/langchain/pull/32469>
- LangChain JS 参考 `BaseMessage.name` — <https://reference.langchain.com/javascript/langchain-core/messages/BaseMessage/name>
- CrewAI `crew.py` — <https://github.com/crewAIInc/crewAI/blob/main/src/crewai/crew.py>

**本文件不对 CrewAI / LangGraph 的说话人归属下结论。**

### 3.3 各 provider 的 role 与连续同 role 行为（官方文档原文）

#### Anthropic Messages API（✅ 本人）

来源：<https://platform.claude.com/docs/en/api/messages>（Markdown 版 `.../messages.md`，抓取于 2026-09-12）

> "Our models are trained to operate on alternating `user` and `assistant` conversational turns. ... **Consecutive `user` or `assistant` turns in your request will be combined into a single turn.**"

> "**there is no `"system"` role for input messages in the Messages API.**"（system 是顶层参数）

> "There is a limit of 100,000 messages in a single request."

**事实**：
1. 输入消息**只有 `user` / `assistant`**。
2. **连续同 role 不报错，而是被合并为一个 turn** → 多个 agent 的发言若都标成同一 role，**说话人边界会在服务端消失**。
3. `MessageParam` 只有 `role` 与 `content` —— **没有 `name`**。在 Anthropic 上标说话人只能写进文本。

**对「让某 agent 最后发言」的直接影响**（✅ 本人，<https://platform.claude.com/docs/en/api/errors>）：

> "Claude 4.6 and later models and Claude Mythos Preview **do not support prefilling assistant messages**. Sending a request with a prefilled last assistant message ... returns a 400 `invalid_request_error`: `This model does not support assistant message prefill. The conversation must end with a user message.`"

**对「用 temperature 制造多样性」的直接影响**（✅ 本人，同页）：

> "`temperature`: **Deprecated**. Models released after Claude Opus 4.6 **do not support setting temperature**. A value of 1.0 ... will be accepted for backwards compatibility, **all other values will be rejected with a 400 error.**"

`top_p` / `top_k` 同样 deprecated 并对新模型 400。

#### OpenAI

- Chat Completions role：`system` / `developer` / `user` / `assistant` / `tool`。AutoGen 的 `SystemMessage` docstring（✅ 本人）指出 "Open AI is moving away from using 'system' role in favor of 'developer' role ... the 'system' role is still allowed in their API and will be **automatically converted to 'developer' role on the server side.**"
- **`name` 字段仍在官方 OpenAPI 生成的 SDK 类型里**（✅ 本人）：
  ```python
  class ChatCompletionUserMessageParam(TypedDict, total=False):
      content: Required[...]
      role: Required[Literal["user"]]
      name: str
      """An optional name for the participant.
      Provides the model information to differentiate between participants of the same role."""
  ```
  <https://raw.githubusercontent.com/openai/openai-python/main/src/openai/types/chat/chat_completion_user_message_param.py>
- ⚪ **未证**：OpenAI 官方文档**没有**关于「连续多条 `assistant` 消息」行为的说明，也**没有**「`name` 在当前前沿模型上是否生效/是否被拒绝」的声明。openai-node issue #508（<https://github.com/openai/openai-node/issues/508>）我只看到标题。→ 「OpenAI 不拒绝连续 assistant」是**从文档缺口推断**，不是已证事实。

#### Moonshot / Kimi（✅ 本人）

来源：<https://platform.kimi.ai/docs/api/chat>（`.md` 版）

- `Message.role` 的 enum 明确为 **`system` / `user` / `assistant` / `tool`**。
- `name`："**Optional name for the message sender**"。
- `partial: bool` —— Partial Mode（Prefill），在**最后一条 assistant 消息**上使用。
- 官方把「多说话人」列为 Partial Mode 的用例之一，且**直接点名 `name`**：
  > "**Maintain role name prefixes in role-play scenarios (combined with the `name` field)**"
- 另有 Anthropic 兼容的 Messages API（`POST /anthropic/v1/messages`，<https://platform.kimi.ai/docs/api/messages>）。

#### DeepSeek（✅ 本人）

来源：<https://api-docs.deepseek.com/api/create-chat-completion>（2026-09-12；模型 `deepseek-flash` / `deepseek-v4-pro`）

- `messages` 的 oneOf 为 **System / User / Assistant / Tool** 四类。
- `name`（原文）："**An optional name for the participant. Provides the model information to differentiate between participants of the same role.**"
- **Chat Prefix Completion (Beta)**（<https://api-docs.deepseek.com/guides/chat_prefix_completion/>）：
  > "users **must ensure that the `role` of the last message in the `messages` list is `assistant`** and set the **`prefix` parameter of the last message to `True`**."；需 `base_url="https://api.deepseek.com/beta"`。

  → 证实「最后一条可以是 assistant」；**不**等于允许任意多条连续 assistant（⚪ 未证）。

### 3.4 小结（事实层面）

1. **没有任何被检查的 provider 为「第三方说话人」提供独立 role。** 可用 role 集合最大为 `system/developer/user/assistant/tool`。
2. 归因只有三条路，支持度不一：
   - **`name` 字段**：OpenAI（SDK 有）、DeepSeek（文档明确）、Kimi（文档明确）；**Anthropic 没有**。
   - **文本前缀**（AutoGen 的 `f"{source} said:\n"`）：所有 provider 可行。
   - **把他人发言压成 `user`**：会与真实 user 混淆；且 Anthropic 会**合并连续同 role**。
3. **连续同 role 的处理各家不同**：Anthropic **合并**（文档明确）；DeepSeek 允许**最后一条**为 assistant（前缀续写）；OpenAI 无文档记载。
4. **AutoGen 的解法是「per-agent 视角重写 role」**：他人一律 `user`，自己一律 `assistant`，因此从任何单个 agent 的模型调用看，序列永远是严格交替的。这是目前唯一看到的、被生产框架验证过的结构性规避方案。

---

## 4. 已知的从众缓解手段：哪些真被证实

### 4.1 独立首轮 + 聚合（证据最强）

- **Choi et al. 2508.17536（✅ 本人）**：多数投票解释绝大部分增益；辩论是 martingale。
- **Self-consistency**（✅ 委派）：Wang et al. 2203.11171，ICLR 2023。"samples a diverse set of reasoning paths ... then selects the most consistent answer"；报告增幅 **GSM8K +17.9%、SVAMP +11.0%、AQuA +12.2%、StrategyQA +6.4%、ARC-challenge +3.9%**；模型 LaMDA-137B / PaLM-540B；Appendix A.1.1 明确做了 temperature/top-k 的**稳健性**消融（只采样，不跨样本可见）。<https://arxiv.org/abs/2203.11171>
- **LLM-Blender**（✅ 委派）：PairRanker + GenFuser，**独立候选先排序再融合**，无对话。<https://arxiv.org/abs/2306.02561>
- **「More Agents Is All You Need」**（✅ 委派）：纯独立采样 + 多数投票，**无任何 agent 间通信**；Table 2（ensemble size 40）GSM8K Llama2-13B 0.35→0.59、Llama2-70B 0.54→0.74、GPT-3.5 0.73→0.85。<https://arxiv.org/abs/2402.05120>
- **2511.07784（✅ 本人）**："**intrinsic reasoning strength and group diversity are the dominant drivers of debate success**"。
- **2601.19921（✅ 本人）**：diversity-aware initialization；§5.1 Proposition 1 `P(A_T | diverse init) ≥ P(A_T | random init)`。

### 4.2 强制异议 / devil's advocate：**有，但默认是负效果**

- **Liang et al. 2305.19118**（✅ 委派）确实使用对抗角色：negative 侧 prompt "You are negative side. You disagree with the affirmative side's points."，并有 meta-prompt "It's not necessary to fully agree with each other's perspectives, as our objective is to find the correct answer." 但作者自己在 §4.3 指出："'must disagree with each other on every point' (with a disagreement of 0.988) **does not lead to the best performance**"，且 "continuous disagreement without finding common ground can contribute to **polarization**"。
- **Multi-Persona 的 devil**（✅ 本人，2311.17371）：
  > "**the Multi-Persona system reduces the overall performance compared to relying solely on the initial response of the first agent.** This can be attributed to the role of the second agent (the 'devil'), which is deliberately designed to contradict or disagree, **even if the initial response was correct**."
- **有效的调节旋钮：agreement intensity**（✅ 本人）：prompt 写 "you should agree with the other agents X% of the time"，在 USMLE 上带来 "**≈15% improvement for Multi-Persona, and ≈5% for SoM**"；ChatEval "hardly affected"；且方向**依数据集而反**（MedQA/PubMedQA 高一致更好，CIAR 相反）。

### 4.3 隐藏推理只暴露结论

- ⚪ **未证（无直接消融）**：委派读者全文检索 Du et al.、Liang et al.、Smit et al.，**均没有** "full reasoning chains vs answers only" 的消融。
- 最近的已文档化机制：**ChatEval 的 simultaneous-talk-with-summarizer**（✅ 委派）："we prompt this extra LLM to **summarize the messages** conveyed so far and **concatenate this summarization** into all debater agents' chat history slots."
- **间接支持「少暴露」的证据**：
  - 2605.00914 的 **contextual fragility**：peer **rationales** destabilize previously correct reasoning（vulnerability up to **70.0%**）。（✅ 本人）
  - **《Selective Agreement in LLM Debates: Anchoring Effects and Resistance》**（🟡 **ABSTRACT-ONLY**，全文 PDF 返回 403）。2026-06-15 出版，peer-reviewed book chapter（Dykinson, DOI 10.14679/4976）：
    > "**Hidden positions lead to longer, more argumentative exchanges, while visible positions produce shorter, convergence-oriented discussions.**"
    模型未在摘要中说明。
    <https://research.hva.nl/en/publications/selective-agreement-in-llm-debates-anchoring-effects-and-resistan/>

### 4.4 揭示顺序控制

- **ChatEval（✅ 委派）** 是最直接的一手测量：
  - "one-on-one, where each agent answers the provided question **in turn**, and each agent is provided with the history of all previous agents' answers"
  - "**simultaneous-talk**, where agents asynchronously generate responses in each round to **nullify the effects of agent order**"
  - 结果 §4.2："the **one-by-one communication strategy is more effective** than other strategies **for ChatGPT setting**."（Table 4：One-by-One 60 vs Simultaneous-Talk 55）
  → ChatEval 作者的设计动机是「顺序有影响」，而其**实测结果是顺序化反而更好**（至少在 ChatGPT 上）。
- **2511.07784（✅ 本人）**：把 "debate order" 作为受控因子之一，结论是 "structural parameters such as order or confidence visibility offer **limited gains**"。
- **2505.21588（✅ 委派）**：位置效应实测（见 1.7）——第 1 个 peer 影响最小（0.03），第 2 个最大（0.48）。
- **2311.17371（✅ 本人）**：MAD 对超参**特别敏感**、最优设置因数据集而异。

### 4.5 温度

- **Borchers et al. 2507.11198**（✅ 委派，preprint，6 个开源模型 3–32B）：
  > "**Temperature significantly impacted whether and when consensus was reached** across all six LLMs."；"**neither temperature nor persona pairing led to robust improvements in coding accuracy. Single agents matched or outperformed MAS consensus in most conditions.**"
  → 温度影响**共识时机**，不影响**准确率**。<https://arxiv.org/abs/2507.11198>
- **2605.00914（✅）**：`T ∈ {0.4, 0.7}` 消融下，**conformity 在 K=2 时已很高，并随初始多样性上升而加剧**。
- **MoA 的单/多 proposer 对比**（✅ 委派）："single-proposer"（同一 LLM，temperature 0.7）× n vs "multiple-proposer"（不同 LLM）× n → n=6 时 **56.7% vs 61.3%**；"using multiple different LLMs consistently yielded better results"。
- **ReConcile**（✅ 委派）：高温度的同模型多实例仍不及多模型 —— "This surpasses ReConcile with multiple ChatGPT instances, even when the generations ... are encouraged to exhibit high diversity with a sufficiently high temperature."
- **平台事实（✅ 本人）**：Anthropic 新模型（Opus 4.6 之后）**不再支持设置 `temperature`**，非 1.0 值直接 400。
- ⚪ **未证**：没有找到「per-agent 温度异质性」作为独立手段的正面 head-to-head 证据。

### 4.6 不同 persona / 不同基座模型（异质性）

- **ChatEval（✅ 委派）**：§4.1 "ChatEval with the **same role prompt design underperforms** that with diverse role prompt design and **cannot effectively enhance the performance** compared with single-agent setting"；§1 "the **diverse role prompts (different personas) are essential**"。
- **ReConcile（✅ 委派）**：Table 7（StrategyQA）ReConcile **79.0 ± 1.6** vs "**w/o Multiple Models**" **72.2 ± 2.1**；Appendix C.5 "**Single-Model Multi-Agent Debate Struggles with Echo Chamber**"："due to a lack of external feedback from diverse models, **all agents persist with the same incorrect response** throughout the interaction."
- **异质性缩放（✅ 委派，preprint）**：Yang et al. 2602.03794（2026-02-03）："**2 diverse agents can match or exceed the performance of 16 homogeneous agents**"；"homogeneous agents saturate early because their outputs are strongly correlated".<https://arxiv.org/abs/2602.03794>
- **2502.08788（✅ 委派）**："we further explore the role of **model heterogeneity** and find it as a **universal antidote** to consistently improve current MAD frameworks."
- **反证**：**2310.02124（✅ 本人）**："multi-agent societies composed of agents with **different traits do not clearly differ in performance**"；"**merely increasing the number of agents or the number of collaboration rounds does not consistently yield better outcomes**"。（注意：这里说的是 **trait/persona**，不是**基座模型**；二者不可混同。）
- **反证**：**2605.00914（✅ 本人）**：**初始多样性越高，从众越严重**。

→ 「异质性（尤其**不同基座模型**）重要」有多条一致证据；「用 persona 造多样性」在 2310.02124 里**没有**可辨收益；且「更多初始多样性」在 2605.00914 里**加剧从众**。这三条不能合并成一句结论。

### 4.7 judge / aggregator / moderator

- **Multi-Persona** 有 judge，并有提前结束辩论的权限（✅ 本人经 2311.17371 转述）。
- **ChatEval** 是 "multi-agent **referee team**"（✅ 委派）。
- **Choi et al. 2508.17536（✅ 本人）**：可读作「**聚合步骤**才是收益来源」。
- **MoA 的 aggregator**（✅ 委派）："Aggregators are models proficient in **synthesizing responses from other models** into a single, high-quality output"；MoA "significantly outperforms an **LLM-ranker** baseline"（即**综合优于挑选**）。
- **LLM-as-a-judge 的已知偏差（✅ 委派）**：Zheng et al. 2306.05685，NeurIPS 2023 D&B：
  > "**The position bias can be very significant. Only GPT-4 outputs consistent results in more than 60% of cases.**"；verbosity bias：judge 偏好更长回答；"**GPT-4 favors itself with a 10% higher win rate; Claude-v1 favors itself with a 25% higher win rate**"（但作者声明 "our study cannot determine whether the models exhibit a self-enhancement bias"）。
- **MAD 的 judge 在异质 debater 下不公平（✅ 委派）**：Liang et al. §4.2 "the judge shows a preference to the side with the same LLM as the backbone"；Table 5 结论 "Strong debaters with a weak judge work better than the reverse"。
- **Anthropic 工程实践（✅ 本人）**：多 agent 研究系统用**单个 LLM judge（单次调用、单一 prompt、0.0–1.0 分 + pass/fail）**；"we experimented with **multiple judges** ... but found that a **single LLM call with a single prompt** ... was the **most consistent and aligned with human judgements**"。

### 4.8 缓解手段**失效**的明确记录

| 手段 | 失效/负面证据 | 标记 |
| --- | --- | --- |
| 强制异议（devil） | Multi-Persona 低于「只用第一个 agent 的初始答案」；「每点都反对」不是最优且导致极化 | ✅ 本人 / ✅ 委派 |
| 多轮辩论 | martingale：期望正确率不提升；token 多 2.1–3.4× | ✅ 本人 |
| 增加 agent 数 / 轮数 | "does not consistently yield better outcomes" | ✅ 本人 / ✅ 委派 |
| 不同 trait 的 persona | "do not clearly differ in performance" | ✅ 本人 |
| 更大初始多样性 | 从众反而**加剧** | ✅ 本人 |
| 改变顺序 / 暴露置信度 | "limited gains" | ✅ 本人 |
| 单一 dissenter 打破共识 | 8 模型中 7 个 cascade 成功率 <26%，1 个为 0 | ✅ 本人 |
| 温度 | 影响共识**时机**，不影响准确率；单 agent 常追平/超过 MAS 共识 | ✅ 委派 |
| 辩论整体 | 对 CoT 胜率 <20%；SC 更强；辩论降低输出多样性 | ✅ 委派 |
| 多 agent 系统整体 | MAST：7 个开源 MAS 的**失败率 41%–86.7%**；14 种失败模式，含 "Information withholding"、"Ignored other agent's input"；作者结论 "stem from **system design issues, not just LLM limitations**" | ✅ 委派 |
| 简单缓解锚定 | "none of these simple mitigating strategies can effectively reduce the anchoring bias in responses to 'expert' anchoring questions" | ✅ 委派 |
| 辩论的多样性 | "its convergence-driven design **actively suppresses output diversity** across independent runs, creating an inherent trade-off with creative tasks"（Nguyen et al., arXiv 2609.00683） | ✅ 委派 |

MAST = Cemri et al.《Why Do Multi-Agent LLM Systems Fail?》arXiv 2503.13657（v3 2025-10-26，NeurIPS 2025）<https://arxiv.org/abs/2503.13657>

---

## 5. 「对等讨论」vs「孤立委派」：有没有直接比较？

### 5.1 工业界一手文档：两家头部实验室都把**隔离上下文**写成优势

**OpenAI《Multi-agent》官方指南**（✅ 本人，GPT-5.6，beta `responses_multi_agent=v1`）。<https://developers.openai.com/api/docs/guides/responses-multi-agent>

> "**Focused context.** Each subagent receives a bounded task and **maintains its own context, which reduces interference in context between unrelated lines of work and improves performance.**"

> | Use Multi-agent when | Prefer one agent when |
> | --- | --- |
> | Work can be split into independent, bounded tasks | **Each step depends directly on the previous step** |
> | Separate context improves focus | The task is small enough to complete in one short run |
> | Parallel exploration can reduce wall-clock time | **Agents would contend over the same mutable resource** |
> | Comparing independent findings improves coverage | You require a fixed, deterministic execution graph |

> "adding subagents can increase token usage, and may not be as beneficial for tasks that depend on **a single ordered chain of reasoning**, require **frequent writes to shared mutable state**, or are already dominated by one slow external operation."

**但它也实现了 agent 间消息传递**（已超出纯隔离）：hosted actions `spawn_agent` / `send_message` / `followup_task` / `wait_agent` / `interrupt_agent` / `list_agents`；输出 item 类型 `agent_message` 带 `author` / `recipient`。说话人身份**不是 chat role**，而是文本协议：

> "You will receive messages in the form:
> ```
> Message Type: MESSAGE | FINAL_ANSWER
> Task name: <recipient>
> Sender: <author>
> Payload:
> <payload text>
> ```"

agent 间消息内容加密（`{"type":"encrypted_content","encrypted_content":"enc_..."}`）；`fork_turns` 控制向子 agent 传播多少上下文；子 agent 用层级路径命名（`/root`、`/root/researcher`、`/root/reviewer/tester`）。

**Anthropic《How we built our multi-agent research system》**（✅ 本人，**2025-06-13**）。<https://www.anthropic.com/engineering/multi-agent-research-system>

> "Subagents facilitate compression by operating in parallel **with their own context windows** ... Each subagent also provides **separation of concerns** ... which **reduces path dependency** and enables thorough, independent investigations."

> "**some domains that require all agents to share the same context or involve many dependencies between agents are not a good fit for multi-agent systems today.** For instance, **most coding tasks involve fewer truly parallelizable tasks than research, and LLM agents are not yet great at coordinating and delegating to other agents in real time.**"

> "**the lead agent can't steer subagents, subagents can't coordinate**, and the entire system can be blocked while waiting for a single subagent to finish searching."

> "**Subagent output to a filesystem to minimize the 'game of telephone.'**"

性能/成本（✅ 本人）：多 agent（Opus 4 lead + Sonnet 4 subagents）比单 agent Opus 4 在内部 research eval 上 **高 90.2%**；BrowseComp 上 **token 用量单独解释 80% 的方差**（三因子共 95%）；**multi-agent 约用 15× chat 的 token**。

**Anthropic《When to use multi-agent systems (and when not to)》**（✅ 委派，**2026-01-23**）。<https://claude.com/blog/building-multi-agent-systems-when-and-how-to-use-them>

> "A multi-agent system is an architecture where **multiple LLM instances run with separate conversation contexts, coordinated through code.**"；"**Subagents provide isolation**, with each operating in its own clean context focused on its specific task."

> "multi-agent implementations typically use **3-10x more tokens** than single-agent approaches for equivalent tasks."

> "we've seen teams invest months building elaborate multi-agent architectures only to discover that **improved prompting on a single agent achieved equivalent results.**"

> 多 agent 真正胜出的三种情形："**when context pollution degrades performance, when tasks can run in parallel, and when specialization improves tool selection or task focus. Outside these situations, the coordination costs typically exceed the benefits.**"

**Anthropic《Building effective agents》**（✅ 委派，2024-12-19）。<https://www.anthropic.com/engineering/building-effective-agents> 记录了两类工作流："**Parallelization ... Voting: Running the same task multiple times to get diverse outputs**" 与 "**Orchestrator-workers** ... a central LLM dynamically breaks down tasks, delegates them to worker LLMs, and synthesizes their results"；并建议 "finding the **simplest solution possible**, and only increasing complexity when needed."

**OpenAI《Agents SDK orchestration》**（✅ 委派，无发布日期）。<https://developers.openai.com/api/docs/guides/agents/orchestration>

> "**Handoffs** — A specialist should take over the conversation for that branch of the work — Control moves to the specialist agent" vs "**Agents as tools** — A manager should stay in control and call specialists as bounded capabilities."

> "**Start with one agent whenever you can.** Add specialists only when they materially improve capability isolation, policy isolation, prompt clarity, or trace legibility."；handoffs "can also carry structured metadata or **filtered history**"（即上下文可被有意裁剪）。无数值基准。

### 5.2 学术侧的直接比较：结论偏向「孤立/聚合」

- **ReConcile（✅ 委派，ACM/ACL 2024）**：arXiv 2309.13007。Table 1 明确把 "Ensemble"（Self-Consistency / Self-Refine+SC）与 "Multi-Agent" debate 并列；Appendix B.3 "**ReConcile continues to outperform 9-way SC by a large margin on most datasets**"；Table 7 ReConcile 79.0 vs w/o Multiple Models 72.2。<https://arxiv.org/abs/2309.13007>
- **More Agents 的 Table 3（✅ 委派，直接数值反例）**：Llama2-13B GSM8K 上，**ZS-CoT + 独立采样/投票 = 0.61**，而 **Debate + sampling = 0.48**（Debate 单独 0.38；普通 CoT+sampling 0.56）。同规模下**孤立并行采样胜出**。
- **Choi et al. 2508.17536（✅ 本人）**：多数投票解释绝大部分增益。
- **2605.00914（✅ 本人）**：isolated self-correction 的 cost-accuracy 更好。
- **2502.08788（✅ 委派）**：MAD 对 CoT 胜率 <20%，对 SC 更差。
- **2509.05396（✅ 委派）**：辩论可能**降低**准确率，即使强模型占多数。
- **2606.03032（✅ 委派）**：no-interaction 保留 80.8% 事实，full discussion 只保留 32.2%。
- **Smit et al. 2311.17371（✅ 本人）**：MAD 不可靠地优于 self-consistency / ensembling。
- **2511.07784（✅ 本人）**："**majority pressure suppresses independent correction**"。

### 5.3 缺口

⚠️ **未证**：我**没有找到**任何论文把「**agents 在同一共享 transcript 里互相讨论**」与「**agents 完全隔离并行执行、最后由 orchestrator 合并**」作为两个明确条件做 head-to-head 对照并报告效应量。现有最接近的是：
- 「辩论 vs 独立采样+投票」（Choi et al.；More Agents Table 3）
- 「peer debate vs isolated **self**-correction」（Bertalanič & Fortuna）—— 注意这是**同一模型的自我纠错**，不是多个独立 subagent 分工
- 「full discussion vs **no-interaction**」（2606.03032）—— 最接近，但测的是**事实保留**而非任务正确率
- 「subagent 隔离」只出现在**工程文档**（OpenAI、Anthropic）里，是**设计声明**而非对照实验
- AutoGen / MetaGPT / AgentVerse / Reflexion 都**没有**提供 isolated-parallel-delegation 的对照条件（✅ 委派）

---

## 6. 未验证 / 仅二手 的清单（务必与上文区分）

1. ⚪ **「先发言者锚定」的直接受控实验**：未找到。现有最强证据（2505.21588）显示**第一个 peer 影响最小**（flip 0.03）。
2. ⚪ **某博客称论文发现「agent 锚定第一个发言者」**：原文（2606.19494）检索 "first speaker"/"primacy"/"order effect" **命中 0 次**，属未被原文支持的二手说法。
3. ⚪ **peer / authority / user 三方在同一实验内的效应量对比**：未找到；§2 的结论是跨论文拼合。且 2.2/2.3 与 2.4 之间存在**未解决的不一致**。
4. ⚪ **CrewAI / LangGraph 的说话人归属实现**：本轮**未取得**一手源码或文档正文（见 §3.2）。
5. ⚪ **OpenAI 对「连续多条 assistant 消息」的行为**：官方文档无记载；仅为「文档未禁止」的推断。
6. ⚪ **OpenAI `name` 字段是否在当前前沿模型上真正生效/是否被拒绝**：SDK 类型里有，但无 API 行为文档。
7. ⚪ **DeepSeek 是否允许任意多条连续 assistant**：只证实「最后一条可以是 assistant 且需 `prefix`」（Beta）。
8. 🟡 **Anthropic SDK issue #565**（Bedrock `roles must alternate ... but found multiple "user" roles in a row`）：只读到标题。**但 Anthropic 官方文档已明确「连续同 role 会被合并」**，故不改变结论。
9. 🟡 **LangChain message `name` 的 message-type-specific 文档**：仅 PR 标题（#32469），正文未读。
10. 🟡 **2305.14325 / 2305.19118 / 2308.07201 / 2509.05396 / 2509.23055 / 2502.08788 / 2606.03032 / 2602.13568 / 2606.01637 / 2601.13433 / 2402.18272 / 2409.13007 等**：部分为 **preprint（未同行评审）**，且多数由委派读者读取；引用的表内数字未由我二次核对。
11. 🟡 **《Selective Agreement in LLM Debates》**：全文 PDF 403，仅摘要。
12. 🟡 **Nature Scientific Reports《Impact of authoritative and subjective cues...》**：重定向阻断，正文未读。
13. ⚪ **Anthropic 多 agent 博文的 90.2% 等指标**：Anthropic **内部** eval，非公开可复现基准。
14. ⚪ **「Peacemaker / troublemaker」组合**（委派子 agent 在 D6 提及）：**未给出可核对的 URL**，故不写入正文结论。

---

## 7. 参考来源一览

### 论文 / 会议
1. Choi, Zhu, Li. *Debate or Vote.* arXiv 2508.17536v2, 2025-10-23. NeurIPS 2025 Spotlight. <https://arxiv.org/abs/2508.17536>
2. Bertalanič, Fortuna. *The Cost of Consensus.* arXiv 2605.00914v1, 2026-04-29. ACM CAIS. DOI 10.1145/3786335.3813137. <https://arxiv.org/abs/2605.00914>
3. *The Deliberative Illusion.* arXiv 2606.03032, 2026-06-02. <https://arxiv.org/abs/2606.03032>
4. *If Multi-Agent Debate is the Answer, What is the Question? / Stop Overvaluing Multi-Agent Debate.* arXiv 2502.08788（v1 2025-02-12；v3 2025-06-21）. <https://arxiv.org/abs/2502.08788>
5. *Rethinking the Bounds of LLM Reasoning: Are Multi-Agent Discussions the Key?* arXiv 2402.18272, 2024-02-28. <https://arxiv.org/abs/2402.18272>
6. *Debate can decrease accuracy over time.* arXiv 2509.05396（v2 2025-10-13）. <https://arxiv.org/abs/2509.05396>
7. *Inter-agent sycophancy / disagreement collapse.* arXiv 2509.23055, 2025-09-27. <https://arxiv.org/abs/2509.23055>
8. Estornell, Liu. *Multi-agent debate converges to the majority opinion.* NeurIPS 2024. <https://proceedings.neurips.cc/paper_files/paper/2024/hash/32e07a110c6c6acf1afbf2bf82b614ad-Abstract-Conference.html>
9. Smit, Duckworth, Grinsztajn, Barrett, Pretorius. *Should we be going MAD?* arXiv 2311.17371v3, 2024-07-18. <https://arxiv.org/abs/2311.17371> · <https://arxiv.org/html/2311.17371v3>
10. Weng, Chen, Wang. *Do as We Do, Not as You Think.* arXiv 2501.13381v2, 2025-02-11. ICLR 2025 Oral. <https://arxiv.org/abs/2501.13381> · <https://arxiv.org/html/2501.13381v2> · <https://github.com/Zhiyuan-Weng/BenchForm>
11. Zhong et al. *Spiral of Silence in Large Language Model Agents.* Findings of EMNLP 2025, 23238–23253. <https://aclanthology.org/2025.findings-emnlp.1262/> · arXiv 2510.02360
12. Cho, Guntuku, Ungar. *Herd Behavior: Investigating Peer Influence in LLM-based Multi-Agent Systems.* arXiv 2505.21588, 2025-05-27. <https://arxiv.org/abs/2505.21588>
13. Yashwanth YS. *Everyone Conforms, No One Believes.* arXiv 2608.02758v1, 2026-08-03. <https://arxiv.org/abs/2608.02758>
14. Zhu et al. *Demystifying Multi-Agent Debate.* arXiv 2601.19921v3, 2026-06-03. <https://arxiv.org/abs/2601.19921> · <https://arxiv.org/html/2601.19921v3>
15. Helmi. *Modeling Response Consistency in Multi-Agent LLM Systems.* arXiv 2504.07303v1, 2025-04-09. <https://arxiv.org/abs/2504.07303>
16. Du et al. *Improving Factuality and Reasoning in Language Models through Multiagent Debate.* arXiv 2305.14325, 2023-05-23. <https://arxiv.org/abs/2305.14325>
17. Liang et al. *Encouraging Divergent Thinking through Multi-Agent Debate.* arXiv 2305.19118v4, 2024-10-09. EMNLP 2024. <https://arxiv.org/abs/2305.19118>
18. Chan et al. *ChatEval.* arXiv 2308.07201, 2023-08-14. ICLR 2024. <https://arxiv.org/abs/2308.07201>
19. Li et al. *More Agents Is All You Need.* arXiv 2402.05120v2, 2024-10-11. TMLR. <https://arxiv.org/abs/2402.05120>
20. Chen, Saha, Bansal. *ReConcile.* arXiv 2309.13007v3, 2024-06-21. ACL 2024. <https://arxiv.org/abs/2309.13007>
21. Wang et al. *Self-Consistency.* arXiv 2203.11171v4, 2023-03-07. ICLR 2023. <https://arxiv.org/abs/2203.11171>
22. Jiang, Ren, Lin. *LLM-Blender.* arXiv 2306.02561, 2023. ACL 2023. <https://arxiv.org/abs/2306.02561>
23. Wang et al. *Mixture-of-Agents.* arXiv 2406.04692, 2024-06-07. <https://arxiv.org/abs/2406.04692>
24. Wu, Li, Li. *Can LLM Agents Really Debate?* arXiv 2511.07784v1, 2025-11-11. <https://arxiv.org/abs/2511.07784>
25. Sharma et al. *Towards Understanding Sycophancy in Language Models.* arXiv 2310.13548v4, 2025-05-10. <https://arxiv.org/abs/2310.13548>
26. Wang et al. *When Truth Is Overridden.* arXiv 2508.02087v4, 2025-11-12. <https://arxiv.org/abs/2508.02087>
27. Qu, Fu, Hu. *Easier to Mislead Than to Correct.* arXiv 2606.01637, 2026-06-01. <https://arxiv.org/abs/2606.01637>
28. Joswin, Medicherla, Mammen. *A Mechanistic View of Authority Hierarchy in LLM Sycophancy.* arXiv 2607.00415v1, 2026-07-01. ICML 2026. <https://arxiv.org/abs/2607.00415>
29. *Who Endorsed It? Measuring Authority Bias Across Expertise Levels.* arXiv 2601.13433, 2026-01-19. <https://arxiv.org/abs/2601.13433>
30. Lou, Sun. *Anchoring Bias in Large Language Models.* arXiv 2412.06593, 2024-12-09. <https://arxiv.org/abs/2412.06593>
31. Zhang et al. *Exploring Collaboration Mechanisms for LLM Agents: A Social Psychology View.* arXiv 2310.02124v3, 2024-05-27. ACL 2024. <https://arxiv.org/abs/2310.02124>
32. Cemri et al. *Why Do Multi-Agent LLM Systems Fail? (MAST).* arXiv 2503.13657v3, 2025-10-26. NeurIPS 2025. <https://arxiv.org/abs/2503.13657>
33. Zheng et al. *Judging LLM-as-a-Judge.* arXiv 2306.05685v4, 2023-12-24. NeurIPS 2023 D&B. <https://arxiv.org/abs/2306.05685>
34. Borchers et al. arXiv 2507.11198. <https://arxiv.org/abs/2507.11198>
35. Nguyen et al. *Does Debate Suppress Diversity?* arXiv 2609.00683. <https://arxiv.org/abs/2609.00683>
36. Yang et al. arXiv 2602.03794, 2026-02-03. <https://arxiv.org/abs/2602.03794>
37. *Selective Agreement in LLM Debates*（🟡 摘要，全文 403）. <https://research.hva.nl/en/publications/selective-agreement-in-llm-debates-anchoring-effects-and-resistan/>
38. *Pokharel, Dantu. Hidden Anchors in Multi-Agent LLM Deliberation.* arXiv 2606.19494, 2026-06-17. <https://arxiv.org/abs/2606.19494>
39. AutoGen 论文 arXiv 2308.08155；MetaGPT arXiv 2308.00352；AgentVerse arXiv 2308.10848；Reflexion arXiv 2303.11366（均为框架主张，非对照实验）

### 框架源码 / 文档
40. AutoGen `autogen_core.models._types`：<https://microsoft.github.io/autogen/stable/_modules/autogen_core/models/_types.html>
41. AutoGen `autogen_ext.models.openai._openai_client`：<https://microsoft.github.io/autogen/stable/_modules/autogen_ext/models/openai/_openai_client.html>
42. AutoGen `_transformation/registry.py`：<https://raw.githubusercontent.com/microsoft/autogen/main/python/packages/autogen-ext/src/autogen_ext/models/openai/_transformation/registry.py>
43. AutoGen 0.2 `conversable_agent.py` @ `c631b34`：<https://github.com/microsoft/autogen/blob/c631b34e212f5fe2fc74218d60470e83c805fd07/autogen/agentchat/conversable_agent.py>
44. AutoGen 0.2 `groupchat.py` @ `c631b34`：<https://github.com/microsoft/autogen/blob/c631b34e212f5fe2fc74218d60470e83c805fd07/autogen/agentchat/groupchat.py>
45. AutoGen main `_message_transform.py` @ `027ecf0`：<https://github.com/microsoft/autogen/blob/027ecf0a379bcc1d09956d46d12d44a3ad9cee14/python/packages/autogen-ext/src/autogen_ext/models/openai/_message_transform.py>
46. AutoGen main `autogen_agentchat/messages.py` @ `027ecf0`：<https://github.com/microsoft/autogen/blob/027ecf0a379bcc1d09956d46d12d44a3ad9cee14/python/packages/autogen-agentchat/src/autogen_agentchat/messages.py>
47. AutoGen issues/PR（🟡 标题级）：#3333 <https://github.com/microsoft/autogen/issues/3333> · #2457 <https://github.com/microsoft/autogen/issues/2457> · PR #6845 <https://github.com/microsoft/autogen/pull/6845>
48. LangChain PR #32469（🟡 标题级）：<https://github.com/langchain-ai/langchain/pull/32469>
49. LangChain JS `BaseMessage.name`（正文未渲染）：<https://reference.langchain.com/javascript/langchain-core/messages/BaseMessage/name>
50. CrewAI `crew.py`：<https://github.com/crewAIInc/crewAI/blob/main/src/crewai/crew.py>

### Provider 官方 API 文档
51. Anthropic Messages API：<https://platform.claude.com/docs/en/api/messages> · <https://platform.claude.com/docs/en/api/messages.md>
52. Anthropic errors（prefill、temperature 弃用）：<https://platform.claude.com/docs/en/api/errors> · <https://platform.claude.com/docs/en/api/errors.md>
53. OpenAI Python SDK message param（`name`）：<https://raw.githubusercontent.com/openai/openai-python/main/src/openai/types/chat/chat_completion_user_message_param.py>
54. OpenAI Multi-agent guide：<https://developers.openai.com/api/docs/guides/responses-multi-agent> · <https://developers.openai.com/api/docs/guides/responses-multi-agent.md>
55. OpenAI Agents SDK orchestration：<https://developers.openai.com/api/docs/guides/agents/orchestration>
56. Moonshot/Kimi Chat Completions：<https://platform.kimi.ai/docs/api/chat> · <https://platform.kimi.ai/docs/api/chat.md> · Messages API <https://platform.kimi.ai/docs/api/messages>
57. DeepSeek Chat Completions：<https://api-docs.deepseek.com/api/create-chat-completion>
58. DeepSeek Chat Prefix Completion (Beta)：<https://api-docs.deepseek.com/guides/chat_prefix_completion/>
59. openai-node issue #508（🟡 标题级）：<https://github.com/openai/openai-node/issues/508>
60. anthropic-sdk-typescript issue #565（🟡 标题级）：<https://github.com/anthropics/anthropic-sdk-typescript/issues/565>

### 一手工程博客
61. Anthropic, *How we built our multi-agent research system*, 2025-06-13：<https://www.anthropic.com/engineering/multi-agent-research-system>
62. Anthropic, *When to use multi-agent systems (and when not to)*, 2026-01-23：<https://claude.com/blog/building-multi-agent-systems-when-and-how-to-use-them>
63. Anthropic, *Building effective agents*, 2024-12-19：<https://www.anthropic.com/engineering/building-effective-agents>
