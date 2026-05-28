//! OpenRouter provider.
//!
//! OpenAI-compatible, with optional `HTTP-Referer` / `X-Title` headers that
//! OpenRouter recommends so it can attribute and rate-limit calls fairly.

use crate::openai_compat::OpenAiCompatProvider;

/// Public OpenRouter base URL.
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api";

/// Build a minimal OpenRouter provider (no attribution headers).
pub fn provider(api_key: impl Into<String>) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new("openrouter", api_key, OPENROUTER_BASE_URL)
}

/// Build an OpenRouter provider with referer + title attribution.
pub fn provider_with_attribution(
    api_key: impl Into<String>,
    referer: impl Into<String>,
    title: impl Into<String>,
) -> OpenAiCompatProvider {
    provider(api_key)
        .with_header("HTTP-Referer", referer)
        .with_header("X-Title", title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::LlmProvider;

    #[test]
    fn provider_name_is_openrouter() {
        let p = provider("sk-or-test");
        assert_eq!(p.name(), "openrouter");
        assert_eq!(p.base_url(), OPENROUTER_BASE_URL);
    }
}
