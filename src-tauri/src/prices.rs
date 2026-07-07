//! Built-in default model price database.
//!
//! Prices are stored in USD per 1K tokens. The built-in table is a fallback used
//! when a provider model does not have explicit user-provided costs. Prices can
//! be refreshed at runtime from a remote JSON URL without an app release.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

/// Price entry for a single model.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelPrice {
    /// Input cost in USD per 1K tokens.
    pub input_per_1k: f64,
    /// Output cost in USD per 1K tokens.
    pub output_per_1k: f64,
    /// Cached-input cost in USD per 1K tokens, if applicable.
    #[serde(default)]
    pub cached_input_per_1k: Option<f64>,
}

/// Built-in default prices for common models. These are loaded once at startup
/// and can be overridden by `refresh_prices_from_url` at runtime.
static DEFAULT_PRICES: Lazy<Mutex<HashMap<String, ModelPrice>>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // OpenAI
    map.insert(
        "gpt-4o".to_string(),
        ModelPrice {
            input_per_1k: 2.50,
            output_per_1k: 10.00,
            cached_input_per_1k: Some(1.25),
        },
    );
    map.insert(
        "gpt-4o-mini".to_string(),
        ModelPrice {
            input_per_1k: 0.15,
            output_per_1k: 0.60,
            cached_input_per_1k: Some(0.075),
        },
    );
    map.insert(
        "o1".to_string(),
        ModelPrice {
            input_per_1k: 15.00,
            output_per_1k: 60.00,
            cached_input_per_1k: Some(7.50),
        },
    );
    map.insert(
        "o1-mini".to_string(),
        ModelPrice {
            input_per_1k: 1.10,
            output_per_1k: 4.40,
            cached_input_per_1k: Some(0.55),
        },
    );
    map.insert(
        "o3-mini".to_string(),
        ModelPrice {
            input_per_1k: 1.10,
            output_per_1k: 4.40,
            cached_input_per_1k: Some(0.55),
        },
    );

    // Anthropic
    map.insert(
        "claude-3-5-sonnet".to_string(),
        ModelPrice {
            input_per_1k: 3.00,
            output_per_1k: 15.00,
            cached_input_per_1k: Some(0.30),
        },
    );
    map.insert(
        "claude-3-5-haiku".to_string(),
        ModelPrice {
            input_per_1k: 0.80,
            output_per_1k: 4.00,
            cached_input_per_1k: Some(0.08),
        },
    );
    map.insert(
        "claude-3-opus".to_string(),
        ModelPrice {
            input_per_1k: 15.00,
            output_per_1k: 75.00,
            cached_input_per_1k: Some(1.50),
        },
    );
    map.insert(
        "claude-3-sonnet".to_string(),
        ModelPrice {
            input_per_1k: 3.00,
            output_per_1k: 15.00,
            cached_input_per_1k: Some(0.30),
        },
    );
    map.insert(
        "claude-3-haiku".to_string(),
        ModelPrice {
            input_per_1k: 0.25,
            output_per_1k: 1.25,
            cached_input_per_1k: Some(0.03),
        },
    );

    // Groq
    map.insert(
        "llama-3".to_string(),
        ModelPrice {
            input_per_1k: 0.05,
            output_per_1k: 0.08,
            cached_input_per_1k: None,
        },
    );
    map.insert(
        "llama-3.1".to_string(),
        ModelPrice {
            input_per_1k: 0.05,
            output_per_1k: 0.08,
            cached_input_per_1k: None,
        },
    );
    map.insert(
        "llama-3.3".to_string(),
        ModelPrice {
            input_per_1k: 0.05,
            output_per_1k: 0.08,
            cached_input_per_1k: None,
        },
    );
    map.insert(
        "mixtral-8x7b".to_string(),
        ModelPrice {
            input_per_1k: 0.24,
            output_per_1k: 0.24,
            cached_input_per_1k: None,
        },
    );

    // DeepSeek
    map.insert(
        "deepseek-chat".to_string(),
        ModelPrice {
            input_per_1k: 0.27,
            output_per_1k: 1.10,
            cached_input_per_1k: Some(0.07),
        },
    );
    map.insert(
        "deepseek-reasoner".to_string(),
        ModelPrice {
            input_per_1k: 0.55,
            output_per_1k: 2.19,
            cached_input_per_1k: Some(0.14),
        },
    );

    // Google
    map.insert(
        "gemini-1.5-pro".to_string(),
        ModelPrice {
            input_per_1k: 1.25,
            output_per_1k: 5.00,
            cached_input_per_1k: Some(0.315),
        },
    );
    map.insert(
        "gemini-1.5-flash".to_string(),
        ModelPrice {
            input_per_1k: 0.075,
            output_per_1k: 0.30,
            cached_input_per_1k: Some(0.018),
        },
    );

    Mutex::new(map)
});

/// Return a copy of the current default price map.
pub fn get_default_prices() -> HashMap<String, ModelPrice> {
    DEFAULT_PRICES.lock().map(|m| m.clone()).unwrap_or_default()
}

/// Look up a default price entry by local model name (case-insensitive, and
/// tolerant of common prefixes/suffixes).
pub fn lookup_default_price(model: &str) -> Option<ModelPrice> {
    let key = normalize_model_name(model);
    let map = DEFAULT_PRICES.lock().ok()?;

    // Exact normalized match.
    if let Some(p) = map.get(&key) {
        return Some(*p);
    }

    // Prefix match: e.g. "gpt-4o-2024-08-06" should match "gpt-4o".
    for (k, v) in map.iter() {
        if key.starts_with(k) || k.starts_with(&key) {
            return Some(*v);
        }
    }

    None
}

fn normalize_model_name(model: &str) -> String {
    model.to_lowercase().replace(' ', "-")
}

/// Replace the entire in-memory price map with values fetched from a remote
/// JSON URL. The expected payload is a JSON object mapping model names to
/// `{ input_per_1k, output_per_1k, cached_input_per_1k? }`.
pub async fn refresh_prices_from_url(url: &str) -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("TokenGuard/0.1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to fetch prices: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("price fetch returned {}", resp.status()));
    }

    let fetched: HashMap<String, ModelPrice> = resp
        .json()
        .await
        .map_err(|e| format!("price JSON is invalid: {e}"))?;

    let mut map = DEFAULT_PRICES.lock().map_err(|e| e.to_string())?;
    let count = fetched.len();
    *map = fetched;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_exact_match() {
        let p = lookup_default_price("gpt-4o").unwrap();
        assert!((p.input_per_1k - 2.50).abs() < 0.001);
    }

    #[test]
    fn lookup_prefix_match() {
        let p = lookup_default_price("gpt-4o-2024-08-06").unwrap();
        assert!((p.input_per_1k - 2.50).abs() < 0.001);
    }

    #[test]
    fn lookup_case_insensitive() {
        let p = lookup_default_price("Claude-3-5-Sonnet").unwrap();
        assert!((p.input_per_1k - 3.00).abs() < 0.001);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup_default_price("totally-unknown-model").is_none());
    }
}
