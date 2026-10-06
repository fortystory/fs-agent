use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use heng::events::Usage;
use heng::provider::capability::{caps_for, ModelCaps};
use heng::provider::{
    ChatRequest, EventStream, FinishReason, Provider, ProviderError, StreamEvent,
};
use tokio::sync::{Barrier, Notify};

/// 一次会合等多久才判定两次调用并不并发。长到在
/// 一台负载高的机器上永远不会误触，短到串行化的回退会失败
/// 而不是把整个测试套件挂住。
const RENDEZVOUS_TIMEOUT: Duration = Duration::from_secs(5);

/// 一次 `Provider::send` 调用的一条脚本化响应。
#[derive(Clone)]
pub enum Reply {
    /// 按顺序发出这些事件（末尾没有 `Finished` 就补一条）。
    Stream(Vec<StreamEvent>),
    /// 发出原始条目，包括流中途的错误。
    Raw(Vec<Result<StreamEvent, ProviderError>>),
    /// 在产出任何流之前失败。
    Fail(ProviderError),
    /// 先在这个屏障上等，再产出里层的响应。
    ///
    /// 为一次脚本化的*调用*准备的会合工具：它只拦那些必须会合的调用，
    /// 所以同一份脚本里派发者自己的那些回合不用等。
    Meet(Arc<Barrier>, Box<Reply>),
    /// 发出这些事件，然后永不结束。
    ///
    /// 为一次取消手势必须打断的流准备的仪器：这个流永远到不了
    /// `[DONE]`，所以只有取消能收尾这个回合。`opened` 在这条流交出去时
    /// 被 notify，于是测试能在一个确定的时刻取消，
    /// 而不是靠睡一会儿。
    Stall(Arc<Notify>, Vec<StreamEvent>),
}

impl Reply {
    /// 一条文本增量、零用量，然后 `Finished { Stop }`。
    pub fn text(text: &str) -> Self {
        Reply::Stream(vec![
            StreamEvent::TextDelta(text.to_owned()),
            StreamEvent::Usage(Usage::default()),
            StreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            },
        ])
    }

    /// 同一段文本切成几条增量，用来证明流式透传是保住的。
    pub fn chunks(chunks: &[&str]) -> Self {
        let mut events: Vec<StreamEvent> = chunks
            .iter()
            .map(|chunk| StreamEvent::TextDelta((*chunk).to_owned()))
            .collect();
        events.push(StreamEvent::Finished {
            finish_reason: FinishReason::Stop,
        });
        Reply::Stream(events)
    }
}

/// 按调用顺序照脚本作答、并记下递给它的每一个请求
/// 的 provider。
///
/// 克隆共享同一份脚本与请求日志，于是测试可以自己留一个把手做断言，
/// 而 harness 手里握着注入的那一份。
#[derive(Clone)]
pub struct FakeProvider {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<ChatRequest>>,
    caps: ModelCaps,
    /// 设上之后，`send` 在各方都到齐之前拒绝产出流：
    /// 用来证明「这两次调用真的并发」的仪器。
    rendezvous: Option<Arc<Barrier>>,
}

impl FakeProvider {
    pub fn new(replies: Vec<Reply>) -> Self {
        Self::with_caps(replies, caps_for("deepseek-flash").expect("内置模型"))
    }

    /// 绑在明确写出的能力事实上的假 provider，于是测试能驱动
    /// 投影里那些由能力决定的分支。
    pub fn with_caps(replies: Vec<Reply>, caps: ModelCaps) -> Self {
        Self::build(replies, caps, None)
    }

    /// 在 `barrier` 的每一方都调用过 `send` 之前
    /// 不作答的假 provider。
    ///
    /// 测试就是这样证明两个回合同时进行中的：一个把它们一个接一个跑的循环
    /// 会卡在第一次 `send` 上、撞上会合超时，
    /// 而不是安安静静地产出一条流。
    pub fn meeting_at(replies: Vec<Reply>, barrier: Arc<Barrier>) -> Self {
        Self::build(
            replies,
            caps_for("deepseek-flash").expect("内置模型"),
            Some(barrier),
        )
    }

    fn build(replies: Vec<Reply>, caps: ModelCaps, rendezvous: Option<Arc<Barrier>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                replies: Mutex::new(replies.into()),
                requests: Mutex::new(Vec::new()),
                caps,
                rendezvous,
            }),
        }
    }

    pub fn requests(&self) -> Vec<ChatRequest> {
        self.inner
            .requests
            .lock()
            .expect("假 provider 已中毒")
            .clone()
    }
}

#[async_trait]
impl Provider for FakeProvider {
    fn caps(&self) -> ModelCaps {
        self.inner.caps
    }

    async fn send(&self, request: ChatRequest) -> Result<EventStream, ProviderError> {
        if let Some(barrier) = &self.inner.rendezvous {
            if tokio::time::timeout(RENDEZVOUS_TIMEOUT, barrier.wait())
                .await
                .is_err()
            {
                panic!(
                    "FakeProvider: {RENDEZVOUS_TIMEOUT:?} 之内没有别的调用到达，\
                     所以这次调用与别的调用并不并发"
                );
            }
        }
        self.inner
            .requests
            .lock()
            .expect("假 provider 已中毒")
            .push(request);

        let reply = self
            .inner
            .replies
            .lock()
            .expect("假 provider 已中毒")
            .pop_front()
            .expect("FakeProvider: 这次调用已经没有脚本化响应了");

        // 脚本要会合时的会合：一次永远没等到同伴的调用就是一次
        // 并不并发的调用，它大声失败，
        // 而不是把整个测试套件挂住。
        let mut reply = reply;
        let reply = loop {
            match reply {
                Reply::Meet(barrier, inner) => {
                    if tokio::time::timeout(RENDEZVOUS_TIMEOUT, barrier.wait())
                        .await
                        .is_err()
                    {
                        panic!(
                            "FakeProvider: 在 {RENDEZVOUS_TIMEOUT:?} 之内没有别的调用到达这个屏障，\
                             所以这次调用与别的调用并不并发"
                        );
                    }
                    reply = *inner;
                }
                other => break other,
            }
        };

        match reply {
            Reply::Fail(error) => Err(error),
            Reply::Raw(items) => Ok(Box::pin(stream::iter(items))),
            Reply::Stream(mut events) => {
                if !matches!(events.last(), Some(StreamEvent::Finished { .. })) {
                    events.push(StreamEvent::Finished {
                        finish_reason: FinishReason::Stop,
                    });
                }
                Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
            }
            Reply::Stall(opened, events) => {
                // 在循环有机会 poll 之前先打声招呼：测试醒来时发的
                // 那个手势，仍然落在这一条流进行的时候。
                opened.notify_one();
                let head = stream::iter(events.into_iter().map(Ok));
                Ok(Box::pin(head.chain(stream::pending())))
            }
            // 上面那圈会合循环已经把它拆开了。
            Reply::Meet(..) => unreachable!("a rendezvous reply is unwrapped before use"),
        }
    }
}
