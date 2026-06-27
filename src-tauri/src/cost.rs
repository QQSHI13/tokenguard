//! Cost calculation. Per-model pricing is approximate and user-overridable.

/// Returns `(input_per_1k, output_per_1k)` in USD for known models.
///
/// NOTE: LLM pricing changes frequently. These rates are best-effort snapshots
/// for cost *estimation* only. Users can override per-provider in settings.
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
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    input_per_1k: Option<f64>,
    output_per_1k: Option<f64>,
) -> f64 {
    let (i, o) = match (input_per_1k, output_per_1k) {
        (Some(i), Some(o)) => (i, o),
        _ => lookup(model).unwrap_or((0.0, 0.0)),
    };
    (prompt_tokens as f64 * i + completion_tokens as f64 * o) / 1000.0
}
