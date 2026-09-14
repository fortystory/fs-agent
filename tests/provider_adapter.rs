//! Provider adapter behaviour: the capability table, the wire body, SSE
//! fragment assembly, usage normalization, error classification and retry.
//!
//! Every one of these goes through a public pure function, so the vendor
//! differences are pinned down without a network call.

use std::time::Duration;

use fs_agent::config::{resolve, EnvMap, ReasoningEffort, BUILTIN_MODELS};
use fs_agent::events::Usage;
use fs_agent::provider::capability::{caps_for, ModelCaps, KNOWN_MODELS};
use fs_agent::provider::openai::{
    build_body, chat_completions_url, classify_status, normalize_usage, parse_retry_after,
    retry_delay, silent_warnings, BuildError, OpenAiProvider, RetryPolicy, StreamDecoder,
};
use fs_agent::provider::{
    ChatRequest, FinishReason, GenerationParams, Message, Provider, ProviderError, StreamEvent,
    ToolCall, ToolChoice, ToolSpec,
};

fn env(pairs: &[(&str, &str)]) -> EnvMap {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn kimi() -> ModelCaps {
    caps_for("kimi-k3").unwrap()
}

fn deepseek() -> ModelCaps {
    caps_for("deepseek-v4-pro").unwrap()
}

fn request(model: &str, params: GenerationParams) -> ChatRequest {
    ChatRequest {
        model: model.to_owned(),
        messages: vec![Message::User {
            content: "hi".to_owned(),
            name: None,
        }],
        tools: Vec::new(),
        tool_choice: ToolChoice::Auto,
        params,
        cache_key: Some("s-1".to_owned()),
    }
}

fn decode(caps: ModelCaps, frames: &[&str]) -> Vec<StreamEvent> {
    let mut decoder = StreamDecoder::new(caps);
    let mut events = Vec::new();
    for frame in frames {
        // Each frame is one complete SSE event, blank line included.
        events.extend(decoder.push(format!("{frame}\n\n").as_bytes()).unwrap());
    }
    events.extend(decoder.finish().unwrap());
    events
}

// --- capability table ------------------------------------------------------

#[test]
fn an_unregistered_model_is_an_error_not_a_silent_downgrade() {
    let error = caps_for("gpt-4o").expect_err("unknown ids must not be guessed");
    let message = error.to_string();
    assert!(message.contains("gpt-4o"), "{message}");
    for known in KNOWN_MODELS {
        assert!(message.contains(known), "{message}");
    }
}

#[test]
fn every_builtin_model_id_has_a_capability_entry() {
    for (id, _provider) in BUILTIN_MODELS {
        caps_for(id).unwrap_or_else(|error| panic!("built-in model {id}: {error}"));
    }
}

#[test]
fn every_registered_model_has_a_self_consistent_window() {
    for id in KNOWN_MODELS {
        let caps = caps_for(id).unwrap();
        assert!(
            caps.max_output_tokens <= caps.context_window,
            "{id}: output cap {} exceeds the context window {}",
            caps.max_output_tokens,
            caps.context_window
        );
        assert!(
            caps.min_cacheable_tokens < caps.context_window,
            "{id}: cache floor {} is not below the context window",
            caps.min_cacheable_tokens
        );
    }
}

#[test]
fn the_two_vendors_differ_where_the_spec_says_they_do() {
    let kimi = kimi();
    let deepseek = deepseek();

    // Kimi accepts prompt_cache_key; DeepSeek's cache is automatic.
    assert!(kimi.supports_prompt_cache_key);
    assert!(!deepseek.supports_prompt_cache_key);
    // Kimi K3 fixes temperature/top_p; DeepSeek accepts them.
    assert!(!kimi.supports_temperature);
    assert!(deepseek.supports_temperature);
    // Different output-cap parameter names.
    assert_eq!(kimi.max_tokens_field.field_name(), "max_completion_tokens");
    assert_eq!(deepseek.max_tokens_field.field_name(), "max_tokens");
    // Kimi only caches above 256 prompt tokens.
    assert!(kimi.min_cacheable_tokens > 256);
    assert_eq!(deepseek.min_cacheable_tokens, 0);
}

// --- request body ----------------------------------------------------------

#[test]
fn the_kimi_body_carries_the_session_cache_key_and_never_a_user_id() {
    let (body, warnings) = build_body(&request("kimi-k3", GenerationParams::default()), kimi());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(body["model"], "kimi-k3");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["prompt_cache_key"], "s-1");
    assert!(
        body.get("user_id").is_none(),
        "user_id must never be sent: {body}"
    );
}

#[test]
fn the_deepseek_body_omits_the_cache_key_it_does_not_support() {
    let (body, warnings) = build_body(
        &request("deepseek-v4-pro", GenerationParams::default()),
        deepseek(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        body.get("prompt_cache_key").is_none(),
        "DeepSeek has no prompt_cache_key: {body}"
    );
}

#[test]
fn the_output_cap_uses_each_vendors_own_parameter_name() {
    let params = GenerationParams {
        max_output_tokens: Some(4096),
        ..Default::default()
    };
    let (kimi_body, _) = build_body(&request("kimi-k3", params.clone()), kimi());
    assert_eq!(kimi_body["max_completion_tokens"], 4096);
    assert!(kimi_body.get("max_tokens").is_none());

    let (deepseek_body, _) = build_body(&request("deepseek-v4-pro", params), deepseek());
    assert_eq!(deepseek_body["max_tokens"], 4096);
    assert!(deepseek_body.get("max_completion_tokens").is_none());
}

#[test]
fn parameters_a_model_does_not_support_are_dropped_with_a_warning() {
    let params = GenerationParams {
        temperature: Some(0.5),
        top_p: Some(0.9),
        ..Default::default()
    };
    let (body, warnings) = build_body(&request("kimi-k3", params), kimi());

    assert!(body.get("temperature").is_none(), "{body}");
    assert!(body.get("top_p").is_none(), "{body}");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("temperature")));
    assert!(warnings.iter().any(|warning| warning.contains("top_p")));
}

#[test]
fn an_output_cap_above_the_model_maximum_is_clamped_with_a_warning() {
    let params = GenerationParams {
        max_output_tokens: Some(9_000_000),
        ..Default::default()
    };
    let (body, warnings) = build_body(&request("deepseek-v4-pro", params), deepseek());

    assert_eq!(body["max_tokens"], deepseek().max_output_tokens);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("clamped"), "{warnings:?}");
}

#[test]
fn reasoning_effort_is_sent_at_the_top_level() {
    let params = GenerationParams {
        reasoning_effort: Some(ReasoningEffort::Low),
        ..Default::default()
    };
    let (body, warnings) = build_body(&request("kimi-k3", params), kimi());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(body["reasoning_effort"], "low");
}

#[test]
fn an_assistant_turn_round_trips_its_reasoning_and_tool_calls() {
    let mut request = request("deepseek-v4-pro", GenerationParams::default());
    request.messages = vec![
        Message::User {
            content: "read it".to_owned(),
            name: None,
        },
        Message::Assistant {
            content: None,
            reasoning_content: Some("I should read the file".to_owned()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_owned(),
                name: "read_file".to_owned(),
                arguments: "{\"path\":\"a\"}".to_owned(),
            }],
            name: None,
        },
        Message::Tool {
            tool_call_id: "call_1".to_owned(),
            content: "contents".to_owned(),
        },
    ];
    request.tools = vec![ToolSpec {
        name: "read_file".to_owned(),
        description: "read a file".to_owned(),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let (body, warnings) = build_body(&request, deepseek());
    assert!(warnings.is_empty(), "{warnings:?}");

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["reasoning_content"], "I should read the file");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(
        messages[1]["tool_calls"][0]["function"]["name"],
        "read_file"
    );
    assert_eq!(
        messages[1]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"a\"}"
    );
    // A tool message cannot carry `name` on DeepSeek.
    assert_eq!(messages[2]["role"], "tool");
    assert!(messages[2].get("name").is_none());
    // The tool declaration is the provider's own JSON Schema shape.
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    assert_eq!(body["tool_choice"], "auto");
}

#[test]
fn the_chat_endpoint_is_joined_without_doubling_or_dropping_slashes() {
    assert_eq!(
        chat_completions_url("https://api.moonshot.cn/v1"),
        "https://api.moonshot.cn/v1/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.deepseek.com/"),
        "https://api.deepseek.com/chat/completions"
    );
}

// --- SSE decoding and fragment assembly ------------------------------------

#[test]
fn kimi_tool_call_fragments_are_assembled_into_one_completed_call() {
    let frames = [
        r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"pa"}}]},"finish_reason":null}]}"#,
        r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        r#"data: {"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":5,"cached_tokens":80}}"#,
        "data: [DONE]",
    ];
    let events = decode(kimi(), &frames);
    assert_eq!(
        events,
        vec![
            StreamEvent::ToolCallStarted {
                index: 0,
                id: "call_1".to_owned(),
                name: "read_file".to_owned(),
            },
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call_1".to_owned(),
                name: "read_file".to_owned(),
                arguments: "{\"path\":\"a\"}".to_owned(),
            },
            StreamEvent::Usage(Usage {
                input_tokens: 100,
                output_tokens: 5,
                cached_tokens: 80,
                miss_tokens: 20,
                reasoning_tokens: None,
            }),
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ]
    );
}

#[test]
fn deepseek_usage_rides_the_last_content_chunk() {
    let frames = [
        r#"data: {"choices":[{"index":0,"delta":{"reasoning_content":"thinking","content":"hi"},"finish_reason":null}]}"#,
        r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop","usage":{"prompt_tokens":100,"completion_tokens":5,"prompt_cache_hit_tokens":90,"prompt_cache_miss_tokens":10,"completion_tokens_details":{"reasoning_tokens":3}}}]}"#,
        "data: [DONE]",
    ];
    let events = decode(deepseek(), &frames);
    assert_eq!(
        events,
        vec![
            StreamEvent::ReasoningDelta("thinking".to_owned()),
            StreamEvent::TextDelta("hi".to_owned()),
            StreamEvent::Usage(Usage {
                input_tokens: 100,
                output_tokens: 5,
                cached_tokens: 90,
                miss_tokens: 10,
                reasoning_tokens: Some(3),
            }),
            StreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            },
        ]
    );
}

#[test]
fn kimi_and_deepseek_tool_calls_are_never_seen_as_fragments() {
    // Two calls interleaved by index: the layer above sees two complete calls.
    let frames = [
        r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"beta","arguments":"{"}},{"index":0,"id":"a","function":{"name":"alpha","arguments":"{}"}}]},"finish_reason":null}]}"#,
        "data: [DONE]",
    ];
    let events = decode(deepseek(), &frames);
    let completed: Vec<&StreamEvent> = events
        .iter()
        .filter(|event| matches!(event, StreamEvent::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(completed.len(), 2);
    match completed[0] {
        StreamEvent::ToolCallCompleted { index, id, .. } => {
            assert_eq!((*index, id.as_str()), (0, "a"));
        }
        other => panic!("expected a completed call, got {other:?}"),
    }
    match completed[1] {
        StreamEvent::ToolCallCompleted { index, id, .. } => {
            assert_eq!((*index, id.as_str()), (1, "b"));
        }
        other => panic!("expected a completed call, got {other:?}"),
    }
}

#[test]
fn a_frame_split_across_transport_chunks_is_reassembled() {
    let frame =
        r#"data: {"choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":"stop"}]}"#;
    let bytes = frame.as_bytes();
    let mut decoder = StreamDecoder::new(kimi());
    let mut events = decoder.push(&bytes[..20]).unwrap();
    assert!(events.is_empty(), "no complete frame yet: {events:?}");
    events.extend(decoder.push(&bytes[20..]).unwrap());
    events.extend(decoder.push(b"\n\n").unwrap());
    assert_eq!(events, vec![StreamEvent::TextDelta("hello".to_owned())]);
}

#[test]
fn a_stream_that_never_sees_done_produces_no_completed_unit() {
    let frame = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"stop\"}]}\n\n";
    let mut decoder = StreamDecoder::new(kimi());
    let mut events = decoder.push(frame).unwrap();
    events.extend(decoder.finish().unwrap());

    assert!(events.contains(&StreamEvent::TextDelta("partial".to_owned())));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, StreamEvent::Finished { .. })),
        "only [DONE] ends a stream: {events:?}"
    );
}

// --- usage normalization ---------------------------------------------------

#[test]
fn kimi_cached_tokens_become_cached_and_miss() {
    let usage = normalize_usage(
        fs_agent::config::Vendor::Kimi,
        &serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 10,
            "total_tokens": 110,
            "cached_tokens": 80,
        }),
    );
    assert_eq!(
        usage,
        Usage {
            input_tokens: 100,
            output_tokens: 10,
            cached_tokens: 80,
            miss_tokens: 20,
            reasoning_tokens: None,
        }
    );
}

#[test]
fn deepseek_hit_and_miss_tokens_become_cached_and_miss() {
    let usage = normalize_usage(
        fs_agent::config::Vendor::DeepSeek,
        &serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 10,
            "prompt_cache_hit_tokens": 90,
            "prompt_cache_miss_tokens": 10,
            "completion_tokens_details": { "reasoning_tokens": 3 },
        }),
    );
    assert_eq!(
        usage,
        Usage {
            input_tokens: 100,
            output_tokens: 10,
            cached_tokens: 90,
            miss_tokens: 10,
            reasoning_tokens: Some(3),
        }
    );
}

#[test]
fn a_missing_miss_count_falls_back_to_input_minus_cached() {
    let usage = normalize_usage(
        fs_agent::config::Vendor::DeepSeek,
        &serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 1,
            "prompt_cache_hit_tokens": 70,
        }),
    );
    assert_eq!(usage.cached_tokens, 70);
    assert_eq!(usage.miss_tokens, 30);
}

// --- error classification --------------------------------------------------

#[test]
fn the_six_error_classes_are_assigned_by_status_and_body() {
    let auth = classify_status(401, r#"{"error":{"message":"Authentication Fails"}}"#, None);
    assert_eq!(
        auth,
        ProviderError::Auth {
            detail: "Authentication Fails".to_owned()
        }
    );

    let balance = classify_status(402, r#"{"error":{"message":"Insufficient Balance"}}"#, None);
    assert!(matches!(balance, ProviderError::QuotaExhausted { .. }));

    let rate = classify_status(
        429,
        r#"{"error":{"message":"rate_limit_reached_error"}}"#,
        Some(Duration::from_secs(2)),
    );
    assert_eq!(
        rate,
        ProviderError::RateLimited {
            retry_after: Some(Duration::from_secs(2))
        }
    );

    // Kimi signals an exhausted quota with 429; the body is what separates it
    // from a plain rate limit.
    let kimi_quota = classify_status(
        429,
        r#"{"error":{"message":"Insufficient Balance"}}"#,
        Some(Duration::from_secs(2)),
    );
    assert!(
        matches!(kimi_quota, ProviderError::QuotaExhausted { .. }),
        "{kimi_quota:?}"
    );

    let invalid = classify_status(400, r#"{"error":{"message":"bad args"}}"#, None);
    assert!(matches!(invalid, ProviderError::InvalidRequest { .. }));

    let transport = classify_status(503, "upstream is down", None);
    assert!(matches!(transport, ProviderError::Transport { .. }));

    let protocol = classify_status(418, "teapot", None);
    assert!(matches!(protocol, ProviderError::Protocol { .. }));
}

#[test]
fn retry_after_parses_seconds_and_ignores_junk() {
    assert_eq!(parse_retry_after(Some("2")), Some(Duration::from_secs(2)));
    assert_eq!(
        parse_retry_after(Some(" 0.5 ")),
        Some(Duration::from_millis(500))
    );
    assert_eq!(parse_retry_after(None), None);
    assert_eq!(parse_retry_after(Some("")), None);
    assert_eq!(
        parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        None
    );
}

#[test]
fn retries_are_bounded_and_only_cover_transport_and_rate_limits() {
    let policy = RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(1),
    };
    let transport = ProviderError::Transport {
        detail: "dropped".to_owned(),
    };
    let auth = ProviderError::Auth {
        detail: "nope".to_owned(),
    };

    assert_eq!(
        retry_delay(policy, 1, &transport),
        Some(Duration::from_millis(100))
    );
    assert_eq!(
        retry_delay(policy, 2, &transport),
        Some(Duration::from_millis(200))
    );
    assert_eq!(retry_delay(policy, 3, &transport), None, "bounded");
    assert_eq!(retry_delay(policy, 1, &auth), None, "auth is not retryable");

    let limited = ProviderError::RateLimited {
        retry_after: Some(Duration::from_secs(30)),
    };
    assert_eq!(
        retry_delay(policy, 1, &limited),
        Some(Duration::from_secs(1)),
        "the vendor's retry-after is capped by max_delay"
    );
}

// --- provider construction -------------------------------------------------

#[test]
fn building_a_provider_without_a_key_fails_loudly() {
    let config = resolve(None, &env(&[])).unwrap();
    let error = OpenAiProvider::build(&config, "kimi-k3", silent_warnings())
        .err()
        .expect("a keyless provider must not build");
    match error {
        BuildError::MissingKey { provider, hint } => {
            assert_eq!(provider, "kimi");
            assert!(hint.contains("MOONSHOT_API_KEY"), "{hint}");
        }
        other => panic!("expected MissingKey, got {other}"),
    }
}

#[test]
fn building_a_provider_for_an_unregistered_model_fails_loudly() {
    let config = resolve(
        Some("[models.gpt-4o]\nprovider = \"kimi\"\n"),
        &env(&[("MOONSHOT_API_KEY", "sk-kimi")]),
    )
    .unwrap();
    let error = OpenAiProvider::build(&config, "gpt-4o", silent_warnings())
        .err()
        .expect("an unregistered model must not build");
    assert!(matches!(error, BuildError::UnknownModel(_)), "{error}");
}

#[test]
fn a_keyed_builtin_provider_builds_against_its_default_host() {
    let config = resolve(None, &env(&[("MOONSHOT_API_KEY", "sk-kimi")])).unwrap();
    let provider = OpenAiProvider::build(&config, "kimi-k3", silent_warnings()).unwrap();
    assert_eq!(provider.model(), "kimi-k3");
    assert_eq!(provider.profile().base_url, "https://api.moonshot.cn/v1");
    assert!(provider.caps().supports_prompt_cache_key);
}

#[test]
fn the_kimi_coding_plan_models_are_registered_with_their_own_windows() {
    // `k3` is the coding-plan id for the same model as `kimi-k3`; `k3-256k` is
    // its 256K-context variant.
    let k3 = caps_for("k3").unwrap();
    let k3_256k = caps_for("k3-256k").unwrap();
    assert_eq!(k3.context_window, 1_048_576);
    assert_eq!(k3_256k.context_window, 262_144);
    assert!(k3_256k.max_output_tokens <= k3_256k.context_window);
    assert!(k3_256k.supports_prompt_cache_key);
    assert!(k3_256k.requires_reasoning_replay);

    // K2.8 Preview takes an effort tier; K2.7 HighSpeed is thinking-on only.
    assert!(
        caps_for("kimi-for-coding")
            .unwrap()
            .supports_reasoning_effort
    );
    assert!(
        !caps_for("kimi-for-coding-highspeed")
            .unwrap()
            .supports_reasoning_effort
    );
}

#[test]
fn the_coding_plan_body_carries_the_cache_key_and_no_user_id() {
    let (body, warnings) = build_body(
        &request("k3-256k", GenerationParams::default()),
        caps_for("k3-256k").unwrap(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(body["prompt_cache_key"], "s-1");
    assert!(body.get("user_id").is_none(), "{body}");
}

#[test]
fn the_coding_plan_builds_against_the_coding_endpoint() {
    let config = resolve(None, &env(&[("KIMI_API_KEY", "sk-kimi-coding")])).unwrap();
    let provider = OpenAiProvider::build(&config, "k3-256k", silent_warnings()).unwrap();
    assert_eq!(
        provider.profile().base_url,
        "https://api.kimi.com/coding/v1"
    );
    assert_eq!(
        chat_completions_url(&provider.profile().base_url),
        "https://api.kimi.com/coding/v1/chat/completions"
    );
}

#[test]
fn kimi_code_plan_limits_are_read_from_the_body_not_the_status_alone() {
    // Kimi Code reports plan limits as 403, so 403 cannot mean "bad key" by
    // itself; quota windows are exhaustion and the concurrency cap is a rate
    // limit, neither of which is retried as an auth problem.
    let five_hour = classify_status(
        403,
        r#"{"error":{"message":"You've reached your 5-hour usage limit. Your quota will reset when the current 5-hour window ends."}}"#,
        None,
    );
    assert!(
        matches!(five_hour, ProviderError::QuotaExhausted { .. }),
        "{five_hour:?}"
    );

    let weekly = classify_status(
        403,
        r#"{"error":{"message":"You've reached your weekly (7-day) usage limit."}}"#,
        None,
    );
    assert!(
        matches!(weekly, ProviderError::QuotaExhausted { .. }),
        "{weekly:?}"
    );

    let concurrent = classify_status(
        403,
        r#"{"error":{"message":"You've reached your concurrent request limit. Please wait for your ongoing requests to finish and try again."}}"#,
        None,
    );
    assert!(
        matches!(concurrent, ProviderError::RateLimited { .. }),
        "{concurrent:?}"
    );

    // A 403 with no account-limit wording is still a refusal.
    let forbidden = classify_status(403, "forbidden", None);
    assert!(
        matches!(forbidden, ProviderError::Auth { .. }),
        "{forbidden:?}"
    );

    // Transient 429s stay rate limits.
    let transient = classify_status(
        429,
        r#"{"error":{"message":"The engine is currently overloaded, please try again later"}}"#,
        None,
    );
    assert!(
        matches!(transient, ProviderError::RateLimited { .. }),
        "{transient:?}"
    );
}
