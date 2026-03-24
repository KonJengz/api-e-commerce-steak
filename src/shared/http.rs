use std::net::SocketAddr;

use axum::http::HeaderMap;

/// Extract the real client IP from request headers or fall back to the socket address.
/// Checks `X-Forwarded-For` first, then `X-Real-Ip`, then the connection address.
pub fn client_ip(headers: &HeaderMap, addr: SocketAddr, trust_proxy_headers: bool) -> String {
    if !trust_proxy_headers {
        return addr.ip().to_string();
    }

    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| addr.ip().to_string())
}
