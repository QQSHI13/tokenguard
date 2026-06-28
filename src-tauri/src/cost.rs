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
pub fn estimate(
    model_local: &str,
    model_remote: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    input_per_1k: Option<f64>,
    output_per_1k: Option<f64>,
) -> f64 {
    let (table_i, table_o) = lookup(model_local)
        .or_else(|| lookup(model_remote))
        .unwrap_or((0.0, 0.0));
    let i = input_per_1k.unwrap_or(table_i);
    let o = output_per_1k.unwrap_or(table_o);
    (prompt_tokens as f64 * i + completion_tokens as f64 * o) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_known_model() {
        // gpt-4o: $2.50 / 1K input, $10.00 / 1K output
        let cost = estimate("gpt-4o", "gpt-4o", 1000, 500, None, None);
        assert!((cost - 7.5).abs() < 0.001, "expected ~7.5, got {cost}");
    }

    #[test]
    fn estimate_unknown_model_falls_back_to_zero() {
        let cost = estimate(
            "some-local-model",
            "some-local-model",
            1000,
            500,
            None,
            None,
        );
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn estimate_override_wins() {
        let cost = estimate("gpt-4o", "gpt-4o", 1000, 500, Some(1.0), Some(2.0));
        assert!((cost - 2.0).abs() < 0.001, "expected ~2.0, got {cost}");
    }

    #[test]
    fn estimate_partial_override_uses_table_for_missing_side() {
        let cost = estimate("gpt-4o-mini", "gpt-4o-mini", 1000, 500, Some(1.0), None);
        // input override 1.0, output falls back to table 0.60 -> 1.0 + 0.3 = 1.3
        assert!((cost - 1.3).abs() < 0.001, "expected ~1.3, got {cost}");
    }
}
