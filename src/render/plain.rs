//! plain 渲染器：给管道或简单终端看的人类转录。
//!
//! 它把共享的 [`Block`]（见 [`super::transcript`]）画成文本，并补上一条实时转录需要、
//! 而一次查完成的流不需要的两样东西：**每一**行都重复的发言前缀，好让交错的两个讨论者
//! 仍然读得下去；以及每一个终止点上的严重度颜色，好让 `Completed` 永不看起来像
//! `Aborted` 或 `Error`（spec §19）。
//!
//! 最终产物仍然去 `stdout`，叙述去 `stderr`，与 headless 模式用的是同一种分工：
//! `heng --plain "q" 2>/dev/null` 只印出那个答案，别的什么都不印，而在终端前的人
//! 看得见整场讨论。

use std::io::Write;

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::events::{Role, SpeakerId};

use super::severity::Severity;
use super::transcript::{Block, ToolBlock, Transcript, summarize_args};
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent, RenderSinks};

/// 一条工具结果在省略之前，plain 转录显示多少。
const TOOL_PREVIEW: usize = 2_000;

/// plain 渲染器的那些值。`color` 由前端决定，不在这里从环境里读：库永远不去猜终端。
pub struct PlainOptions {
    pub sinks: RenderSinks,
    /// 是否用 ANSI 转义来画严重度与推理。
    pub color: bool,
}

/// plain 渲染器。
pub struct Plain {
    sinks: RenderSinks,
    color: bool,
    transcript: Transcript,
    /// 正文流敞着的那位发言者，这样发言者一换就起一行。
    open_speaker: Option<SpeakerId>,
    /// 下一个字符之前欠不欠一个新前缀。
    at_line_start: bool,
    /// 推理标记敞不敞着。
    in_reasoning: bool,
    /// 增量是不是已经把当前消息的正文产出来了。
    streamed: bool,
}

impl Plain {
    pub fn new(options: PlainOptions) -> Self {
        Self {
            sinks: options.sinks,
            color: options.color,
            transcript: Transcript::new(),
            open_speaker: None,
            at_line_start: true,
            in_reasoning: false,
            streamed: false,
        }
    }

    fn emit(&mut self, event: RenderEvent) {
        let blocks = self.transcript.push(event);
        for block in blocks {
            self.paint(block);
        }
    }

    /// 把一个块画进诊断写出口（最终产物是唯一也到 stdout 的东西）。
    fn paint(&mut self, block: Block) {
        match block {
            Block::Delta {
                speaker,
                kind,
                text,
            } => self.delta(speaker, kind, text),
            Block::Message {
                speaker,
                role,
                text,
                // plain 不显示推理：它可见的输出是定稿的（事件流只追加），而一条思考行是 TUI
                // 的一种便利，不是日志的一部分（票 02 §5）。这个字段是为编译器命名的，不是
                // 为页面命名的。
                reasoning: _,
            } => self.message(speaker, role, &text),
            Block::RoundStarted { round, mode } => {
                self.end_line();
                self.line(&format!("\n{}", wording::round_section(round, mode)));
            }
            Block::RoundEnded { round, reason } => {
                let text = wording::round_ended(round, reason);
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::Divergence { topic, positions } => {
                self.end_line();
                self.line(&format!("!! {}", wording::divergence(&topic)));
                for position in positions {
                    self.line(&format!("  - {position}"));
                }
            }
            Block::Tool(tool) => self.tool(&tool),
            Block::ToolFeedback { outcome, .. } => {
                self.line(&indent(&wording::hook_feedback(&outcome), 2));
            }
            Block::TurnStarted { speaker, iteration } => {
                self.end_line();
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::turn_started(iteration)
                ));
            }
            Block::TurnEnded { speaker, reason } => {
                self.end_line();
                let text = format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::turn_ended(reason)
                );
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::PermissionAsked {
                speaker,
                tool_name,
                args,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::permission_asked(tool_name.as_deref(), &summarize_args(&args))
                ));
            }
            Block::PermissionDecided {
                speaker,
                decision,
                source,
                reason,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::permission_decided(decision, source, reason.as_deref())
                ));
            }
            Block::Hook {
                speaker,
                point,
                outcome,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::hook(&point, &outcome)
                ));
            }
            Block::ExecutorSpawned {
                speaker,
                executor_id,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::executor_spawned(executor_id.as_str())
                ));
            }
            Block::ExecutorFinished {
                executor_id,
                reason,
                summary,
            } => {
                let text = wording::executor_finished(executor_id.as_str(), reason, &summary);
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::Usage { speaker, usage } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::usage_summary(&usage)
                ));
            }
            Block::AgentError { speaker, message } => {
                let text = format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::agent_error(&message)
                );
                self.line(&self.paint_severity(Severity::Bad, &text));
            }
            Block::SessionError { code, detail } => {
                let text = wording::session_error(&code, &detail);
                self.line(&self.paint_severity(Severity::Bad, &text));
            }
            Block::SessionEnded { reason } => {
                let text = wording::session_ended(reason);
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::ContextInjected { source, .. } => {
                self.line(&wording::context_injected(source));
            }
            Block::Sandbox {
                mode,
                unavailable_reason,
            } => {
                self.line(&wording::sandbox(&mode, unavailable_reason.as_deref()));
            }
            Block::History { reason, summary } => {
                self.line(&wording::history(reason, summary.as_deref()));
            }
            Block::Diagnostic(message) => {
                self.line(&wording::diagnostic(&message));
            }
            Block::Notice(message) => {
                self.line(&message);
            }
        }
        self.flush_sink();
    }

    /// 流式输出增量文本，每一行都重复发言前缀。
    fn delta(&mut self, speaker: SpeakerId, kind: DeltaKind, text: String) {
        match kind {
            DeltaKind::Text => {
                self.close_reasoning();
                self.stream(&speaker, &text);
                self.streamed = true;
            }
            DeltaKind::Reasoning => {
                if !self.in_reasoning {
                    self.end_line();
                    let prefix = speaker_label(&speaker);
                    let marker =
                        self.paint_reasoning(&format!("{prefix} {} ", wording::reasoning_label()));
                    self.write(&marker);
                    self.open_speaker = Some(speaker.clone());
                    self.at_line_start = false;
                    self.in_reasoning = true;
                }
                self.write(&text);
                self.flush_sink();
            }
        }
    }

    fn message(&mut self, speaker: SpeakerId, role: Role, text: &str) {
        self.close_reasoning();
        // 一个没有流式输出的供应商（或者一条整份拼起来的合成器消息）还是得显示：只有在
        // 没有任何增量印过时才印正文。
        if !self.streamed && !text.is_empty() {
            self.stream(&speaker, text);
        }
        self.streamed = false;
        self.end_line();
        if speaker == SpeakerId::System && role == Role::Assistant {
            self.final_product(text);
        }
    }

    fn tool(&mut self, tool: &ToolBlock) {
        self.end_line();
        let head = format!(
            "{} → {}",
            speaker_label(&tool.speaker),
            wording::tool_call(&tool.tool, &summarize_args(&tool.args))
        );
        self.line(&head);
        match &tool.outcome {
            Some(outcome) if outcome.ok => {
                if let Some(output) = &outcome.output {
                    if !output.trim().is_empty() {
                        self.line(&indent(
                            &wording::tool_output_preview(output, TOOL_PREVIEW),
                            2,
                        ));
                    }
                }
            }
            Some(outcome) => {
                let error = outcome
                    .error
                    .as_deref()
                    .unwrap_or_else(|| wording::no_message())
                    .to_owned();
                let text = indent(&wording::tool_output_preview(&error, TOOL_PREVIEW), 2);
                self.line(&self.paint_severity(Severity::Bad, &text));
            }
            None => self.line(&format!("  {}", wording::no_tool_result())),
        }
    }

    /// 写 `text`，每一行开头都带一个全新的 `[speaker]` 前缀。
    fn stream(&mut self, speaker: &SpeakerId, text: &str) {
        if self.open_speaker.as_ref() != Some(speaker) && !self.at_line_start {
            self.write("\n");
            self.at_line_start = true;
        }
        self.open_speaker = Some(speaker.clone());
        for ch in text.chars() {
            if self.at_line_start {
                self.at_line_start = false;
                if ch != '\n' {
                    let prefix = format!("{} ", speaker_label(speaker));
                    self.write(&prefix);
                }
            }
            if ch == '\n' {
                self.at_line_start = true;
            }
            self.write(&ch.to_string());
        }
    }

    fn close_reasoning(&mut self) {
        if self.in_reasoning {
            self.write("\n");
            self.in_reasoning = false;
            self.at_line_start = true;
        }
    }

    /// 把敞着的那一行收尾，如果有的话。
    fn end_line(&mut self) {
        self.close_reasoning();
        if !self.at_line_start {
            self.write("\n");
            self.at_line_start = true;
        }
    }

    /// 一整行叙述，先把当前那一行收尾。
    fn line(&mut self, text: &str) {
        self.end_line();
        self.write(text);
        self.write("\n");
        self.at_line_start = true;
    }

    /// 会话的最终产物也去 stdout，与 headless 模式一样。
    fn final_product(&mut self, text: &str) {
        let _ = self.sinks.stdout_result.write_all(text.as_bytes());
        let _ = self.sinks.stdout_result.write_all(b"\n");
        let _ = self.sinks.stdout_result.flush();
    }

    fn write(&mut self, text: &str) {
        let _ = self.sinks.stderr_diagnostic.write_all(text.as_bytes());
    }

    fn flush_sink(&mut self) {
        let _ = self.sinks.stderr_diagnostic.flush();
    }

    fn paint_severity(&self, severity: Severity, text: &str) -> String {
        if !self.color {
            return text.to_owned();
        }
        format!("{}{}{}", severity.ansi(), text, Severity::ANSI_RESET)
    }

    fn paint_reasoning(&self, text: &str) -> String {
        if !self.color {
            return text.to_owned();
        }
        format!("\x1b[2m{text}\x1b[0m")
    }
}

#[async_trait]
impl Render for Plain {
    async fn consume(self: Box<Self>, mut receiver: broadcast::Receiver<RenderEvent>) {
        let mut plain = *self;
        loop {
            match receiver.recv().await {
                Ok(event) => plain.emit(event),
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    plain.emit(RenderEvent::diagnostic(wording::renderer_dropped(dropped)));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        // 一条结果始终没到的调用（一次取消）仍然显示出来。
        for block in plain.transcript.flush() {
            plain.paint(block);
        }
        plain.end_line();
        let _ = plain.sinks.stderr_diagnostic.flush();
        let _ = plain.sinks.stdout_result.flush();
    }
}

/// 给 `text` 的每一行加缩进。
fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
