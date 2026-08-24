use crate::config::{
    AuthScheme, BudgetPeriod, Config, LimitAction, ModelMapping, Project, Provider, ProviderFormat,
};
use crate::db;
use crate::proxy::{convert, forwarder};
use crate::state::{remote_model_name, AppState, LimitCheckResult, RequestSettlement};
use axum::body::to_bytes;
use axum::http::HeaderMap;
use bytes::Bytes;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Mocks {
    openai: MockServer,
    anthropic: MockServer,
    google: MockServer,
    responses: MockServer,
}

fn make_provider(
    name: &str,
    format: ProviderFormat,
    auth: AuthScheme,
    server: &MockServer,
    remote_model: &str,
) -> Provider {
    Provider {
        id: match format {
            ProviderFormat::OpenAI => 1,
            ProviderFormat::Anthropic => 2,
            ProviderFormat::Google => 3,
            ProviderFormat::Responses => 4,
        },
        name: name.to_string(),
        base_url: server.uri(),
        format,
        auth,
        models: vec![ModelMapping {
            local: "test-model".to_string(),
            remote: remote_model.to_string(),
            pricing: crate::cost::PricingProfile::default(),
        }],
        is_default: false,
        fallback_provider_id: None,
        extra_headers: Vec::new(),
    }
}

async fn setup() -> (Arc<AppState>, Mocks) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.db");
    let pool = db::build_pool(db_path.to_str().unwrap()).unwrap();

    let openai_mock = MockServer::start().await;
    let anthropic_mock = MockServer::start().await;
    let google_mock = MockServer::start().await;
    let responses_mock = MockServer::start().await;

    let config = Config {
        providers: vec![
            make_provider(
                "openai",
                ProviderFormat::OpenAI,
                AuthScheme::Bearer,
                &openai_mock,
                "gpt-4o",
            ),
            make_provider(
                "anthropic",
                ProviderFormat::Anthropic,
                AuthScheme::XApiKey,
                &anthropic_mock,
                "claude-3-5-sonnet",
            ),
            make_provider(
                "google",
                ProviderFormat::Google,
                AuthScheme::XGoogApiKey,
                &google_mock,
                "gemini-1.5-pro",
            ),
            make_provider(
                "responses",
                ProviderFormat::Responses,
                AuthScheme::Bearer,
                &responses_mock,
                "gpt-5.4",
            ),
        ],
        projects: vec![Project {
            id: 1,
            name: "test-project".to_string(),
            label_key: "test-key".to_string(),
            budget: 0.0,
            budget_period: BudgetPeriod::Daily,
            budget_action: LimitAction::Warn,
        }],
        limits: Vec::new(),
        ..Config::default()
    };

    let state = Arc::new(AppState::new(pool, db_path, config, None).unwrap());
    (
        state,
        Mocks {
            openai: openai_mock,
            anthropic: anthropic_mock,
            google: google_mock,
            responses: responses_mock,
        },
    )
}

fn client_case(format: ProviderFormat) -> (Value, &'static str) {
    match format {
        ProviderFormat::OpenAI => (
            serde_json::json!({
                "model": "test-model",
                "messages": [
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": "Hello"},
                ],
                "max_tokens": 100,
                "temperature": 0.5,
            }),
            "/v1/chat/completions",
        ),
        ProviderFormat::Anthropic => (
            serde_json::json!({
                "model": "test-model",
                "system": "You are a helpful assistant.",
                "messages": [{"role": "user", "content": "Hello"}],
                "max_tokens": 100,
                "temperature": 0.5,
            }),
            "/v1/messages",
        ),
        ProviderFormat::Google => (
            serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
                "systemInstruction": {"parts": [{"text": "You are a helpful assistant."}]},
                "generationConfig": {"maxOutputTokens": 100, "temperature": 0.5},
            }),
            "/v1beta/models/test-model:generateContent",
        ),
        ProviderFormat::Responses => (
            serde_json::json!({
                "model": "test-model",
                "input": "Hello",
                "instructions": "You are a helpful assistant.",
                "max_tokens": 100,
                "temperature": 0.5,
            }),
            "/v1/responses",
        ),
    }
}

fn upstream_case(format: ProviderFormat) -> Value {
    match format {
        ProviderFormat::OpenAI => serde_json::json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hi"},
                "finish_reason": "stop",
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
        }),
        ProviderFormat::Anthropic => serde_json::json!({
            "id": "msg-1",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }),
        ProviderFormat::Google => serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "Hi"}]}}],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15,
            },
        }),
        ProviderFormat::Responses => serde_json::json!({
            "id": "resp-1",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "message",
                "id": "msg-1",
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": "Hi"}],
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
        }),
    }
}

fn upstream_path_regex(format: ProviderFormat) -> &'static str {
    match format {
        ProviderFormat::OpenAI => "^/v1/chat/completions$",
        ProviderFormat::Anthropic => "^/v1/messages$",
        ProviderFormat::Responses => "^/v1/responses$",
        ProviderFormat::Google => "^/v1beta/models/[^/]+:generateContent$",
    }
}

fn mock_for(mocks: &Mocks, format: ProviderFormat) -> &MockServer {
    match format {
        ProviderFormat::OpenAI => &mocks.openai,
        ProviderFormat::Anthropic => &mocks.anthropic,
        ProviderFormat::Google => &mocks.google,
        ProviderFormat::Responses => &mocks.responses,
    }
}

fn provider_by_format(state: &AppState, format: ProviderFormat) -> Provider {
    let cfg = state.config.read().unwrap();
    cfg.providers
        .iter()
        .find(|p| p.format == format)
        .unwrap()
        .clone()
}

fn extract_text(format: ProviderFormat, body: &Value) -> Option<String> {
    match format {
        ProviderFormat::OpenAI => body["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from),
        ProviderFormat::Anthropic => body["content"][0]["text"].as_str().map(String::from),
        ProviderFormat::Google => body["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(String::from),
        ProviderFormat::Responses => body["output"][0]["content"][0]["text"]
            .as_str()
            .map(String::from),
    }
}

#[tokio::test]
async fn four_by_four_forward_matrix() {
    let (state, mocks) = setup().await;
    let client_formats = [
        ProviderFormat::OpenAI,
        ProviderFormat::Anthropic,
        ProviderFormat::Google,
        ProviderFormat::Responses,
    ];
    let provider_formats = [
        ProviderFormat::OpenAI,
        ProviderFormat::Anthropic,
        ProviderFormat::Google,
        ProviderFormat::Responses,
    ];

    for client_fmt in client_formats {
        let (client_body, client_path) = client_case(client_fmt);
        for provider_fmt in provider_formats {
            let mock = mock_for(&mocks, provider_fmt);
            let upstream_resp = upstream_case(provider_fmt);

            mock.reset().await;
            Mock::given(method("POST"))
                .and(path_regex(upstream_path_regex(provider_fmt)))
                .respond_with(ResponseTemplate::new(200).set_body_json(upstream_resp))
                .mount(mock)
                .await;

            let provider = provider_by_format(&state, provider_fmt);
            let remote_model = remote_model_name(&provider, "test-model");
            let body_bytes = Bytes::from(serde_json::to_vec(&client_body).unwrap());
            let start = Instant::now();
            let settlement = RequestSettlement::new(
                state.clone(),
                start,
                provider.id,
                Some("test-project".to_string()),
                "test-model".to_string(),
                &LimitCheckResult::default(),
            );

            let resp = forwarder::forward(
                state.clone(),
                start,
                client_path.to_string(),
                body_bytes,
                HeaderMap::new(),
                client_fmt,
                provider,
                "fake-api-key".to_string(),
                Some("test-project".to_string()),
                "test-model".to_string(),
                settlement,
            )
            .await;

            let label = format!("client={client_fmt:?}, provider={provider_fmt:?}");
            assert_eq!(resp.status(), 200, "{label}");

            let resp_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let resp_json: Value = serde_json::from_slice(&resp_bytes).unwrap();
            let text = extract_text(client_fmt, &resp_json)
                .unwrap_or_else(|| panic!("response text: {label}"));
            assert_eq!(text, "Hi", "{label}");

            let requests = mock.received_requests().await.unwrap();
            assert_eq!(requests.len(), 1, "{label}");

            let upstream_body: Value = serde_json::from_slice(&requests[0].body).unwrap();
            let expected_upstream_body =
                convert::convert_request(client_fmt, provider_fmt, &client_body, &remote_model);
            assert_eq!(upstream_body, expected_upstream_body, "{label}");
        }
    }
}

/// A hand-rolled SSE upstream that holds the connection open until told to
/// finish. wiremock delivers a whole body at once, which cannot express "the
/// response has started but is not done" — the exact state this test is about.
struct SlowSseUpstream {
    addr: std::net::SocketAddr,
    finish: tokio::sync::oneshot::Sender<()>,
}

impl SlowSseUpstream {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (finish, wait) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut sock, _) = listener.accept().await.unwrap();
            // Read the request head; the body length does not matter here.
            let mut buf = [0u8; 8192];
            let _ = sock.read(&mut buf).await;
            sock.write_all(
                b"HTTP/1.1 200 OK\r\n\
                  content-type: text/event-stream\r\n\
                  transfer-encoding: chunked\r\n\r\n",
            )
            .await
            .unwrap();
            // One chunk, then hold the stream open.
            let first = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";
            sock.write_all(format!("{:x}\r\n{}\r\n", first.len(), first).as_bytes())
                .await
                .unwrap();
            sock.flush().await.unwrap();
            let _ = wait.await;
            let last = "data: {\"choices\":[{\"delta\":{\"content\":\"!\"}}],\
                        \"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n\
                        data: [DONE]\n\n";
            sock.write_all(format!("{:x}\r\n{}\r\n0\r\n\r\n", last.len(), last).as_bytes())
                .await
                .unwrap();
            sock.flush().await.unwrap();
        });
        Self { addr, finish }
    }

    fn uri(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// The bug this guards: a streaming response used to release its in-flight
/// reservations when `forward` returned — which for SSE is the *first* byte. A
/// `ConcurrentRequests` limit therefore stopped counting streams that were still
/// open, and `TimeSec` measured time-to-first-byte.
#[tokio::test]
async fn streaming_holds_reservation_until_stream_ends() {
    let (state, _mocks) = setup().await;
    let upstream = SlowSseUpstream::start().await;

    let mut provider = provider_by_format(&state, ProviderFormat::OpenAI);
    provider.base_url = upstream.uri();
    const LIMIT_ID: i64 = 42;

    // Stand in for what check_limits would have reserved for a
    // ConcurrentRequests limit.
    let check = LimitCheckResult {
        reservations: vec![(LIMIT_ID, 1.0)],
        ..LimitCheckResult::default()
    };
    state.check_limits_test_reserve(LIMIT_ID, 1.0);
    assert_eq!(state.in_flight_for_limit(LIMIT_ID), 1.0);

    let settlement = RequestSettlement::new(
        state.clone(),
        Instant::now(),
        provider.id,
        Some("test-project".to_string()),
        "test-model".to_string(),
        &check,
    );

    let body = serde_json::json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true,
    });
    let resp = forwarder::forward(
        state.clone(),
        Instant::now(),
        "/v1/chat/completions".to_string(),
        Bytes::from(serde_json::to_vec(&body).unwrap()),
        HeaderMap::new(),
        ProviderFormat::OpenAI,
        provider,
        "fake-api-key".to_string(),
        Some("test-project".to_string()),
        "test-model".to_string(),
        settlement,
    )
    .await;
    assert_eq!(resp.status(), 200);

    // forward() has returned, but the upstream stream is still open: the request
    // is in flight and must still be counted.
    assert_eq!(
        state.in_flight_for_limit(LIMIT_ID),
        1.0,
        "reservation released before the stream ended"
    );

    // Let the upstream finish, drain the response, then wait for the pump task
    // to log and settle.
    upstream.finish.send(()).unwrap();
    let _ = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    for _ in 0..200 {
        if state.in_flight_for_limit(LIMIT_ID) == 0.0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        state.in_flight_for_limit(LIMIT_ID),
        0.0,
        "reservation was never released after the stream ended"
    );
}
