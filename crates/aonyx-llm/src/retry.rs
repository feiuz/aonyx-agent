//! Shared HTTP retry/backoff for LLM providers (Phase RR).
//!
//! Transient failures — HTTP 429 (rate limit), 5xx (server errors), and
//! network errors (timeouts, resets) — are retried with exponential
//! backoff. 4xx other than 429 are treated as fatal (bad request, auth)
//! and returned immediately. The decision functions are pure so they can
//! be unit-tested without a live server; the send loop is thin glue.

use std::time::Duration;

use aonyx_core::{AonyxError, Result};

/// Retry configuration. [`Default`] = 3 retries, 500 ms base backoff
/// (so waits of 500 ms, 1 s, 2 s before the 1st/2nd/3rd retry).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Maximum number of *retries* after the initial attempt.
    pub max_retries: u32,
    /// Base backoff in milliseconds, doubled each attempt.
    pub base_backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_backoff_ms: 500,
        }
    }
}

/// Whether an HTTP status code warrants a retry: 429 (rate limited) or
/// any 5xx (server-side). Everything else — including 4xx like 400/401/
/// 403/404 — is fatal and must not be retried.
pub fn is_retriable_status(code: u16) -> bool {
    code == 429 || (500..=599).contains(&code)
}

/// Backoff delay before the `attempt`-th retry (1-indexed): an
/// exponential `base * 2^(attempt-1)`, capped at 30 s to bound the wait.
pub fn backoff_ms(policy: RetryPolicy, attempt: u32) -> u64 {
    const CAP_MS: u64 = 30_000;
    let shift = attempt.saturating_sub(1).min(16);
    policy
        .base_backoff_ms
        .saturating_mul(1u64 << shift)
        .min(CAP_MS)
}

/// Send a request, retrying transient failures per `policy` (Phase RR).
///
/// The builder must be cloneable (a non-streaming body, e.g. a `String`)
/// — every provider here posts a serialized JSON string, so it is. On a
/// retriable status the response is dropped and the send retried; the
/// final response (success or fatal) is returned for the caller's own
/// status handling. Network errors are retried too, surfacing the last
/// error only after retries are exhausted.
pub async fn send_with_retry(
    builder: reqwest::RequestBuilder,
    policy: RetryPolicy,
    label: &str,
) -> Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        let this = builder.try_clone().ok_or_else(|| {
            AonyxError::Provider(format!("{label}: request body is not retry-cloneable"))
        })?;
        match this.send().await {
            Ok(resp) => {
                if is_retriable_status(resp.status().as_u16()) && attempt < policy.max_retries {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(backoff_ms(policy, attempt))).await;
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                if attempt < policy.max_retries {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(backoff_ms(policy, attempt))).await;
                    continue;
                }
                return Err(AonyxError::Provider(format!("{label} send: {e}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retriable_status_covers_429_and_5xx_only() {
        assert!(is_retriable_status(429));
        assert!(is_retriable_status(500));
        assert!(is_retriable_status(503));
        assert!(is_retriable_status(599));
        // Fatal: success + non-429 client errors.
        assert!(!is_retriable_status(200));
        assert!(!is_retriable_status(400));
        assert!(!is_retriable_status(401));
        assert!(!is_retriable_status(404));
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let p = RetryPolicy {
            max_retries: 10,
            base_backoff_ms: 500,
        };
        assert_eq!(backoff_ms(p, 1), 500); // 500 * 2^0
        assert_eq!(backoff_ms(p, 2), 1000); // 500 * 2^1
        assert_eq!(backoff_ms(p, 3), 2000); // 500 * 2^2
        assert_eq!(backoff_ms(p, 4), 4000);
        // Capped at 30 s no matter how high the attempt.
        assert_eq!(backoff_ms(p, 20), 30_000);
    }

    #[test]
    fn default_policy_is_three_retries() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.base_backoff_ms, 500);
    }
}
