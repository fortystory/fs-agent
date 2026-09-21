//! The render channel under a provider burst (spec §19).
//!
//! The renderer is a separate task and the channel is bounded (and deliberately
//! lossy). That is only safe if the producer is a good runtime citizen: a decoded
//! network chunk can carry hundreds of SSE frames, and if the pumping loop drains
//! them without ever yielding, the renderer task is never scheduled until the
//! bounded channel has already overflowed. The user sees `渲染器丢弃了 N 个事件`
//! and loses the live tail.

use futures::StreamExt;

use fs_agent::events::SpeakerId;
use fs_agent::provider::capability::caps_for;
use fs_agent::provider::openai::sse_stream;
use fs_agent::provider::StreamEvent;
use fs_agent::render::{self, RENDER_CHANNEL_CAPACITY};

/// One SSE frame carrying one text delta, as the wire writes it.
fn sse_frame(text: &str) -> Vec<u8> {
    let chunk = serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
    });
    format!("data: {chunk}\n\n").into_bytes()
}

#[tokio::test]
async fn a_burst_of_decoded_deltas_does_not_starve_the_renderer() {
    // One network chunk carrying more frames than the channel can hold. The
    // decoder queues them all, so the stream hands them back-to-back; if nothing
    // yields, the renderer task never runs and the channel drops its oldest.
    let frames = RENDER_CHANNEL_CAPACITY * 2;
    let mut body = Vec::new();
    for _ in 0..frames {
        body.extend_from_slice(&sse_frame("x"));
    }

    let caps = caps_for("deepseek-flash").unwrap();
    let bytes = futures::stream::once(async move { Ok::<_, reqwest::Error>(body) });
    let mut stream = Box::pin(sse_stream(bytes, caps));

    let (handle, mut receiver) = render::channel();
    let consumer = tokio::spawn(async move {
        let mut received = 0usize;
        let mut dropped = 0u64;
        loop {
            match receiver.recv().await {
                Ok(_) => received += 1,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => dropped += count,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        (received, dropped)
    });

    // The pumping loop, exactly as the turn loop drives it.
    let speaker = SpeakerId::Debater("kimi".into());
    while let Some(event) = stream.next().await {
        if let Ok(StreamEvent::TextDelta(text)) = event {
            handle.text_delta(&speaker, &text);
        }
    }
    drop(handle);

    let (received, dropped) = consumer.await.unwrap();
    assert_eq!(dropped, 0, "the renderer was starved and lost events");
    assert_eq!(received, frames, "every delta reached the renderer");
}
