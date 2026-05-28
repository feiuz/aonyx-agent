//! LM Studio provider — OpenAI-compatible with a user-configurable base URL.
//!
//! LM Studio commonly exposes its server at `http://localhost:1234` without an
//! API key. We pass an empty key so the `Authorization` header is omitted.

use crate::openai_compat::OpenAiCompatProvider;

/// LM Studio's default local server URL.
pub const LM_STUDIO_DEFAULT_BASE_URL: &str = "http://localhost:1234";

/// Build an LM Studio provider against `base_url`.
pub fn provider(base_url: impl Into<String>) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new("lm-studio", String::new(), base_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::LlmProvider;

    #[test]
    fn provider_name_is_lm_studio() {
        let p = provider(LM_STUDIO_DEFAULT_BASE_URL);
        assert_eq!(p.name(), "lm-studio");
        assert_eq!(p.base_url(), LM_STUDIO_DEFAULT_BASE_URL);
    }
}
