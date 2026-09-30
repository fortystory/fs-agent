//! 两个不属于回合循环的流操作：把一个被杀死的进程留下的调用收尾，以及 `/undo`。
//!
//! 两者都是历史编辑，也都因为这个层是事件流的唯一写者而住在 `agent` 层 —— 与其它每一处
//! 写入同一个理由。恢复是**补完**崩溃留下的一段开口；`/undo` 是**注销**一段并还原它改过的
//! 字节（spec §11）。
//!
//! 两者都不是流上的手势：一次续上由一个未完成的调用自己暗示，而一次 undo 唯一的痕迹是那
//! 条记录它干了什么的 `HistorySuperseded`（spec §6、§11）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;

use super::{emit, emit_completed, Error};
use crate::events::{
    pending_tool_calls, superseded_seqs, Event, EventPayload, HistoryReason, SpeakerId, ToolCallId,
};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::tools::edit;
use crate::tools::{before_artifact, EditCall, ToolError, EDIT_FILE};

/// 被杀死的进程始终没来得及写下的那一个结果。
///
/// 它说的是「未知」，不是「失败」：调用有可能在这个进程死掉之前已经生效了，所以模型不能
/// 假定工作区没被动过 —— 也不能抱着「多半没生效」的侥幸把它重跑一遍（spec §11）。
const INTERRUPTED: &str =
    "这条调用在飞时会话被中断了，所以它的结果未知。它没有被重跑；在依赖任何一种结果之前先检查工作区。";

/// 把一个被中断的进程留下的、没有结果的 `tool_call` 逐个收尾。
///
/// 查询是会话级的（用 `pending_tool_calls`，而不是逐发言者的那种循环形式）：整个进程都死
/// 了，所以每个 agent 没做完的调用就是本会话没做完的调用（spec §11，Further Notes）。
/// 返回合成了多少个结果。
///
/// 它只在组装期、任何回合开跑之前调用一次 —— 这同时也是循环的不变量 2 能跨一次续上依然
/// 成立的原因。
pub fn recover_pending_calls(session: &mut Session, render: &RenderHandle) -> Result<usize, Error> {
    let events = session.events();
    let mut recovered = 0;
    for tool_call_id in pending_tool_calls(&events) {
        // 结果必须归属到发起这次调用的 agent，否则投影没法把它和那条还攥着
        // `tool_call` 的助手消息配成一对。
        let Some(speaker) = started_by(&events, &tool_call_id) else {
            continue;
        };
        emit_completed(
            session,
            render,
            &speaker,
            tool_call_id,
            Err(ToolError::message(INTERRUPTED)),
            Instant::now(),
        )?;
        recovered += 1;
    }
    Ok(recovered)
}

/// 谁发起了这次调用：从流上读，而不是猜。
fn started_by(events: &[Event], tool_call_id: &ToolCallId) -> Option<SpeakerId> {
    events.iter().find_map(|event| match &event.payload {
        EventPayload::ToolCallStarted {
            tool_call_id: id, ..
        } if id == tool_call_id => Some(event.speaker_id.clone()),
        _ => None,
    })
}

/// 一次 `/undo` 可以回滚的编辑。
#[derive(Debug, Clone, PartialEq)]
struct UndoableEdit {
    tool_call_id: ToolCallId,
    /// undo 要注销的那两个 seq（`ToolCallStarted` 与 `ToolCallCompleted`），
    /// 投影从此不再回放这次编辑。
    started_seq: u64,
    completed_seq: u64,
    /// 模型当时发来的参数，与流上记录的一模一样。
    args: Value,
}

/// 一次 `/undo` 干了什么，供前端叙述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    pub tool_call_id: ToolCallId,
    pub path: PathBuf,
}

/// 下一次 `/undo` 会回滚的那次编辑，没有时是 `None`。
///
/// 一条纯查询：流上最近一次成功、且尚未被注销的 `edit_file` 调用（注销可能来自更早的一次
/// undo，也可能来自任何别的取代历史的东西）。倒着走正是连按 undo 能一次一步地退回会话里
/// 各次编辑的原因。
fn last_undoable_edit(events: &[Event]) -> Option<UndoableEdit> {
    let superseded = superseded_seqs(events);
    let mut open: HashMap<ToolCallId, (u64, String, Value)> = HashMap::new();
    let mut last = None;
    for event in events {
        if superseded.contains(&event.seq) {
            continue;
        }
        match &event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                open.insert(
                    tool_call_id.clone(),
                    (event.seq, tool_name.clone(), args.clone()),
                );
            }
            EventPayload::ToolCallCompleted {
                tool_call_id, ok, ..
            } => {
                if let Some((started_seq, tool_name, args)) = open.remove(tool_call_id) {
                    if *ok && tool_name == EDIT_FILE {
                        last = Some(UndoableEdit {
                            tool_call_id: tool_call_id.clone(),
                            started_seq,
                            completed_seq: event.seq,
                            args,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    last
}

/// 回滚最近一次编辑：还原它的字节，并注销它的事件。
///
/// 还原的来源是 `outputs/<tool_call_id>.before` —— **真正被替换掉的那些字节**，所以一次
/// 落在降级匹配上的编辑也能精确还原（spec §11）。这里锁的路径锁与那次编辑自己拿的是同一
/// 把，所以 undo 不会和另一个写者交错，而用户的 git 从头到尾没被碰过。
///
/// 没有可 undo 的编辑时返回 `Ok(None)`。其它任何失败都是错误，且不动工作区：陈旧的快照会
/// 被拒绝，而不是被猜着用。
pub async fn undo_last_edit(
    session: &mut Session,
    render: &RenderHandle,
) -> Result<Option<UndoOutcome>, Error> {
    let events = session.events();
    let Some(edit) = last_undoable_edit(&events) else {
        return Ok(None);
    };

    let parsed: EditCall = serde_json::from_value(edit.args.clone())
        .map_err(|error| Error::Undo(format!("记下的 edit_file 参数读不回来：{error}")))?;
    let path = session
        .paths()
        .resolve(&PathBuf::from(&parsed.file_path))
        .map_err(|error| Error::Undo(error.to_string()))?;

    let before = std::fs::read_to_string(
        session
            .outputs_dir()
            .join(before_artifact(edit.tool_call_id.as_str())),
    )
    .map_err(|error| Error::Undo(format!("读不了 {} 的快照：{error}", edit.tool_call_id)))?;

    // 与那次编辑自己拿的同一把锁（spec §11）：undo 也是这条路径的写者，
    // 所以它排在其它每一个写者后面。
    let _guard = session.path_locks().lock(&path).await;

    let content = std::fs::read_to_string(&path)
        .map_err(|error| Error::Undo(format!("读不了 {}：{error}", path.display())))?;
    let restored = edit::revert(
        &content,
        &before,
        &parsed.old_string,
        &parsed.new_string,
        parsed.replace_all,
    )
    .map_err(|error| Error::Undo(format!("撤销不了对 {} 的这次编辑：{error}", path.display())))?;
    std::fs::write(&path, restored.as_bytes())
        .map_err(|error| Error::Undo(format!("写不回去 {}：{error}", path.display())))?;

    // 手势本身从不进流；它的效果进。
    emit(
        session,
        render,
        &SpeakerId::User,
        EventPayload::HistorySuperseded {
            targets: vec![edit.started_seq, edit.completed_seq],
            reason: HistoryReason::Undo,
            summary: Some(format!("撤销对 {} 的这次 edit_file", path.display())),
        },
    )?;

    Ok(Some(UndoOutcome {
        tool_call_id: edit.tool_call_id,
        path,
    }))
}
