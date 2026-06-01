//! Secret storage backed by the OS keyring — macOS Keychain, Windows
//! Credential Manager, Linux Secret Service (GNOME Keyring / KWallet via
//! a pure-Rust D-Bus client) — with graceful degradation when no backend
//! is reachable (headless servers, CI).
//!
//! Nothing here ever panics or surfaces a hard error to a read path:
//! [`get`] returns `None` when the keyring is missing or empty, so the
//! caller can fall back to an environment variable or `config.toml`.
//! Only [`set`] reports failure, so the setup wizard can offer a fallback.
//!
//! Runtime key-resolution order (see `resolve_key` in `main.rs`): an
//! explicit value in `config.toml` wins, then the keyring, then the
//! environment variable.

use keyring::Entry;

/// Keyring service namespace shared by every Aonyx secret. Individual
/// secrets are addressed by a stable key, e.g. `"anthropic_api_key"` or
/// `"telegram_bot_token"`.
const SERVICE: &str = "aonyx-agent";

/// Read a secret from the OS keyring. Returns `None` when there is no
/// entry *or* no usable backend — never an error — so callers fall back
/// silently to env / config.
pub fn get(key: &str) -> Option<String> {
    Entry::new(SERVICE, key).ok()?.get_password().ok()
}

/// Store a secret in the OS keyring. Returns a human-readable error when
/// the backend is unavailable so the caller can offer a plaintext / env
/// fallback.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    Entry::new(SERVICE, key)
        .map_err(|e| e.to_string())?
        .set_password(value)
        .map_err(|e| e.to_string())
}
