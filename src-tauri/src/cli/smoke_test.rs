//! `tokenguard test` — end-to-end smoke test through the running gateway.
//!
//! Sends a tiny OpenAI-shaped chat request exactly like a teammate's client
//! would (label key auth → project tagging → limits → 4 × 4 conversion) and
//! reports status, latency, and usage.

use crate::state::AppState;
use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

pub struct TestArgs {
    /// Project label key (`tg_...`) used as the bearer token.
    pub key: String,
    /// Model to request.
    pub model: String,
    /// Prompt text.
    pub prompt: String,
    /// Gateway base URL; defaults to loopback on the configured port.
    pub base_url: Option<String>,
}

pub async fn run(state: &Arc<AppState>, args: TestArgs) -> Result<()> {
    let port = state
        .config
        .read()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .port;
    let base = args
        .base_url
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
    let url = format!("{base}/v1/chat/completions");

    println!("POST {url}");
    println!("  model:   {}", args.model);
    println!("  key:     {}…{}", &args.key[..3.min(args.key.len())], {
        let l = args.key.len();
        if l > 6 {
            &args.key[l - 2..]
        } else {
            ""
        }
    });

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": args.model,
        "messages": [{"role": "user", "content": args.prompt}],
        "max_tokens": 16,
    });
    let started = Instant::now();
    let resp = client
        .post(&url)
        .bearer_auth(&args.key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .context("request failed — is the gateway running? (`tokenguard start`)");
    let elapsed = started.elapsed();
    let resp = match resp {
        Ok(r) => r,
        Err(e) => anyhow::bail!("{e}"),
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        let hint = match status.as_u16() {
            401 => " — use a project label key (tg_...) created on this machine",
            403 => " — the project may be paused or over its budget/limits",
            404 => " — unknown model for the tagged project's providers",
            _ => "",
        };
        anyhow::bail!("gateway returned {status}{hint}\n{}", truncate(&text, 400));
    }

    let json: Value = serde_json::from_str(&text)
        .with_context(|| format!("non-JSON response: {}", truncate(&text, 200)))?;
    let model = json["model"].as_str().unwrap_or("?");
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(no content)");
    let prompt_tokens = json["usage"]["prompt_tokens"].as_i64();
    let completion_tokens = json["usage"]["completion_tokens"].as_i64();

    println!();
    println!("OK in {:.2?} ({status})", elapsed);
    println!("  model:   {model}");
    println!("  reply:   {}", truncate(content, 120));
    if let (Some(p), Some(c)) = (prompt_tokens, completion_tokens) {
        println!("  usage:   {p} in / {c} out tokens");
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncates_long_strings() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello…");
    }
}
