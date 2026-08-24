//! Cost calculation. Per-model pricing is approximate and user-overridable.
//!
//! Pricing data lives in `pricing.json` at the repository root and is embedded
//! into the binary at build time — Token Guard never fetches pricing from the
//! internet. The file is community-maintained (see CONTRIBUTING.md); every
//! entry cites its source. Prices are USD per 1K tokens.

use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTier {
    /// Upper bound of the context window for this tier, in tokens.
    /// `None` means "up to the model's maximum" / catches everything larger.
    pub max_context_tokens: Option<u64>,
    pub input_per_1k: f64,
    pub output_per_1k: f64,
    pub cached_input_per_1k: Option<f64>,
}

/// Time-of-day pricing tier. All times are UTC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeTier {
    pub name: String,
    /// Days of the week when this tier applies, where 0 = Monday and 6 = Sunday.
    /// Empty means "all days".
    pub days: Vec<u8>,
    /// Start time in `HH:MM` format (UTC, inclusive).
    pub start: String,
    /// End time in `HH:MM` format (UTC, inclusive).
    pub end: String,
    pub input_per_1k: f64,
    pub output_per_1k: f64,
    pub cached_input_per_1k: Option<f64>,
}

/// A complete pricing profile for a model. All fields are optional so the
/// built-in table can supply some dimensions while a user override supplies
/// others.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingProfile {
    pub input_per_1k: Option<f64>,
    pub output_per_1k: Option<f64>,
    pub cached_input_per_1k: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tiers: Option<Vec<ContextTier>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_tiers: Option<Vec<TimeTier>>,
    /// Multiplier applied to the token cost after tier selection, e.g. 0.5 for
    /// a 50 % batch discount. Does not discount the flat request fee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_discount: Option<f64>,
    /// Price for reasoning tokens. If set, `reasoning_tokens` are charged at
    /// this rate and the remainder of completion tokens are charged at the
    /// output rate. This avoids double-charging when a provider reports
    /// reasoning as a subset of completion tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_per_1k: Option<f64>,
    /// Flat fee added to every request, e.g. per-image charges or minimum fees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_fee: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageBreakdown {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
    /// Optional UTC timestamp for time-of-day pricing. If omitted, time tiers
    /// are skipped.
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, serde::Deserialize)]
struct PriceEntry {
    pattern: String,
    match_type: String,
    #[serde(flatten)]
    profile: PricingProfile,
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

/// Returns the built-in pricing profile for the first matching model.
fn lookup(model: &str) -> Option<PricingProfile> {
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
        .map(|e| e.profile.clone())
}

impl PricingProfile {
    /// Select the context tier that applies to `prompt_tokens`. Tiers are
    /// sorted by `max_context_tokens` ascending; the first tier whose bound is
    /// >= prompt_tokens wins. A tier with `None` as its bound acts as a catch-all.
    fn select_context_tier(&self, prompt_tokens: u64) -> Option<&ContextTier> {
        let tiers = self.context_tiers.as_ref()?;
        let mut indexed: Vec<_> = tiers.iter().enumerate().collect();
        indexed.sort_by(
            |a, b| match (a.1.max_context_tokens, b.1.max_context_tokens) {
                (Some(a_max), Some(b_max)) => a_max.cmp(&b_max),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.0.cmp(&b.0),
            },
        );
        indexed
            .into_iter()
            .map(|(_, t)| t)
            .find(|t| match t.max_context_tokens {
                Some(max) => prompt_tokens <= max,
                None => true,
            })
    }

    /// Select the time tier that applies to the given UTC timestamp. Returns
    /// `None` if no timestamp is provided or no tier matches. Intervals may wrap
    /// across midnight (e.g. start 18:00, end 08:59).
    fn select_time_tier(
        &self,
        timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Option<&TimeTier> {
        let ts = timestamp?;
        let tiers = self.time_tiers.as_ref()?;
        let weekday = ts.weekday().num_days_from_monday() as u8;
        let minute_of_day = ts.hour() * 60 + ts.minute();
        tiers.iter().find(|t| {
            if !t.days.is_empty() && !t.days.contains(&weekday) {
                return false;
            }
            let parse_min = |s: &str| {
                let mut parts = s.split(':');
                let h: u32 = parts.next()?.parse().ok()?;
                let m: u32 = parts.next()?.parse().ok()?;
                Some(h * 60 + m)
            };
            let start = parse_min(&t.start).unwrap_or(0);
            let end = parse_min(&t.end).unwrap_or(23 * 60 + 59);
            if start <= end {
                minute_of_day >= start && minute_of_day <= end
            } else {
                minute_of_day >= start || minute_of_day <= end
            }
        })
    }
}

/// Estimate cost in USD.
///
/// `override_profile` overrides the built-in pricing table for custom/local
/// providers and per-model mappings.
pub fn estimate(
    model_local: &str,
    model_remote: &str,
    usage: &UsageBreakdown,
    override_profile: &PricingProfile,
) -> f64 {
    let table_profile = lookup(model_local)
        .or_else(|| lookup(model_remote))
        .unwrap_or_default();

    // 1. Choose context and time tiers. Override-provided tiers replace the
    //    table tiers entirely; if the override does not define that dimension,
    //    fall back to the table.
    let context_tier = override_profile
        .context_tiers
        .as_ref()
        .and_then(|_| override_profile.select_context_tier(usage.prompt_tokens))
        .or_else(|| table_profile.select_context_tier(usage.prompt_tokens));
    let time_tier = override_profile
        .time_tiers
        .as_ref()
        .and_then(|_| override_profile.select_time_tier(usage.timestamp))
        .or_else(|| table_profile.select_time_tier(usage.timestamp));

    // 2. Resolve scalar prices. Override values take precedence, then time tiers,
    //    then context tiers, then base table values.
    let input_per_1k = override_profile
        .input_per_1k
        .or(time_tier.map(|t| t.input_per_1k))
        .or(context_tier.map(|t| t.input_per_1k))
        .or(table_profile.input_per_1k)
        .unwrap_or(0.0);
    let output_per_1k = override_profile
        .output_per_1k
        .or(time_tier.map(|t| t.output_per_1k))
        .or(context_tier.map(|t| t.output_per_1k))
        .or(table_profile.output_per_1k)
        .unwrap_or(0.0);
    let cached_input_per_1k = override_profile
        .cached_input_per_1k
        .or(time_tier.and_then(|t| t.cached_input_per_1k))
        .or(context_tier.and_then(|t| t.cached_input_per_1k))
        .or(table_profile.cached_input_per_1k)
        .or(Some(input_per_1k))
        .unwrap_or(input_per_1k);

    // 3. Compute token costs.
    let regular_input = usage.prompt_tokens.saturating_sub(usage.cached_tokens);
    let mut token_cost =
        regular_input as f64 * input_per_1k + usage.cached_tokens as f64 * cached_input_per_1k;

    // Resolve remaining dimensions: reasoning, batch discount, request fee.
    let reasoning_per_1k = override_profile
        .reasoning_per_1k
        .or(table_profile.reasoning_per_1k);
    let batch_discount = override_profile
        .batch_discount
        .or(table_profile.batch_discount);
    let request_fee = override_profile.request_fee.or(table_profile.request_fee);

    // Reasoning tokens are a subset of completion tokens. Charge them at the
    // reasoning rate and the remaining completion tokens at the output rate.
    let reasoning_tokens = usage.reasoning_tokens.min(usage.completion_tokens);
    let regular_completion = usage.completion_tokens.saturating_sub(reasoning_tokens);
    if let Some(reasoning_rate) = reasoning_per_1k {
        token_cost += reasoning_tokens as f64 * reasoning_rate;
    } else {
        token_cost += reasoning_tokens as f64 * output_per_1k;
    }
    token_cost += regular_completion as f64 * output_per_1k;

    token_cost /= 1000.0;

    // Apply batch discount (if any) and flat request fee.
    if let Some(discount) = batch_discount {
        token_cost *= discount;
    }
    if let Some(fee) = request_fee {
        token_cost += fee;
    }

    token_cost
}

/// Fallback output budget when the client declares no maximum.
///
/// Anthropic requires `max_tokens`, but the OpenAI, Responses and Google formats
/// all make it optional — and with no declared cap the pre-flight estimate would
/// be zero tokens for zero dollars, so `Money`/`Tokens`/`CostPerRequest` limits
/// would never fire on exactly the requests most able to blow through them.
/// Assume a typical response instead.
const ASSUMED_MAX_COMPLETION_TOKENS: u64 = 4096;

/// Rough characters-per-token ratio for English-weighted prompts. Only used for
/// the pre-flight estimate — the authoritative token counts come back from the
/// provider in the response and are what we actually bill and log.
const CHARS_PER_TOKEN: u64 = 4;

/// Ratio for scripts where one character is roughly one token (CJK, and by
/// extension most non-Latin scripts). BPE tokenizers spend about a token per
/// Han/Hiragana/Hangul character, so dividing those by [`CHARS_PER_TOKEN`] would
/// under-count a Chinese prompt ~4x — the direction that lets a request slip
/// past a `Money` or `Tokens` limit.
const WIDE_CHARS_PER_TOKEN: u64 = 1;

/// True for characters a BPE tokenizer typically spends a whole token on.
///
/// Covers CJK ideographs and the Japanese/Korean syllabaries; deliberately not
/// an exhaustive Unicode script test, since this only feeds an estimate.
fn is_wide_script(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30ff}'      // Hiragana + Katakana
        | '\u{3400}'..='\u{4dbf}'    // CJK Ext A
        | '\u{4e00}'..='\u{9fff}'    // CJK Unified
        | '\u{ac00}'..='\u{d7af}'    // Hangul syllables
        | '\u{f900}'..='\u{faff}'    // CJK compatibility
        | '\u{20000}'..='\u{2ffff}'  // CJK Ext B-F
    )
}

/// Estimated tokens for one string, counting *characters* rather than bytes.
///
/// `s.len()` is the UTF-8 byte length, so a Chinese prompt (3 bytes per char)
/// estimated ~3x too high and a Cyrillic one ~2x — the estimate is what
/// `CostPerRequest` and pre-flight `Money` limits enforce against, so inflating
/// it blocks requests that would have been affordable.
fn string_tokens(s: &str) -> u64 {
    let mut narrow: u64 = 0;
    let mut wide: u64 = 0;
    for c in s.chars() {
        if is_wide_script(c) {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    narrow / CHARS_PER_TOKEN + wide / WIDE_CHARS_PER_TOKEN
}

/// Keys whose string values are opaque blobs, not prompt text.
///
/// A base64 data URL for a 1 MB image is ~1.4 M characters, which the generic
/// walk would price as ~350 k prompt tokens — enough to trip a per-request cap on
/// a request that actually bills a few hundred tokens for the image. Providers
/// price images by dimensions, not by the length of the encoding, so the blob is
/// better excluded than counted.
fn is_opaque_blob_key(key: &str) -> bool {
    matches!(
        key,
        // OpenAI/Anthropic image + file payloads, Google inlineData.
        "b64_json" | "data" | "image_url" | "url" | "source" | "inline_data" | "inlineData"
    )
}

/// Sum the estimated tokens of every string *value* in the body (object keys
/// excluded, opaque blobs skipped — see [`is_opaque_blob_key`]).
///
/// Deliberately format-agnostic: OpenAI `messages[].content`, Anthropic `system`,
/// Google `contents[].parts[].text` and Responses `input` all reduce to strings
/// somewhere in the tree, as do tool schemas, so one walk covers all four
/// dialects and any future field without a per-format extractor to keep in sync.
fn text_tokens(v: &serde_json::Value) -> u64 {
    match v {
        serde_json::Value::String(s) => string_tokens(s),
        serde_json::Value::Array(items) => items.iter().map(text_tokens).sum(),
        serde_json::Value::Object(map) => map
            .iter()
            .filter(|(k, _)| !is_opaque_blob_key(k))
            .map(|(_, v)| text_tokens(v))
            .sum(),
        _ => 0,
    }
}

/// Approximate the prompt size without a tokenizer.
fn estimate_prompt_tokens(body: &serde_json::Value) -> u64 {
    text_tokens(body)
}

/// Pre-flight cost/token estimate from the request body.
///
/// We can't tokenize the prompt locally, so the prompt side is approximated from
/// the request's text length and the output side from the declared maximum
/// (`max_tokens`, `max_completion_tokens`, `max_output_tokens`,
/// `generationConfig.maxOutputTokens`), falling back to
/// `ASSUMED_MAX_COMPLETION_TOKENS`. Returns `(estimated_cost, estimated_tokens)`.
pub fn estimate_request(
    body: &serde_json::Value,
    model_local: &str,
    model_remote: &str,
    profile: &PricingProfile,
) -> (f64, u64) {
    let generation_config = body.get("generationConfig");
    let max_completion = body
        .get("max_tokens")
        .or_else(|| body.get("max_completion_tokens"))
        .or_else(|| body.get("max_output_tokens"))
        .or_else(|| generation_config.and_then(|g| g.get("maxOutputTokens")))
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .unwrap_or(ASSUMED_MAX_COMPLETION_TOKENS);
    let n = body
        .get("n")
        .or_else(|| generation_config.and_then(|g| g.get("candidateCount")))
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1);
    // Attacker-controlled values: saturate instead of wrapping to a low number
    // (which would bypass the pre-flight cost check with a ~$0 estimate).
    let total_completion = max_completion.saturating_mul(n);
    let prompt_tokens = estimate_prompt_tokens(body);
    let cost = estimate(
        model_local,
        model_remote,
        &UsageBreakdown {
            prompt_tokens,
            completion_tokens: total_completion,
            cached_tokens: 0,
            reasoning_tokens: 0,
            timestamp: Some(chrono::Utc::now()),
        },
        profile,
    );
    (cost, prompt_tokens.saturating_add(total_completion))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_known_model() {
        // gpt-4o: $2.50 / 1M input, $10.00 / 1M output = $0.0025 / $0.01 per 1K
        let cost = estimate(
            "gpt-4o",
            "gpt-4o",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile::default(),
        );
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
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile::default(),
        );
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn estimate_override_wins() {
        let cost = estimate(
            "gpt-4o",
            "gpt-4o",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile {
                input_per_1k: Some(1.0),
                output_per_1k: Some(2.0),
                ..Default::default()
            },
        );
        assert!((cost - 2.0).abs() < 0.001, "expected ~2.0, got {cost}");
    }

    #[test]
    fn estimate_partial_override_uses_table_for_missing_side() {
        let cost = estimate(
            "gpt-4o-mini",
            "gpt-4o-mini",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile {
                input_per_1k: Some(1.0),
                ..Default::default()
            },
        );
        // input override 1.0, output falls back to table 0.0006 -> 1.0 + 0.0003 = 1.0003
        assert!(
            (cost - 1.0003).abs() < 0.0001,
            "expected ~1.0003, got {cost}"
        );
    }

    #[test]
    fn estimate_cached_tokens_cheaper() {
        let cost = estimate(
            "gpt-4o",
            "gpt-4o",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 100,
                cached_tokens: 500,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile {
                input_per_1k: Some(2.5),
                output_per_1k: Some(10.0),
                cached_input_per_1k: Some(0.5),
                ..Default::default()
            },
        );
        // (500 * 2.5 + 500 * 0.5 + 100 * 10) / 1000 = (1250 + 250 + 1000) / 1000 = 2.5
        assert!((cost - 2.5).abs() < 0.001, "expected ~2.5, got {cost}");
    }

    #[test]
    fn estimate_cached_tokens_use_table_cache_price() {
        // No overrides: gpt-4o cached input is $0.00125 / 1K from pricing.json.
        let cost = estimate(
            "gpt-4o",
            "gpt-4o",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 100,
                cached_tokens: 500,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile::default(),
        );
        // (500 * 0.0025 + 500 * 0.00125 + 100 * 0.01) / 1000 = 0.002875
        assert!(
            (cost - 0.002875).abs() < 0.0001,
            "expected ~0.002875, got {cost}"
        );
    }

    #[test]
    fn estimate_context_tier_selects_correct_band() {
        let profile = PricingProfile {
            context_tiers: Some(vec![
                ContextTier {
                    max_context_tokens: Some(128_000),
                    input_per_1k: 0.1,
                    output_per_1k: 0.2,
                    cached_input_per_1k: None,
                },
                ContextTier {
                    max_context_tokens: None,
                    input_per_1k: 0.5,
                    output_per_1k: 1.0,
                    cached_input_per_1k: None,
                },
            ]),
            ..Default::default()
        };
        let small = estimate(
            "tiered-model",
            "tiered-model",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &profile,
        );
        // (1000 * 0.1 + 500 * 0.2) / 1000 = 0.2
        assert!((small - 0.2).abs() < 0.0001, "expected ~0.2, got {small}");

        let large = estimate(
            "tiered-model",
            "tiered-model",
            &UsageBreakdown {
                prompt_tokens: 200_000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &profile,
        );
        // (200000 * 0.5 + 500 * 1.0) / 1000 = 100.5
        assert!(
            (large - 100.5).abs() < 0.0001,
            "expected ~100.5, got {large}"
        );
    }

    #[test]
    fn estimate_time_tier_selects_correct_band() {
        let profile = PricingProfile {
            time_tiers: Some(vec![
                TimeTier {
                    name: "peak".to_string(),
                    days: vec![0, 1, 2, 3, 4, 5, 6],
                    start: "09:00".to_string(),
                    end: "17:59".to_string(),
                    input_per_1k: 2.0,
                    output_per_1k: 4.0,
                    cached_input_per_1k: None,
                },
                TimeTier {
                    name: "off-peak".to_string(),
                    days: vec![0, 1, 2, 3, 4, 5, 6],
                    start: "18:00".to_string(),
                    end: "08:59".to_string(),
                    input_per_1k: 0.5,
                    output_per_1k: 1.0,
                    cached_input_per_1k: None,
                },
            ]),
            ..Default::default()
        };
        let peak_ts =
            chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 1, 5, 12, 0, 0).unwrap();
        let peak = estimate(
            "time-tiered-model",
            "time-tiered-model",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: Some(peak_ts),
            },
            &profile,
        );
        // (1000 * 2.0 + 500 * 4.0) / 1000 = 4.0
        assert!((peak - 4.0).abs() < 0.0001, "expected ~4.0, got {peak}");

        let off_peak_ts =
            chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 1, 5, 20, 0, 0).unwrap();
        let off_peak = estimate(
            "time-tiered-model",
            "time-tiered-model",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: Some(off_peak_ts),
            },
            &profile,
        );
        // (1000 * 0.5 + 500 * 1.0) / 1000 = 1.0
        assert!(
            (off_peak - 1.0).abs() < 0.0001,
            "expected ~1.0, got {off_peak}"
        );
    }

    #[test]
    fn estimate_reasoning_tokens_charged_separately() {
        let profile = PricingProfile {
            output_per_1k: Some(10.0),
            reasoning_per_1k: Some(5.0),
            ..Default::default()
        };
        let cost = estimate(
            "reasoning-model",
            "reasoning-model",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 300,
                timestamp: None,
            },
            &profile,
        );
        // (300 * 5.0 + 200 * 10.0) / 1000 = 3.5
        assert!((cost - 3.5).abs() < 0.0001, "expected ~3.5, got {cost}");
    }

    #[test]
    fn estimate_batch_discount_and_request_fee() {
        let profile = PricingProfile {
            input_per_1k: Some(1.0),
            output_per_1k: Some(2.0),
            batch_discount: Some(0.5),
            request_fee: Some(0.01),
            ..Default::default()
        };
        let cost = estimate(
            "batch-model",
            "batch-model",
            &UsageBreakdown {
                prompt_tokens: 1000,
                completion_tokens: 500,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &profile,
        );
        // ((1000 * 1.0 + 500 * 2.0) / 1000) * 0.5 + 0.01 = 2.0 * 0.5 + 0.01 = 1.01
        assert!((cost - 1.01).abs() < 0.0001, "expected ~1.01, got {cost}");
    }

    #[test]
    fn lookup_mini_beats_base_model() {
        let profile = lookup("gpt-4o-mini").unwrap();
        assert_eq!(profile.input_per_1k, Some(0.00015));
        assert_eq!(profile.output_per_1k, Some(0.0006));
    }

    #[test]
    fn lookup_shorthand_alias_resolves() {
        // Local shorthand alias resolves to the same family pricing as the dated id.
        let shorthand = lookup("claude-sonnet-4-5").unwrap();
        let dated = lookup("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(shorthand.input_per_1k, dated.input_per_1k);
        assert_eq!(shorthand.output_per_1k, dated.output_per_1k);
    }

    #[test]
    fn lookup_deepseek_imported_price() {
        // deepseek-chat pricing imported from models.dev.
        let profile = lookup("deepseek-chat").unwrap();
        assert!((profile.input_per_1k.unwrap() - 0.00014).abs() < 1e-9);
        assert!((profile.output_per_1k.unwrap() - 0.00028).abs() < 1e-9);
        assert!(profile.cached_input_per_1k.unwrap() > 0.0);
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
            // Base scalar prices are required unless time/context tiers supply them.
            let has_scalar_prices =
                e.get("input_per_1k").is_some() && e.get("output_per_1k").is_some();
            let has_tiers = e.get("context_tiers").is_some() || e.get("time_tiers").is_some();
            assert!(
                has_scalar_prices || has_tiers,
                "{pattern}: must have scalar prices or pricing tiers"
            );
            for field in ["input_per_1k", "output_per_1k"] {
                if let Some(v) = e.get(field) {
                    let v = v.as_f64().expect("price must be a number");
                    assert!(v.is_finite() && v >= 0.0, "bad {field}: {v}");
                }
            }
            if let Some(ci) = e.get("cached_input_per_1k") {
                let v = ci.as_f64().expect("cached price must be a number");
                assert!(v.is_finite() && v >= 0.0, "bad cached_input_per_1k: {v}");
            }
            if let Some(tiers) = e.get("context_tiers").and_then(|t| t.as_array()) {
                for tier in tiers {
                    for field in ["input_per_1k", "output_per_1k"] {
                        let v = tier[field]
                            .as_f64()
                            .expect("context tier price must be a number");
                        assert!(v.is_finite() && v >= 0.0, "bad context tier {field}: {v}");
                    }
                    if let Some(ci) = tier.get("cached_input_per_1k") {
                        let v = ci
                            .as_f64()
                            .expect("context tier cached price must be a number");
                        assert!(
                            v.is_finite() && v >= 0.0,
                            "bad context tier cached price: {v}"
                        );
                    }
                }
            }
            if let Some(tiers) = e.get("time_tiers").and_then(|t| t.as_array()) {
                for tier in tiers {
                    assert!(
                        tier["name"].as_str().is_some(),
                        "time tier name must be a string"
                    );
                    for field in ["input_per_1k", "output_per_1k"] {
                        let v = tier[field]
                            .as_f64()
                            .expect("time tier price must be a number");
                        assert!(v.is_finite() && v >= 0.0, "bad time tier {field}: {v}");
                    }
                    if let Some(ci) = tier.get("cached_input_per_1k") {
                        let v = ci
                            .as_f64()
                            .expect("time tier cached price must be a number");
                        assert!(v.is_finite() && v >= 0.0, "bad time tier cached price: {v}");
                    }
                }
            }
            if let Some(d) = e.get("batch_discount") {
                let v = d.as_f64().expect("batch_discount must be a number");
                assert!(v.is_finite() && v >= 0.0, "bad batch_discount: {v}");
            }
            if let Some(r) = e.get("reasoning_per_1k") {
                let v = r.as_f64().expect("reasoning price must be a number");
                assert!(v.is_finite() && v >= 0.0, "bad reasoning_per_1k: {v}");
            }
            if let Some(f) = e.get("request_fee") {
                let v = f.as_f64().expect("request_fee must be a number");
                assert!(v.is_finite() && v >= 0.0, "bad request_fee: {v}");
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
        let (cost, tokens) =
            estimate_request(&body, "gpt-4o", "gpt-4o", &PricingProfile::default());
        assert_eq!(tokens, u64::MAX);
        assert!(cost > 0.0, "expected a non-zero estimate, got {cost}");
    }

    #[test]
    fn estimate_request_normal_values() {
        let body = serde_json::json!({"max_tokens": 1000u64, "n": 2u64});
        let (cost, tokens) =
            estimate_request(&body, "gpt-4o", "gpt-4o", &PricingProfile::default());
        assert_eq!(tokens, 2000);
        // gpt-4o output: $0.01 / 1K -> 2000 * 0.01 / 1000 = $0.02
        assert!((cost - 0.02).abs() < 0.0001, "expected ~0.02, got {cost}");
    }

    #[test]
    fn estimate_request_assumes_output_budget_without_max_tokens() {
        // max_tokens is optional in every format except Anthropic. A zero
        // estimate here would let Money/Tokens/CostPerRequest limits pass
        // unconditionally, so the assumed budget must produce real numbers.
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
        });
        let (cost, tokens) =
            estimate_request(&body, "gpt-4o", "gpt-4o", &PricingProfile::default());
        assert!(
            tokens >= ASSUMED_MAX_COMPLETION_TOKENS,
            "expected at least the assumed output budget, got {tokens}"
        );
        assert!(cost > 0.0, "expected a non-zero estimate, got {cost}");
    }

    #[test]
    fn estimate_request_zero_max_tokens_falls_back() {
        // An explicit 0 is not a promise to emit nothing — treat it like absent.
        let body = serde_json::json!({"max_tokens": 0u64});
        let (_, tokens) = estimate_request(&body, "gpt-4o", "gpt-4o", &PricingProfile::default());
        assert_eq!(tokens, ASSUMED_MAX_COMPLETION_TOKENS);
    }

    #[test]
    fn estimate_request_counts_prompt_text() {
        // A long prompt costs money even with a small output cap.
        let long = "x".repeat(40_000);
        let small = serde_json::json!({"max_tokens": 1u64, "messages": []});
        let big = serde_json::json!({
            "max_tokens": 1u64,
            "messages": [{"role": "user", "content": long}],
        });
        let (small_cost, small_tokens) =
            estimate_request(&small, "gpt-4o", "gpt-4o", &PricingProfile::default());
        let (big_cost, big_tokens) =
            estimate_request(&big, "gpt-4o", "gpt-4o", &PricingProfile::default());
        assert!(
            big_tokens > small_tokens + 9_000,
            "expected the 40k-char prompt to dominate, got {big_tokens} vs {small_tokens}"
        );
        assert!(big_cost > small_cost);
    }

    #[test]
    fn prompt_estimate_counts_characters_not_bytes() {
        // "héllo wörld" is 11 chars but 13 UTF-8 bytes. Byte counting made every
        // accented prompt look bigger than it is, and the estimate is what the
        // pre-flight Money / CostPerRequest checks enforce against.
        let ascii = "hello world";
        let accented = "héllo wörld";
        assert_eq!(accented.chars().count(), ascii.chars().count());
        assert_ne!(accented.len(), ascii.len(), "sanity: byte lengths differ");
        assert_eq!(string_tokens(ascii), string_tokens(accented));
    }

    #[test]
    fn cjk_costs_about_one_token_per_character() {
        // A BPE tokenizer spends roughly a token per Han character. Dividing the
        // char count by 4 (the Latin ratio) would under-count ~4x, which is the
        // direction that lets a request slip past a limit.
        let han = "今天天气很好我想去公园散步"; // 13 chars
        assert_eq!(han.chars().count(), 13);
        assert_eq!(string_tokens(han), 13);

        // 13 ASCII chars, by contrast, are ~3 tokens.
        assert_eq!(string_tokens("abcdefghijklm"), 3);
    }

    #[test]
    fn mixed_script_text_splits_by_character_class() {
        // Latin at 1/4 token per char, Han at 1 per char, summed independently.
        let mixed = "model 模型"; // 6 narrow ("model ") + 2 wide
        assert_eq!(string_tokens(mixed), 6 / CHARS_PER_TOKEN + 2);
    }

    #[test]
    fn base64_image_payloads_are_not_priced_as_prompt_text() {
        // A 1 MB image is ~1.4M base64 chars. Counted as text it would read as
        // ~350k prompt tokens and trip a per-request cap, while the provider
        // actually bills the image by its dimensions.
        let blob = "A".repeat(200_000);
        let with_image = serde_json::json!({
            "max_tokens": 16u64,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "what is in this image?"},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{blob}")}},
                ],
            }],
        });
        let text_only = serde_json::json!({
            "max_tokens": 16u64,
            "messages": [{
                "role": "user",
                "content": [{"type": "text", "text": "what is in this image?"}],
            }],
        });
        let (_, with_tokens) =
            estimate_request(&with_image, "gpt-4o", "gpt-4o", &PricingProfile::default());
        let (_, text_tokens_) =
            estimate_request(&text_only, "gpt-4o", "gpt-4o", &PricingProfile::default());
        // Not exactly equal: the `"type": "image_url"` discriminator is a real
        // string in the body and is legitimately counted. What matters is that
        // the 200k-char blob contributes nothing.
        assert!(
            with_tokens - text_tokens_ < 10,
            "the base64 blob inflated the estimate: {with_tokens} vs {text_tokens_}"
        );

        // The same for Anthropic's `source` and Google's `inlineData` shapes.
        for body in [
            serde_json::json!({"messages": [{"content": [
                {"type": "image", "source": {"type": "base64", "data": blob.clone()}}
            ]}]}),
            serde_json::json!({"contents": [{"parts": [
                {"inlineData": {"mimeType": "image/png", "data": blob.clone()}}
            ]}]}),
        ] {
            let (_, tokens) =
                estimate_request(&body, "gpt-4o", "gpt-4o", &PricingProfile::default());
            assert!(
                tokens - ASSUMED_MAX_COMPLETION_TOKENS < 10,
                "the blob leaked into the prompt estimate: {tokens}"
            );
        }
    }

    #[test]
    fn prompt_estimate_still_walks_nested_text_in_every_dialect() {
        // The walk is format-agnostic by design; a regression here would silently
        // price large prompts at $0 in whichever dialect broke.
        let long = "word ".repeat(2_000); // 10_000 chars => ~2_500 tokens
        for body in [
            serde_json::json!({"messages": [{"role": "user", "content": long.clone()}]}),
            serde_json::json!({"system": long.clone(), "messages": []}),
            serde_json::json!({"contents": [{"parts": [{"text": long.clone()}]}]}),
            serde_json::json!({"input": long.clone()}),
        ] {
            assert!(
                estimate_prompt_tokens(&body) > 2_000,
                "prompt text not counted in {body:.60}"
            );
        }
    }

    #[test]
    fn estimate_request_reads_google_generation_config() {
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": "hi"}]}],
            "generationConfig": {"maxOutputTokens": 100u64, "candidateCount": 3u64},
        });
        let (_, tokens) = estimate_request(
            &body,
            "gemini-2.5-pro",
            "gemini-2.5-pro",
            &PricingProfile::default(),
        );
        // 100 * 3 output, plus a handful of prompt tokens.
        assert!(
            (300..320).contains(&tokens),
            "expected ~300 tokens, got {tokens}"
        );
    }

    #[test]
    fn estimate_table_deepseek_v4_flash_peak_and_off_peak() {
        // Official DeepSeek hours: peak 01:00-04:00 and 06:00-10:00 UTC.
        let peak_ts =
            chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 1, 5, 7, 0, 0).unwrap();
        let peak = estimate(
            "deepseek-v4-flash",
            "deepseek-v4-flash",
            &UsageBreakdown {
                prompt_tokens: 1_000_000,
                completion_tokens: 1_000_000,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: Some(peak_ts),
            },
            &PricingProfile::default(),
        );
        // 1M input * $0.44/M + 1M output * $1.32/M = $1.76
        assert!((peak - 1.76).abs() < 0.001, "expected ~1.76, got {peak}");

        let off_ts =
            chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 1, 5, 12, 0, 0).unwrap();
        let off = estimate(
            "deepseek-v4-flash",
            "deepseek-v4-flash",
            &UsageBreakdown {
                prompt_tokens: 1_000_000,
                completion_tokens: 1_000_000,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: Some(off_ts),
            },
            &PricingProfile::default(),
        );
        // 1M input * $0.22/M + 1M output * $0.66/M = $0.88
        assert!((off - 0.88).abs() < 0.001, "expected ~0.88, got {off}");
    }

    #[test]
    fn estimate_table_gemini_3_1_pro_context_tiers() {
        let small = estimate(
            "gemini-3.1-pro",
            "gemini-3.1-pro",
            &UsageBreakdown {
                prompt_tokens: 100_000,
                completion_tokens: 50_000,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile::default(),
        );
        // <=200K context: 100K*$2/M + 50K*$12/M = $0.20 + $0.60 = $0.80
        assert!((small - 0.8).abs() < 0.0001, "expected ~0.8, got {small}");

        let large = estimate(
            "gemini-3.1-pro",
            "gemini-3.1-pro",
            &UsageBreakdown {
                prompt_tokens: 300_000,
                completion_tokens: 50_000,
                cached_tokens: 0,
                reasoning_tokens: 0,
                timestamp: None,
            },
            &PricingProfile::default(),
        );
        // >200K context: 300K*$4/M + 50K*$18/M = $1.20 + $0.90 = $2.10
        assert!((large - 2.1).abs() < 0.0001, "expected ~2.1, got {large}");
    }
}
