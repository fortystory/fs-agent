//! provider 一阵猛冲之下那条渲染通道（spec §19）。
//!
//! 渲染器是一个独立任务，而通道是有界的（还故意允许有损）。
//! 只有生产者是个守规矩的运行时公民时这才能安全：一个解码后的
//! 网络分片可能扛着几百个 SSE 帧，如果抽取循环一口气把它们抽干、
//! 一次都不让出，渲染器任务就永远不会被调度，一直到有界
//! 通道已经溢出为止。用户看到的是 `渲染器丢弃了 N 个事件`，
//! 活的那截尾巴也丢了。

use futures::StreamExt;

use heng::events::SpeakerId;
use heng::provider::capability::caps_for;
use heng::provider::openai::sse_stream;
use heng::provider::StreamEvent;
use heng::render::{self, RENDER_CHANNEL_CAPACITY};

/// 一个 SSE 帧扛着一条文本增量，按线上写出来的样子。
fn sse_frame(text: &str) -> Vec<u8> {
    let chunk = serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
    });
    format!("data: {chunk}\n\n").into_bytes()
}

#[tokio::test]
async fn a_burst_of_decoded_deltas_does_not_starve_the_renderer() {
    // 一个网络分片扛的帧比这条通道装得下的还多。解码器把它们全部排好，
    // 于是这条流一条接一条地连着交出来；如果没有谁让出，
    // 渲染器任务就不会跑，而通道丢掉它最旧的那批。
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

    // 抽取循环，与回合循环驱动它的方式一模一样。
    let speaker = SpeakerId::Debater("kimi".into());
    while let Some(event) = stream.next().await {
        if let Ok(StreamEvent::TextDelta(text)) = event {
            handle.text_delta(&speaker, &text);
        }
    }
    drop(handle);

    let (received, dropped) = consumer.await.unwrap();
    assert_eq!(dropped, 0, "渲染器被饿住了，丢了事件");
    assert_eq!(received, frames, "每一条增量都到了渲染器");
}
