//! OpenAI Chat Completions provider.
//!
//! Thin factory over [`crate::openai_compat::OpenAiCompatProvider`] pinned to
//! the public OpenAI base URL.

use crate::openai_compat::OpenAiCompatProvider;

/// Public OpenAI API base URL.
pub const OPENAI_BASE_URL: &str = "https://api.openai.com";

/// Build an OpenAI provider with the default base URL.
pub fn provider(api_key: impl Into<String>) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new("openai", api_key, OPENAI_BASE_URL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::LlmProvider;

    #[test]
    fn provider_name_is_openai() {
        let p = provider("sk-test");
        assert_eq!(p.name(), "openai");
        assert_eq!(p.base_url(), OPENAI_BASE_URL);
    }
}
