use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::events::Usage;
use fs_agent::provider::{
    ChatRequest, EventStream, FinishReason, Provider, ProviderError, StreamEvent,
};
use futures::stream;

/// One scripted response for one `Provider::send` call.
#[derive(Clone)]
pub enum Reply {
    /// Emit these events in order (appending a terminal `Finished` if absent).
    Stream(Vec<StreamEvent>),
    /// Emit raw items, including mid-stream errors.
    Raw(Vec<Result<StreamEvent, ProviderError>>),
    /// Fail before any stream is produced.
    Fail(ProviderError),
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
}

impl FakeProvider {
    pub fn new(replies: Vec<Reply>) -> Self {
        Self {
            inner: Arc::new(Inner {
                replies: Mutex::new(replies.into()),
                requests: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn call_count(&self) -> usize {
        self.inner
            .requests
            .lock()
            .expect("fake provider poisoned")
            .len()
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
    async fn send(&self, request: ChatRequest) -> Result<EventStream, ProviderError> {
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
        }
    }
}
