use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fs_agent::events::Usage;
use fs_agent::provider::capability::{caps_for, ModelCaps};
use fs_agent::provider::{
    ChatRequest, EventStream, FinishReason, Provider, ProviderError, StreamEvent,
};
use futures::{stream, StreamExt};
use tokio::sync::{Barrier, Notify};

/// How long a rendezvous waits before declaring that the two calls were not
/// concurrent. Long enough never to fire on a loaded machine, short enough that a
/// sequential regression fails instead of hanging the suite.
const RENDEZVOUS_TIMEOUT: Duration = Duration::from_secs(5);

/// One scripted response for one `Provider::send` call.
#[derive(Clone)]
pub enum Reply {
    /// Emit these events in order (appending a terminal `Finished` if absent).
    Stream(Vec<StreamEvent>),
    /// Emit raw items, including mid-stream errors.
    Raw(Vec<Result<StreamEvent, ProviderError>>),
    /// Fail before any stream is produced.
    Fail(ProviderError),
    /// Wait at this barrier before producing the inner reply.
    ///
    /// The rendezvous instrument for a scripted *call*: it gates only the calls
    /// that must meet, so a parent's own turns in the same script do not wait.
    Meet(Arc<Barrier>, Box<Reply>),
    /// Emit these events, then never end.
    ///
    /// The instrument for a stream a cancel gesture has to interrupt: the stream
    /// never reaches `[DONE]`, so only a cancellation can end the turn. `opened`
    /// is notified as the stream is handed over, so a test can cancel at a
    /// defined moment instead of sleeping.
    Stall(Arc<Notify>, Vec<StreamEvent>),
}

impl Reply {
    /// A single text delta, zero usage, then `Finished { Stop }`.
    pub fn text(text: &str) -> Self {
        Reply::Stream(vec![
            StreamEvent::TextDelta(text.to_owned()),
            StreamEvent::Usage(Usage::default()),
            StreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            },
        ])
    }

    /// The same text split into several deltas, to prove streaming is preserved.
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

/// A provider that answers by call order from a script and records every
/// request it was handed.
///
/// Cloning shares the same script and request log, so a test can keep a handle
/// for assertions while the harness owns an injected copy.
#[derive(Clone)]
pub struct FakeProvider {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<ChatRequest>>,
    caps: ModelCaps,
    /// When set, `send` refuses to produce a stream until every party has
    /// arrived: the instrument for "these two calls really are concurrent".
    rendezvous: Option<Arc<Barrier>>,
}

impl FakeProvider {
    pub fn new(replies: Vec<Reply>) -> Self {
        Self::with_caps(replies, caps_for("deepseek-flash").expect("built-in model"))
    }

    /// A fake bound to explicit capability facts, so a test can drive the
    /// capability-driven branches of the projection.
    pub fn with_caps(replies: Vec<Reply>, caps: ModelCaps) -> Self {
        Self::build(replies, caps, None)
    }

    /// A fake that will not answer until every party of `barrier` has called
    /// `send`.
    ///
    /// This is how a test proves two turns were in flight at once: a loop that
    /// ran them one after another would block on the first `send` and trip the
    /// rendezvous timeout instead of quietly producing a stream.
    pub fn meeting_at(replies: Vec<Reply>, barrier: Arc<Barrier>) -> Self {
        Self::build(
            replies,
            caps_for("deepseek-flash").expect("built-in model"),
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
            .expect("fake provider poisoned")
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
                    "FakeProvider: no other call arrived within {RENDEZVOUS_TIMEOUT:?}, \
                     so this call was not concurrent with another"
                );
            }
        }
        self.inner
            .requests
            .lock()
            .expect("fake provider poisoned")
            .push(request);

        let reply = self
            .inner
            .replies
            .lock()
            .expect("fake provider poisoned")
            .pop_front()
            .expect("FakeProvider: no scripted reply left for this call");

        // The rendezvous, when the script asks for one: a call that never meets
        // its partner is a call that was not concurrent, and it fails loudly
        // instead of hanging the suite.
        let mut reply = reply;
        let reply = loop {
            match reply {
                Reply::Meet(barrier, inner) => {
                    if tokio::time::timeout(RENDEZVOUS_TIMEOUT, barrier.wait())
                        .await
                        .is_err()
                    {
                        panic!(
                            "FakeProvider: no other call met this barrier within \
                             {RENDEZVOUS_TIMEOUT:?}, so this call was not concurrent with another"
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
                // Announce before the loop can poll: the gesture a test sends
                // when it wakes still lands while this stream is in flight.
                opened.notify_one();
                let head = stream::iter(events.into_iter().map(Ok));
                Ok(Box::pin(head.chain(stream::pending())))
            }
            // Already unwrapped by the rendezvous loop above.
            Reply::Meet(..) => unreachable!("a rendezvous reply is unwrapped before use"),
        }
    }
}
