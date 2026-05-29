//! Token-count and cost estimation for the status bar (Phase K).
//!
//! No real tokenizer ships with the CLI — that would pull in megabytes of
//! BPE vocab files and a `tiktoken`-style dependency. Instead we use the
//! venerable "≈ 4 chars per token" heuristic. The status bar prefixes
//! everything with `~` to make the imprecision explicit.
//!
//! Pricing is a small hand-curated table covering the providers we ship
//! out of the box. Free / local providers (`ollama`, `lm-studio`,
//! `claude-code`) return `None` from [`lookup`] so the status bar
//! displays token count but no dollar figure.
//!
//! Prices are USD per **million** tokens. Update these when providers
//! change their rates — last refreshed: 2026-05.

/// Input + output rate for a `(provider, model)` pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    /// USD per 1M input tokens.
    pub input_per_million: f64,
    /// USD per 1M output tokens.
    pub output_per_million: f64,
}

/// Look up pricing for a `(provider, model)` pair. Returns `None` for
/// local / free providers or unknown models — callers should fall back
/// to showing token counts only.
pub fn lookup(provider: &str, model: &str) -> Option<Pricing> {
    match provider {
        "anthropic" => anthropic_pricing(model),
        "openai" => openai_pricing(model),
        // OpenRouter aggregates many models with per-model prices — we
        // would need a live API call to be accurate. Skip until a
        // dedicated pricing fetcher lands.
        "openrouter" => None,
        // Local / no-key providers.
        "ollama" | "lm-studio" | "lm_studio" | "claude-code" | "claude_code" => None,
        _ => None,
    }
}

fn anthropic_pricing(model: &str) -> Option<Pricing> {
    // Claude 4.x family, prices as of 2026-05.
    if model.starts_with("claude-opus-4") {
        return Some(Pricing {
            input_per_million: 15.0,
            output_per_million: 75.0,
        });
    }
    if model.starts_with("claude-sonnet-4") {
        return Some(Pricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
        });
    }
    if model.starts_with("claude-haiku-4") {
        return Some(Pricing {
            input_per_million: 0.80,
            output_per_million: 4.00,
        });
    }
    // Claude 3.x family (legacy but still callable).
    if model.starts_with("claude-3-5-sonnet") || model.starts_with("claude-3-7-sonnet") {
        return Some(Pricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
        });
    }
    if model.starts_with("claude-3-opus") {
        return Some(Pricing {
            input_per_million: 15.0,
            output_per_million: 75.0,
        });
    }
    if model.starts_with("claude-3-haiku") {
        return Some(Pricing {
            input_per_million: 0.25,
            output_per_million: 1.25,
        });
    }
    None
}

fn openai_pricing(model: &str) -> Option<Pricing> {
    // Prices as of 2026-05.
    if model.starts_with("gpt-4o-mini") {
        return Some(Pricing {
            input_per_million: 0.15,
            output_per_million: 0.60,
        });
    }
    if model.starts_with("gpt-4o") {
        return Some(Pricing {
            input_per_million: 2.50,
            output_per_million: 10.0,
        });
    }
    if model.starts_with("o1-mini") {
        return Some(Pricing {
            input_per_million: 3.0,
            output_per_million: 12.0,
        });
    }
    if model.starts_with("o1") {
        return Some(Pricing {
            input_per_million: 15.0,
            output_per_million: 60.0,
        });
    }
    if model.starts_with("gpt-4-turbo") {
        return Some(Pricing {
            input_per_million: 10.0,
            output_per_million: 30.0,
        });
    }
    if model.starts_with("gpt-4") {
        return Some(Pricing {
            input_per_million: 30.0,
            output_per_million: 60.0,
        });
    }
    if model.starts_with("gpt-3.5") {
        return Some(Pricing {
            input_per_million: 0.50,
            output_per_million: 1.50,
        });
    }
    None
}

/// A small curated list of well-known model ids per provider, for
/// `/model` argument autocomplete (Phase EE). Not exhaustive — the
/// user can always type any id the provider accepts.
pub fn known_models(provider: &str) -> &'static [&'static str] {
    match provider {
        "anthropic" => &[
            "claude-opus-4-5-20250929",
            "claude-sonnet-4-5-20250929",
            "claude-haiku-4-5-20250929",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
            "claude-3-opus-20240229",
        ],
        "openai" => &[
            "gpt-4o",
            "gpt-4o-mini",
            "o1",
            "o1-mini",
            "gpt-4-turbo",
            "gpt-3.5-turbo",
        ],
        "openrouter" => &[
            "anthropic/claude-sonnet-4.5",
            "openai/gpt-4o",
            "google/gemini-2.5-pro",
            "meta-llama/llama-3.3-70b-instruct",
        ],
        "ollama" => &[
            "llama3.2",
            "llama3.2-vision",
            "llava",
            "qwen2.5-coder",
            "deepseek-r1",
        ],
        "lm-studio" | "lm_studio" => &[],
        _ => &[],
    }
}

/// Estimate token count from a UTF-8 string. Uses the standard 4
/// chars-per-token rule of thumb. Always rounds up — empty input
/// returns 0.
pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4)
    }
}

/// Compute the USD cost of an `(input, output)` token pair under a
/// given [`Pricing`].
pub fn estimate_cost(p: Pricing, input_tokens: u64, output_tokens: u64) -> f64 {
    let in_cost = (input_tokens as f64 / 1_000_000.0) * p.input_per_million;
    let out_cost = (output_tokens as f64 / 1_000_000.0) * p.output_per_million;
    in_cost + out_cost
}

/// Format a token count for the status bar (`1234` → `1.2k`, `1500000`
/// → `1.5M`).
pub fn format_tokens(n: u64) -> String {
    if n < 1_000 {
        format!("{n}")
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

/// Format a USD cost for the status bar. Sub-cent costs collapse to
/// `<$0.01`, anything else gets 3-significant-digit display.
pub fn format_cost(usd: f64) -> String {
    if usd < 0.01 {
        "<$0.01".to_string()
    } else if usd < 1.0 {
        format!("${usd:.3}")
    } else {
        format!("${usd:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_pricing_for_claude_opus() {
        let p = lookup("anthropic", "claude-opus-4-5-20251201").expect("priced");
        assert_eq!(p.input_per_million, 15.0);
        assert_eq!(p.output_per_million, 75.0);
    }

    #[test]
    fn lookup_returns_pricing_for_sonnet_legacy() {
        let p = lookup("anthropic", "claude-3-5-sonnet-20241022").expect("priced");
        assert_eq!(p.input_per_million, 3.0);
    }

    #[test]
    fn lookup_returns_none_for_local_providers() {
        assert!(lookup("ollama", "llama3.1:8b").is_none());
        assert!(lookup("lm-studio", "whatever").is_none());
        assert!(lookup("claude-code", "claude-sonnet-4").is_none());
    }

    #[test]
    fn estimate_tokens_uses_four_char_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("x"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"a".repeat(400)), 100);
    }

    #[test]
    fn estimate_cost_combines_input_and_output() {
        let p = Pricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
        };
        let cost = estimate_cost(p, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 1e-9);
    }

    #[test]
    fn format_tokens_scales_with_magnitude() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_500), "1.5k");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn format_cost_collapses_sub_cent() {
        assert_eq!(format_cost(0.0001), "<$0.01");
        assert_eq!(format_cost(0.123), "$0.123");
        assert_eq!(format_cost(12.34), "$12.34");
    }
}
