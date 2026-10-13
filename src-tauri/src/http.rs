//! Shared outbound HTTP client. `reqwest::Client` owns the TLS backend and
//! connection pool, so it is built once and reused; per-call timeouts live on
//! the request builder.

use std::sync::OnceLock;

static SHARED_CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

/// Returns the process-wide HTTP client with the app user-agent. The build
/// error is cached alongside the client so every caller sees the same result
/// without re-initialising the TLS backend.
pub(crate) fn shared_client() -> Result<&'static reqwest::Client, String> {
    SHARED_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(format!("xterm/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}
