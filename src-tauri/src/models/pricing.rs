//! Per-model pricing — direct port of worktale's `PRICE_PER_MTOK` tables.
//!
//! Source of truth:
//!   - Claude rates: `worktale-plugin/hooks/session-track.mjs`
//!   - OpenAI rates: `worktale-codex-plugin/hooks/session-track.mjs`
//!
//! When vendor prices change, update worktale's tables first (they're
//! the canonical home) then sync here. Diverging would silently
//! misreport costs — keep these in lockstep.

#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    /// USD per 1M input tokens.
    pub input_per_mtok: f64,
    /// USD per 1M output tokens.
    pub output_per_mtok: f64,
    /// USD per 1M cache-read input tokens (Anthropic prompt caching;
    /// OpenAI uses this for cached_input_tokens at ~50% of input).
    pub cache_read_per_mtok: f64,
    /// USD per 1M cache-creation input tokens written into the **5-minute**
    /// ephemeral tier. Claude only — OpenAI: 0. Equivalent to 1.25× input.
    pub cache_5m_per_mtok: f64,
    /// USD per 1M cache-creation input tokens written into the **1-hour**
    /// ephemeral tier. Claude only — OpenAI: 0. Equivalent to 2.0× input.
    /// Pre-1.0 we reported a single `cache_write_per_mtok` (= the 5m rate);
    /// once a session uses 1h cache that estimate undercounts.
    pub cache_1h_per_mtok: f64,
}

impl ModelPricing {
    /// Legacy single-rate cache write. Used when a transcript only has the
    /// flat `cache_creation_input_tokens` field and no 5m/1h breakdown.
    pub fn cache_write_per_mtok(&self) -> f64 {
        // 5m is the default tier and matches what worktale historically
        // computed, so prefer it for the legacy field.
        self.cache_5m_per_mtok
    }
}

// Anthropic ephemeral cache multipliers (relative to input rate).
//   5-minute tier: 1.25× input
//   1-hour   tier: 2.00× input
// Source: https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching
const PR_OPUS: ModelPricing = ModelPricing {
    input_per_mtok: 15.0,
    output_per_mtok: 75.0,
    cache_read_per_mtok: 1.5,
    cache_5m_per_mtok: 18.75, // 15.0 * 1.25
    cache_1h_per_mtok: 30.0,  // 15.0 * 2.0
};
const PR_SONNET: ModelPricing = ModelPricing {
    input_per_mtok: 3.0,
    output_per_mtok: 15.0,
    cache_read_per_mtok: 0.3,
    cache_5m_per_mtok: 3.75, // 3.0 * 1.25
    cache_1h_per_mtok: 6.0,  // 3.0 * 2.0
};
const PR_HAIKU: ModelPricing = ModelPricing {
    input_per_mtok: 1.0,
    output_per_mtok: 5.0,
    cache_read_per_mtok: 0.1,
    cache_5m_per_mtok: 1.25, // 1.0 * 1.25
    cache_1h_per_mtok: 2.0,  // 1.0 * 2.0
};

/// OpenAI doesn't have a write-cache rate, so cache 5m/1h are 0.
const fn price(input: f64, output: f64, cache_read: f64) -> ModelPricing {
    ModelPricing {
        input_per_mtok: input,
        output_per_mtok: output,
        cache_read_per_mtok: cache_read,
        cache_5m_per_mtok: 0.0,
        cache_1h_per_mtok: 0.0,
    }
}

/// Anthropic server-side tool pricing. These are billed **per request**,
/// not per token, on top of the model's own token usage.
/// Source: https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/web-search-tool
pub const WEB_SEARCH_USD_PER_REQUEST: f64 = 0.010; // $10 / 1,000 requests
pub const WEB_FETCH_USD_PER_REQUEST: f64 = 0.0; // free at time of writing

/// Anthropic — copied verbatim from `worktale-plugin/hooks/session-track.mjs`.
const ANTHROPIC_PRICING: &[(&str, ModelPricing)] = &[
    ("claude-opus-4-7", PR_OPUS),
    ("claude-opus-4-6", PR_OPUS),
    ("claude-opus-4-5", PR_OPUS),
    ("claude-opus-4-1", PR_OPUS),
    ("claude-opus-4", PR_OPUS),
    ("claude-sonnet-4-6", PR_SONNET),
    ("claude-sonnet-4-5", PR_SONNET),
    ("claude-sonnet-4", PR_SONNET),
    ("claude-haiku-4-5", PR_HAIKU),
];

/// OpenAI — copied verbatim from `worktale-codex-plugin/hooks/session-track.mjs`.
const OPENAI_PRICING: &[(&str, ModelPricing)] = &[
    ("gpt-5-nano", price(0.10, 0.80, 0.05)),
    ("gpt-5-mini", price(0.30, 2.40, 0.15)),
    ("gpt-5", price(15.0, 60.0, 7.5)),
    ("gpt-4.1-nano", price(0.10, 0.40, 0.05)),
    ("gpt-4.1-mini", price(0.40, 1.60, 0.20)),
    ("gpt-4.1", price(2.0, 8.0, 1.0)),
    ("gpt-4o-mini", price(0.15, 0.60, 0.075)),
    ("gpt-4o", price(2.50, 10.0, 1.25)),
    ("o3-pro", price(25.0, 100.0, 12.5)),
    ("o3-mini", price(1.10, 4.40, 0.55)),
    ("o3", price(15.0, 60.0, 7.5)),
    ("o4-mini", price(1.10, 4.40, 0.55)),
    ("o4", price(15.0, 60.0, 7.5)),
    ("o1-mini", price(1.10, 4.40, 0.55)),
    ("o1", price(15.0, 60.0, 7.5)),
    ("codex-mini", price(1.50, 6.0, 0.75)),
];

/// Normalize a model id the way worktale does: lowercase, strip `openai/`
/// prefix, strip `-YYYYMMDD` date suffix, strip `-preview` and `@<anything>`.
fn normalize_model_id(model_id: &str) -> String {
    let mut s = model_id.to_ascii_lowercase();
    if let Some(stripped) = s.strip_prefix("openai/") {
        s = stripped.to_string();
    }
    if let Some(idx) = find_date_suffix(&s) {
        s.truncate(idx);
    }
    if let Some(stripped) = s.strip_suffix("-preview") {
        s = stripped.to_string();
    }
    if let Some(at) = s.find('@') {
        s.truncate(at);
    }
    s
}

fn find_date_suffix(s: &str) -> Option<usize> {
    if s.len() < 9 {
        return None;
    }
    let bytes = s.as_bytes();
    let start = s.len() - 9; // -YYYYMMDD = 9 chars
    if bytes[start] != b'-' {
        return None;
    }
    if bytes[start + 1..].iter().all(|b| b.is_ascii_digit()) {
        Some(start)
    } else {
        None
    }
}

/// Look up pricing for a model id. Exact match wins, then longest-prefix
/// match (so `gpt-4.1-mini` doesn't get clobbered by `gpt-4`).
pub fn pricing_for(model_id: &str) -> Option<ModelPricing> {
    let norm = normalize_model_id(model_id);

    for (key, pricing) in ANTHROPIC_PRICING.iter().chain(OPENAI_PRICING.iter()) {
        if &norm == key {
            return Some(*pricing);
        }
    }

    let mut all: Vec<(&str, ModelPricing)> = ANTHROPIC_PRICING
        .iter()
        .chain(OPENAI_PRICING.iter())
        .copied()
        .collect();
    all.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (key, pricing) in all {
        if norm.starts_with(key) {
            return Some(pricing);
        }
    }
    None
}

/// Compute the dollar cost of a turn given raw token counts. Returns 0.0
/// when the model isn't in the pricing table — better $0 than an invented
/// number. Rounded to 4 decimals to match worktale's output.
///
/// `cache_write_tokens` is the legacy single-bucket count (treated as the
/// 5-minute tier — the historical default). For Anthropic transcripts that
/// expose the 5m/1h split, prefer [`cost_for_turn_v2`] so the 1h tier gets
/// billed at its higher rate.
pub fn cost_for_turn(
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
) -> f64 {
    cost_for_turn_v2(
        model_id,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        0,
        0,
        0,
    )
}

/// Cost for a turn including the 5m/1h cache-creation split and Anthropic
/// server-side tool requests (`web_search`, `web_fetch`).
///
/// `cache_5m_tokens` and `cache_1h_tokens` are mutually exclusive subsets
/// of the legacy `cache_creation_input_tokens`. When a transcript only
/// provides the flat field, pass it as `cache_5m_tokens` (the historical
/// default rate).
#[allow(clippy::too_many_arguments)]
pub fn cost_for_turn_v2(
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_5m_tokens: i64,
    cache_1h_tokens: i64,
    web_search_requests: i64,
    web_fetch_requests: i64,
) -> f64 {
    let Some(p) = pricing_for(model_id) else {
        return 0.0;
    };
    let mtok = 1_000_000f64;
    let raw = (input_tokens.max(0) as f64 / mtok) * p.input_per_mtok
        + (output_tokens.max(0) as f64 / mtok) * p.output_per_mtok
        + (cache_read_tokens.max(0) as f64 / mtok) * p.cache_read_per_mtok
        + (cache_5m_tokens.max(0) as f64 / mtok) * p.cache_5m_per_mtok
        + (cache_1h_tokens.max(0) as f64 / mtok) * p.cache_1h_per_mtok
        + (web_search_requests.max(0) as f64) * WEB_SEARCH_USD_PER_REQUEST
        + (web_fetch_requests.max(0) as f64) * WEB_FETCH_USD_PER_REQUEST;
    (raw * 10_000.0).round() / 10_000.0
}

/// Cheapest model per provider for the PR-create flow.
pub fn cheapest_model_for_provider(provider: &str) -> Option<&'static str> {
    match provider.to_ascii_lowercase().as_str() {
        "claude" | "anthropic" => Some("claude-haiku-4-5"),
        "codex" | "openai" => Some("gpt-5-nano"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_exact_match() {
        let p = pricing_for("claude-sonnet-4-5").unwrap();
        assert_eq!(p.input_per_mtok, 3.0);
        assert_eq!(p.cache_write_per_mtok(), 3.75);
        assert_eq!(p.cache_5m_per_mtok, 3.75);
        assert_eq!(p.cache_1h_per_mtok, 6.0);
    }

    #[test]
    fn anthropic_1h_cache_costs_more_than_5m() {
        // 1M tokens at 1h vs 5m for sonnet: 6 vs 3.75 = +60%.
        let cost_1h = cost_for_turn_v2("claude-sonnet-4-5", 0, 0, 0, 0, 1_000_000, 0, 0);
        let cost_5m = cost_for_turn_v2("claude-sonnet-4-5", 0, 0, 0, 1_000_000, 0, 0, 0);
        assert!(cost_1h > cost_5m);
        assert!((cost_1h - 6.0).abs() < 1e-9);
        assert!((cost_5m - 3.75).abs() < 1e-9);
    }

    #[test]
    fn web_search_billed_per_request() {
        // 100 web-search requests = $1.00 regardless of model token cost.
        let cost = cost_for_turn_v2("claude-haiku-4-5", 0, 0, 0, 0, 0, 100, 0);
        assert!((cost - 1.0).abs() < 1e-9);
    }

    #[test]
    fn legacy_cost_for_turn_treats_cache_write_as_5m() {
        // The pre-v2 entry point is still callable; its cache_write rate
        // matches the 5m tier so existing call sites don't suddenly change
        // their numbers.
        let legacy = cost_for_turn("claude-sonnet-4-5", 0, 0, 0, 1_000_000);
        let v2 = cost_for_turn_v2("claude-sonnet-4-5", 0, 0, 0, 1_000_000, 0, 0, 0);
        assert_eq!(legacy, v2);
    }

    #[test]
    fn openai_strips_date_suffix() {
        let p = pricing_for("gpt-5-20250805").unwrap();
        assert_eq!(p.input_per_mtok, 15.0);
    }

    #[test]
    fn openai_strips_provider_prefix() {
        let p = pricing_for("openai/gpt-4o-mini").unwrap();
        assert_eq!(p.input_per_mtok, 0.15);
    }

    #[test]
    fn longest_prefix_wins() {
        let p = pricing_for("gpt-4.1-mini-2025").unwrap();
        assert_eq!(p.input_per_mtok, 0.40);
    }

    #[test]
    fn cost_matches_published_rates() {
        let cost = cost_for_turn("claude-sonnet-4-5", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 18.0).abs() < 1e-9);
    }

    #[test]
    fn cost_zero_for_unknown_model() {
        assert_eq!(cost_for_turn("never-shipped-9000", 1_000_000, 0, 0, 0), 0.0);
    }
}
