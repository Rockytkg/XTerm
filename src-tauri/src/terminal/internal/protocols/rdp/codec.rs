//! RDP 回环桥帧协议编解码（自定义二进制，全部小端序）。
//!
//! 每条 WebSocket binary message 恰好承载一条消息，首字节为消息类型。
//! 该布局是前后端共享契约，任何改动必须与前端实现逐字节一致。
//!
//! S→C（后端 → 前端）：
//! - `0x01 Hello {w:u16,h:u16}`：客户端接入即发，携带当前桌面尺寸。
//! - `0x02 Frame {count:u16, [x:u16,y:u16,w:u16,h:u16, RGBA w*h*4]×count}`：脏矩形批量更新。
//! - `0x03 Resize {w:u16,h:u16}`：远端桌面尺寸变化（动态分辨率生效后）。
//! - `0x04 ClipboardText {len:u32, utf8}`：远端剪贴板文本 → 本地。
//! - `0x05 Disconnect {len:u16, utf8 reason}`：会话结束原因。
//!
//! C→S（前端 → 后端）：
//! - `0x01 PointerMove {x:u16,y:u16}`
//! - `0x02 PointerButton {button:u8(0左1右2中), pressed:u8}`
//! - `0x03 Wheel {delta:i16}`：正=向上，单位为"格"，后端转成 WheelRotations。
//! - `0x04 Key {scancode:u16, pressed:u8, extended:u8}`
//! - `0x05 ResizeRequest {w:u16,h:u16}`
//! - `0x06 ClipboardText {len:u32, utf8}`：本地剪贴板文本 → 远端。

/// 单个脏矩形及其 RGBA 像素（行优先、每像素 4 字节、无行填充）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirtyRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

/// 后端发往前端的消息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ServerMessage {
    Hello { width: u16, height: u16 },
    Frame { rects: Vec<DirtyRect> },
    Resize { width: u16, height: u16 },
    ClipboardText(String),
    Disconnect(String),
}

/// 前端发往后端的消息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ClientMessage {
    PointerMove {
        x: u16,
        y: u16,
    },
    PointerButton {
        button: u8,
        pressed: bool,
    },
    Wheel {
        delta: i16,
    },
    Key {
        scancode: u16,
        pressed: bool,
        extended: bool,
    },
    ResizeRequest {
        width: u16,
        height: u16,
    },
    ClipboardText(String),
}

impl ServerMessage {
    pub(crate) fn encode(&self) -> Vec<u8> {
        match self {
            Self::Hello { width, height } => {
                let mut out = Vec::with_capacity(5);
                out.push(0x01);
                out.extend_from_slice(&width.to_le_bytes());
                out.extend_from_slice(&height.to_le_bytes());
                out
            }
            Self::Frame { rects } => encode_frame(rects),
            Self::Resize { width, height } => {
                let mut out = Vec::with_capacity(5);
                out.push(0x03);
                out.extend_from_slice(&width.to_le_bytes());
                out.extend_from_slice(&height.to_le_bytes());
                out
            }
            Self::ClipboardText(text) => {
                let bytes = text.as_bytes();
                let mut out = Vec::with_capacity(5 + bytes.len());
                out.push(0x04);
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(bytes);
                out
            }
            Self::Disconnect(reason) => {
                let bytes = reason.as_bytes();
                // reason 长度用 u16 足够：断连原因是人读的短文本。
                let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
                let mut out = Vec::with_capacity(3 + usize::from(len));
                out.push(0x05);
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&bytes[..usize::from(len)]);
                out
            }
        }
    }
}

fn encode_frame(rects: &[DirtyRect]) -> Vec<u8> {
    debug_assert!(
        rects.len() <= usize::from(u16::MAX),
        "frame message rect count must fit u16"
    );
    let payload_len: usize = rects.iter().map(|rect| 8 + rect.pixels.len()).sum();
    let mut out = Vec::with_capacity(3 + payload_len);
    out.push(0x02);
    out.extend_from_slice(&(rects.len() as u16).to_le_bytes());
    for rect in rects {
        debug_assert_eq!(
            rect.pixels.len(),
            usize::from(rect.width) * usize::from(rect.height) * 4,
            "dirty rect pixel payload must be tightly packed RGBA"
        );
        out.extend_from_slice(&rect.x.to_le_bytes());
        out.extend_from_slice(&rect.y.to_le_bytes());
        out.extend_from_slice(&rect.width.to_le_bytes());
        out.extend_from_slice(&rect.height.to_le_bytes());
        out.extend_from_slice(&rect.pixels);
    }
    out
}

impl ClientMessage {
    /// 解码一条 WS binary message。长度不符或 UTF-8 非法时返回错误描述，
    /// 调用方记录日志后丢弃该消息（桥协议错误不等于会话失败）。
    pub(crate) fn decode(data: &[u8]) -> Result<Self, String> {
        let Some((&tag, body)) = data.split_first() else {
            return Err("empty bridge message".to_string());
        };
        match tag {
            0x01 => {
                let body = expect_len(body, 4, "PointerMove")?;
                Ok(Self::PointerMove {
                    x: u16::from_le_bytes([body[0], body[1]]),
                    y: u16::from_le_bytes([body[2], body[3]]),
                })
            }
            0x02 => {
                let body = expect_len(body, 2, "PointerButton")?;
                Ok(Self::PointerButton {
                    button: body[0],
                    pressed: body[1] != 0,
                })
            }
            0x03 => {
                let body = expect_len(body, 2, "Wheel")?;
                Ok(Self::Wheel {
                    delta: i16::from_le_bytes([body[0], body[1]]),
                })
            }
            0x04 => {
                let body = expect_len(body, 4, "Key")?;
                Ok(Self::Key {
                    scancode: u16::from_le_bytes([body[0], body[1]]),
                    pressed: body[2] != 0,
                    extended: body[3] != 0,
                })
            }
            0x05 => {
                let body = expect_len(body, 4, "ResizeRequest")?;
                Ok(Self::ResizeRequest {
                    width: u16::from_le_bytes([body[0], body[1]]),
                    height: u16::from_le_bytes([body[2], body[3]]),
                })
            }
            0x06 => {
                if body.len() < 4 {
                    return Err("ClipboardText message too short".to_string());
                }
                let len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
                let payload = body.get(4..4 + len).ok_or_else(|| {
                    "ClipboardText payload shorter than declared length".to_string()
                })?;
                let text = std::str::from_utf8(payload).map_err(|error| {
                    format!("ClipboardText payload is not valid UTF-8: {error}")
                })?;
                Ok(Self::ClipboardText(text.to_string()))
            }
            other => Err(format!("unknown bridge message type 0x{other:02x}")),
        }
    }
}

fn expect_len<'a>(body: &'a [u8], expected: usize, name: &str) -> Result<&'a [u8], String> {
    if body.len() == expected {
        Ok(body)
    } else {
        Err(format!(
            "{name} message expects {expected} bytes, got {}",
            body.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientMessage, DirtyRect, ServerMessage};

    #[test]
    fn hello_roundtrip_layout() {
        let bytes = ServerMessage::Hello {
            width: 1920,
            height: 1080,
        }
        .encode();
        assert_eq!(bytes, [0x01, 0x80, 0x07, 0x38, 0x04]);
    }

    #[test]
    fn frame_layout_is_le_and_tightly_packed() {
        let rect = DirtyRect {
            x: 1,
            y: 2,
            width: 2,
            height: 1,
            pixels: vec![10, 20, 30, 40, 50, 60, 70, 80],
        };
        let bytes = ServerMessage::Frame {
            rects: vec![rect.clone()],
        }
        .encode();
        assert_eq!(
            bytes,
            [
                0x02, // tag
                0x01, 0x00, // count
                0x01, 0x00, // x
                0x02, 0x00, // y
                0x02, 0x00, // w
                0x01, 0x00, // h
                10, 20, 30, 40, 50, 60, 70, 80,
            ]
        );
    }

    #[test]
    fn resize_and_clipboard_layout() {
        let bytes = ServerMessage::Resize {
            width: 800,
            height: 600,
        }
        .encode();
        assert_eq!(bytes, [0x03, 0x20, 0x03, 0x58, 0x02]);

        let bytes = ServerMessage::ClipboardText("héllo".to_string()).encode();
        assert_eq!(bytes[0], 0x04);
        assert_eq!(u32::from_le_bytes(bytes[1..5].try_into().unwrap()), 6);
        assert_eq!(&bytes[5..], "héllo".as_bytes());

        let bytes = ServerMessage::Disconnect("bye".to_string()).encode();
        assert_eq!(bytes, [0x05, 0x03, 0x00, b'b', b'y', b'e']);
    }

    #[test]
    fn disconnect_reason_is_truncated_to_u16() {
        let reason = "x".repeat(usize::from(u16::MAX) + 10);
        let bytes = ServerMessage::Disconnect(reason).encode();
        assert_eq!(u16::from_le_bytes([bytes[1], bytes[2]]), u16::MAX);
        assert_eq!(bytes.len(), 3 + usize::from(u16::MAX));
    }

    #[test]
    fn client_messages_decode() {
        assert_eq!(
            ClientMessage::decode(&[0x01, 0x05, 0x00, 0x0a, 0x00]),
            Ok(ClientMessage::PointerMove { x: 5, y: 10 })
        );
        assert_eq!(
            ClientMessage::decode(&[0x02, 0x01, 0x01]),
            Ok(ClientMessage::PointerButton {
                button: 1,
                pressed: true
            })
        );
        assert_eq!(
            ClientMessage::decode(&[0x03, 0x78, 0xff]),
            Ok(ClientMessage::Wheel { delta: -136 })
        );
        assert_eq!(
            ClientMessage::decode(&[0x04, 0x1c, 0x00, 0x01, 0x00]),
            Ok(ClientMessage::Key {
                scancode: 0x1c,
                pressed: true,
                extended: false
            })
        );
        assert_eq!(
            ClientMessage::decode(&[0x05, 0x00, 0x04, 0x00, 0x03]),
            Ok(ClientMessage::ResizeRequest {
                width: 1024,
                height: 768
            })
        );
        let text = "clipboard";
        let mut msg = vec![0x06];
        msg.extend_from_slice(&(text.len() as u32).to_le_bytes());
        msg.extend_from_slice(text.as_bytes());
        assert_eq!(
            ClientMessage::decode(&msg),
            Ok(ClientMessage::ClipboardText(text.to_string()))
        );
    }

    #[test]
    fn client_decode_rejects_malformed_messages() {
        assert!(ClientMessage::decode(&[]).is_err());
        assert!(ClientMessage::decode(&[0x01, 0x00]).is_err());
        assert!(ClientMessage::decode(&[0xff]).is_err());
        // 声明长度超过实际负载
        assert!(ClientMessage::decode(&[0x06, 0x10, 0x00, 0x00, 0x00, b'a']).is_err());
        // 负载不是合法 UTF-8
        assert!(ClientMessage::decode(&[0x06, 0x01, 0x00, 0x00, 0x00, 0xff]).is_err());
    }
}
