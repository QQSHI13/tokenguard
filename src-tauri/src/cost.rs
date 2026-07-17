//! Cost calculation. Per-model pricing is approximate and user-overridable.

/// Returns `(input_per_1k, output_per_1k)` in USD for known models.
///
/// # Pricing freshness
///
/// LLM providers change pricing often and without warning. The table below is a
/// best-effort snapshot used for *estimation only*. If no override is set and
/// the model is unknown, cost is reported as $0.00.
///
/// For accurate spend tracking, set per-provider input/output prices in
/// Settings. Token Guard never phones home to fetch pricing.
fn lookup(model: &str) -> Option<(f64, f64)> {
    let m = model.to_lowercase();
    if m.starts_with("gpt-4o-mini") {
        Some((0.15, 0.60))
    } else if m.starts_with("gpt-4o") {
        Some((2.50, 10.00))
    } else if m.starts_with("gpt-4-turbo") {
        Some((10.00, 30.00))
    } else if m.starts_with("gpt-4") {
        Some((30.00, 60.00))
    } else if m.starts_with("gpt-3.5") {
        Some((0.50, 1.50))
    } else if m.contains("claude-3-5-sonnet") {
        Some((3.00, 15.00))
    } else if m.contains("claude-3-5-haiku") {
        Some((0.80, 4.00))
    } else if m.contains("claude-3-opus") {
        Some((15.00, 75.00))
    } else if m.contains("claude-3-sonnet") {
        Some((3.00, 15.00))
    } else if m.contains("claude-3-haiku") {
        Some((0.25, 1.25))
    } else if m.contains("deepseek-chat") {
        Some((0.27, 1.10))
    } else if m.contains("deepseek-reasoner") {
        Some((0.55, 2.19))
    } else {
        None
    }
}

/// Estimate cost in USD.
///
/// `input_per_1k` / `output_per_1k` override the hardcoded table when set
/// (used for custom / local providers).
#[allow(clippy::too_many_arguments)]
pub fn estimate(
    model_local: &str,
    model_remote: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    input_per_1k: Option<f64>,
    output_per_1k: Option<f64>,
    cached_input_per_1k: Option<f64>,
) -> f64 {
    let (table_i, table_o) = lookup(model_local)
        .or_else(|| lookup(model_remote))
        .unwrap_or((0.0, 0.0));
    let i = input_per_1k.unwrap_or(table_i);
    let o = output_per_1k.unwrap_or(table_o);
    // If no explicit cached price, treat cached tokens at the normal input price.
    let ci = cached_input_per_1k.unwrap_or(i);
    let regular_input = prompt_tokens.saturating_sub(cached_tokens);
    (regular_input as f64 * i + cached_tokens as f64 * ci + completion_tokens as f64 * o) / 1000.0
}

/// Pre-flight cost/token estimate from the request body.
///
/// We can't tokenize the prompt locally, so we only use the declared maximum
/// output tokens (`max_tokens`, `max_completion_tokens`, `max_output_tokens`)
/// multiplied by the provider's output price. This gives a safe upper bound for
/// money/token limit checks. Returns `(estimated_cost, estimated_tokens)`.
pub fn estimate_request(
    body: &serde_json::Value,
    model_local: &str,
    model_remote: &str,
    input_per_1k: Option<f64>,
    output_per_1k: Option<f64>,
) -> (f64, u64) {
    let max_completion = body
        .get("max_tokens")
        .or_else(|| body.get("max_completion_tokens"))
        .or_else(|| body.get("max_output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(1).max(1);
    // Attacker-controlled values: saturate instead of wrapping to a low number
    // (which would bypass the pre-flight cost check with a ~$0 estimate).
    let total_completion = max_completion.saturating_mul(n);
    let cost = estimate(
        model_local,
        model_remote,
        0,
        total_completion,
        0,
        input_per_1k,
        output_per_1k,
        None,
    );
    (cost, total_completion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_known_model() {
        // gpt-4o: $2.50 / 1K input, $10.00 / 1K output
        let cost = estimate("gpt-4o", "gpt-4o", 1000, 500, 0, None, None, None);
        assert!((cost - 7.5).abs() < 0.001, "expected ~7.5, got {cost}");
    }

    #[test]
    fn estimate_unknown_model_falls_back_to_zero() {
        let cost = estimate(
            "some-local-model",
            "some-local-model",
            1000,
            500,
            0,
            None,
            None,
            None,
        );
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn estimate_override_wins() {
        let cost = estimate("gpt-4o", "gpt-4o", 1000, 500, 0, Some(1.0), Some(2.0), None);
        assert!((cost - 2.0).abs() < 0.001, "expected ~2.0, got {cost}");
    }

    #[test]
    fn estimate_partial_override_uses_table_for_missing_side() {
        let cost = estimate(
            "gpt-4o-mini",
            "gpt-4o-mini",
            1000,
            500,
            0,
            Some(1.0),
            None,
            None,
        );
        // input override 1.0, output falls back to table 0.60 -> 1.0 + 0.3 = 1.3
        assert!((cost - 1.3).abs() < 0.001, "expected ~1.3, got {cost}");
    }

    #[test]
    fn estimate_cached_tokens_cheaper() {
        // 1000 input, 500 cached, 100 output
        // normal input $2.5, cached $0.5, output $10
        let cost = estimate(
            "gpt-4o",
            "gpt-4o",
            1000,
            100,
            500,
            Some(2.5),
            Some(10.0),
            Some(0.5),
        );
        // (500 * 2.5 + 500 * 0.5 + 100 * 10) / 1000 = (1250 + 250 + 1000) / 1000 = 2.5
        assert!((cost - 2.5).abs() < 0.001, "expected ~2.5, got {cost}");
    }

    #[test]
    fn estimate_request_multiplication_saturates() {
        // 2^32 * 2^32 would wrap to 0 in u64, faking a $0 estimate.
        let body = serde_json::json!({"max_tokens": 4294967296u64, "n": 4294967296u64});
        let (cost, tokens) = estimate_request(&body, "gpt-4o", "gpt-4o", None, None);
        assert_eq!(tokens, u64::MAX);
        assert!(cost > 0.0, "expected a non-zero estimate, got {cost}");
    }

    #[test]
    fn estimate_request_normal_values() {
        let body = serde_json::json!({"max_tokens": 1000u64, "n": 2u64});
        let (cost, tokens) = estimate_request(&body, "gpt-4o", "gpt-4o", None, None);
        assert_eq!(tokens, 2000);
        // gpt-4o output: $10.00 / 1K -> 2000 * 10 / 1000 = $20
        assert!((cost - 20.0).abs() < 0.001, "expected ~20.0, got {cost}");
    }
}
