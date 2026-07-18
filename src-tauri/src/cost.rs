//! Cost calculation. Per-model pricing is approximate and user-overridable.
//!
//! Pricing data lives in `pricing.json` at the repository root and is embedded
//! into the binary at build time — Token Guard never fetches pricing from the
//! internet. The file is community-maintained (see CONTRIBUTING.md); every
//! entry cites its source. Prices are USD per 1K tokens.

#[derive(Debug, serde::Deserialize)]
struct PriceEntry {
    pattern: String,
    match_type: String,
    input_per_1k: f64,
    output_per_1k: f64,
    cached_input_per_1k: Option<f64>,
    // Provenance fields — validated by tests, not used at runtime.
    #[allow(dead_code)]
    provider: String,
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    updated: String,
}

/// Parsed `pricing.json`, sorted longest-pattern-first so the most specific
/// entry wins (e.g. `gpt-4o-mini` before `gpt-4o`).
fn price_table() -> &'static [PriceEntry] {
    static TABLE: std::sync::OnceLock<Vec<PriceEntry>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: serde_json::Value = serde_json::from_str(include_str!("../../pricing.json"))
            .expect("pricing.json must be valid JSON");
        let mut entries: Vec<PriceEntry> = serde_json::from_value(raw["models"].clone())
            .expect("pricing.json models must match the schema");
        entries.sort_by_key(|e| std::cmp::Reverse(e.pattern.len()));
        entries
    })
}

/// Returns `(input_per_1k, output_per_1k, cached_input_per_1k)` in USD for the
/// first matching model, or `None` if the model is unknown.
fn lookup(model: &str) -> Option<(f64, f64, Option<f64>)> {
    let m = model.to_lowercase();
    price_table()
        .iter()
        .find(|e| {
            let p = e.pattern.to_lowercase();
            match e.match_type.as_str() {
                "contains" => m.contains(&p),
                _ => m.starts_with(&p),
            }
        })
        .map(|e| (e.input_per_1k, e.output_per_1k, e.cached_input_per_1k))
}

/// Estimate cost in USD.
///
/// `input_per_1k` / `output_per_1k` / `cached_input_per_1k` override the
/// built-in table when set (used for custom / local providers).
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
    let (table_i, table_o, table_ci) = lookup(model_local)
        .or_else(|| lookup(model_remote))
        .unwrap_or((0.0, 0.0, None));
    let i = input_per_1k.unwrap_or(table_i);
    let o = output_per_1k.unwrap_or(table_o);
    // If no explicit cached price, treat cached tokens at the normal input price.
    let ci = cached_input_per_1k.or(table_ci).unwrap_or(i);
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
        .or(body.get("max_output_tokens"))
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
        // gpt-4o: $2.50 / 1M input, $10.00 / 1M output = $0.0025 / $0.01 per 1K
        let cost = estimate("gpt-4o", "gpt-4o", 1000, 500, 0, None, None, None);
        assert!(
            (cost - 0.0075).abs() < 0.0001,
            "expected ~0.0075, got {cost}"
        );
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
        // input override 1.0, output falls back to table 0.0006 -> 1.0 + 0.0003 = 1.0003
        assert!(
            (cost - 1.0003).abs() < 0.0001,
            "expected ~1.0003, got {cost}"
        );
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
    fn estimate_cached_tokens_use_table_cache_price() {
        // No overrides: gpt-4o cached input is $0.00125 / 1K from pricing.json.
        let cost = estimate("gpt-4o", "gpt-4o", 1000, 100, 500, None, None, None);
        // (500 * 0.0025 + 500 * 0.00125 + 100 * 0.01) / 1000 = 0.002875
        assert!(
            (cost - 0.002875).abs() < 0.0001,
            "expected ~0.002875, got {cost}"
        );
    }

    #[test]
    fn lookup_mini_beats_base_model() {
        let (i, o, _) = lookup("gpt-4o-mini").expect("gpt-4o-mini must be priced");
        assert_eq!(i, 0.00015);
        assert_eq!(o, 0.0006);
    }

    #[test]
    fn lookup_shorthand_alias_resolves() {
        // Local shorthand alias resolves to the same family pricing as the dated id.
        let shorthand = lookup("claude-sonnet-4-5").expect("alias must resolve");
        let dated = lookup("claude-sonnet-4-5-20250929").expect("dated id must resolve");
        assert_eq!(shorthand, dated);
    }

    #[test]
    fn lookup_deepseek_imported_price() {
        // deepseek-chat moved to $0.14 / $0.28 per 1M on models.dev — proves the
        // import, not the old stale built-in table, backs the lookup.
        let (i, o, ci) = lookup("deepseek-chat").expect("deepseek-chat must be priced");
        assert_eq!(i, 0.00014);
        assert_eq!(o, 0.00028);
        assert_eq!(ci, Some(0.0000028));
    }

    #[test]
    fn pricing_json_schema_is_valid() {
        let raw: serde_json::Value = serde_json::from_str(include_str!("../../pricing.json"))
            .expect("pricing.json must parse");
        let models = raw["models"].as_array().expect("models must be an array");
        assert!(!models.is_empty(), "pricing table must not be empty");
        let mut seen = std::collections::HashSet::new();
        for e in models {
            let pattern = e["pattern"].as_str().expect("pattern must be a string");
            assert!(!pattern.is_empty(), "pattern must not be empty");
            assert_eq!(pattern, pattern.to_lowercase(), "pattern must be lowercase");
            let mt = e["match_type"]
                .as_str()
                .expect("match_type must be a string");
            assert!(mt == "prefix" || mt == "contains", "bad match_type: {mt}");
            for field in ["input_per_1k", "output_per_1k"] {
                let v = e[field].as_f64().expect("price must be a number");
                assert!(v.is_finite() && v >= 0.0, "bad {field}: {v}");
            }
            if let Some(ci) = e.get("cached_input_per_1k") {
                let v = ci.as_f64().expect("cached price must be a number");
                assert!(v.is_finite() && v >= 0.0, "bad cached_input_per_1k: {v}");
            }
            let source = e["source"].as_str().expect("source must be a string");
            assert!(
                source.starts_with("https://"),
                "source must be an https URL"
            );
            assert!(e["updated"].as_str().is_some(), "updated must be a string");
            assert!(
                seen.insert((pattern.to_string(), mt.to_string())),
                "duplicate entry: {pattern} ({mt})"
            );
        }
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
        // gpt-4o output: $0.01 / 1K -> 2000 * 0.01 / 1000 = $0.02
        assert!((cost - 0.02).abs() < 0.0001, "expected ~0.02, got {cost}");
    }
}
