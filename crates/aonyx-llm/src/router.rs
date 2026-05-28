//! Fallback chain router.
//!
//! Wraps an ordered list of providers and tries each one until a stream starts.

// TODO(V1): RouterBuilder + retry policy + circuit breaker.

/// Placeholder router; will gain methods as providers land.
#[derive(Default)]
pub struct Router;

impl Router {
    /// Build an empty router.
    pub fn new() -> Self {
        Self
    }
}
