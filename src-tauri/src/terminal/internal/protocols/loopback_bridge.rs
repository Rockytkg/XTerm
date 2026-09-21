//! Shared loopback WebSocket bridge plumbing for desktop protocols.
//!
//! 桌面前端（noVNC、RDP 面板）无法直接打开 TCP socket，因此每个桌面会话
//! 都在 127.0.0.1 上起一个 WebSocket listener，把前端拼接到协议侧的 TCP
//! 流上。本模块收敛各协议共用的三件基建：每会话随机 token、回环 listener
//! 的建立与 URL 拼接、以及带 token 校验的 WS acceptor。协议特有的握手回放
//! 与字节拼接仍留在各自协议模块。

use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        http::HeaderValue,
    },
    WebSocketStream,
};

/// A bound loopback bridge endpoint: the listener to accept frontend clients
/// on, the one-shot token gating access, and the URL handed to the frontend.
pub(crate) struct LoopbackBridge {
    pub listener: TcpListener,
    pub token: String,
    pub url: String,
}

/// Binds a loopback listener on an ephemeral port and builds the frontend URL
/// `ws://127.0.0.1:{port}/{path}?token={token}`. token 必须随会话随机生成且
/// 只出现在 URL 里，避免本机其它进程劫持桥接。
pub(crate) async fn bind_loopback_bridge(path: &str) -> std::io::Result<LoopbackBridge> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = bridge_token();
    let url = format!("ws://127.0.0.1:{port}/{path}?token={token}");
    Ok(LoopbackBridge {
        listener,
        token,
        url,
    })
}

/// 16 字节随机 hex 作为桥接访问令牌。
pub(crate) fn bridge_token() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 握手必须在限定时间内完成：本机任意进程都能连上 listener，一个停滞的
/// 握手会冻结接受方的会话主循环（读泵、输入、Close 全部停摆）。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Accepts one frontend WebSocket client, rejecting any request whose query
/// does not carry the session token (HTTP 403). 同时按客户端 offer 协商
/// `Sec-WebSocket-Protocol: binary`，与 noVNC 等前端的默认子协议保持一致。
// ErrorResponse is tungstenite's callback signature; boxing it is not an option.
#[allow(clippy::result_large_err)]
pub(crate) async fn accept_bridge_client(
    stream: TcpStream,
    token: &str,
) -> Result<WebSocketStream<TcpStream>, String> {
    let _ = stream.set_nodelay(true);
    let expected = format!("token={token}");
    let callback = |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
        let query = request.uri().query().unwrap_or("");
        if !query.split('&').any(|part| part == expected) {
            return Err(http_forbidden());
        }
        let offers_binary = request
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(',').any(|entry| entry.trim() == "binary"))
            .unwrap_or(false);
        if offers_binary {
            response
                .headers_mut()
                .append("Sec-WebSocket-Protocol", HeaderValue::from_static("binary"));
        }
        Ok(response)
    };
    let handshake = tokio_tungstenite::accept_hdr_async(stream, callback);
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await {
        Ok(result) => result.map_err(|error| format!("websocket handshake failed: {error}")),
        Err(_) => Err("websocket handshake timed out".to_string()),
    }
}

fn http_forbidden() -> ErrorResponse {
    tokio_tungstenite::tungstenite::http::Response::builder()
        .status(403)
        .body(Some("forbidden".to_string()))
        .expect("static 403 response is valid")
}
