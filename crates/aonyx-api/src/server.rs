//! Bind + serve the API router.

use aonyx_core::{AonyxError, Result};

use crate::build_router;
use crate::state::ApiState;

/// Bind `addr` (e.g. `127.0.0.1:8788`) and serve the full API until the
/// process is stopped. The binary wires this from `aonyx serve api`.
pub async fn serve(state: ApiState, addr: &str) -> Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AonyxError::Adapter(format!("bind {addr}: {e}")))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| AonyxError::Adapter(format!("serve: {e}")))?;
    Ok(())
}
