// RDP 本地桥（WS binary，全小端）帧编解码纯函数。一条 WebSocket message = 一条消息。
// 帧布局与后端桥实现一一对应；任何字段变更必须两端同步。

export const RDP_SERVER_MESSAGE = Object.freeze({
  HELLO: 0x01,
  FRAME: 0x02,
  RESIZE: 0x03,
  CLIPBOARD_TEXT: 0x04,
  DISCONNECT: 0x05,
});

export const RDP_CLIENT_MESSAGE = Object.freeze({
  POINTER_MOVE: 0x01,
  POINTER_BUTTON: 0x02,
  WHEEL: 0x03,
  KEY: 0x04,
  RESIZE_REQUEST: 0x05,
  CLIPBOARD_TEXT: 0x06,
});

const textDecoder = new TextDecoder();
const textEncoder = new TextEncoder();

function decodeUtf8(view, offset, length) {
  return textDecoder.decode(new Uint8Array(view.buffer, view.byteOffset + offset, length));
}

// 解析一条服务端消息。返回 { type, ... }；无法识别或长度不足返回 null，
// 由调用方忽略（桥协议不允许半截消息，容错即可）。
export function decodeServerMessage(buffer) {
  const view = new DataView(buffer);
  if (view.byteLength < 1) return null;
  const type = view.getUint8(0);

  switch (type) {
    case RDP_SERVER_MESSAGE.HELLO: {
      if (view.byteLength < 5) return null;
      return { type, width: view.getUint16(1, true), height: view.getUint16(3, true) };
    }
    case RDP_SERVER_MESSAGE.FRAME: {
      if (view.byteLength < 3) return null;
      const count = view.getUint16(1, true);
      const rects = [];
      let offset = 3;
      for (let index = 0; index < count; index += 1) {
        if (view.byteLength < offset + 8) return null;
        const x = view.getUint16(offset, true);
        const y = view.getUint16(offset + 2, true);
        const width = view.getUint16(offset + 4, true);
        const height = view.getUint16(offset + 6, true);
        offset += 8;
        const byteLength = width * height * 4;
        if (view.byteLength < offset + byteLength) return null;
        // 零拷贝视图直接喂给 ImageData，避免大帧再复制一遍。
        const pixels = new Uint8ClampedArray(view.buffer, view.byteOffset + offset, byteLength);
        offset += byteLength;
        rects.push({ x, y, width, height, pixels });
      }
      return { type, rects };
    }
    case RDP_SERVER_MESSAGE.RESIZE: {
      if (view.byteLength < 5) return null;
      return { type, width: view.getUint16(1, true), height: view.getUint16(3, true) };
    }
    case RDP_SERVER_MESSAGE.CLIPBOARD_TEXT: {
      if (view.byteLength < 5) return null;
      const length = view.getUint32(1, true);
      if (view.byteLength < 5 + length) return null;
      return { type, text: decodeUtf8(view, 5, length) };
    }
    case RDP_SERVER_MESSAGE.DISCONNECT: {
      if (view.byteLength < 3) return null;
      const length = view.getUint16(1, true);
      if (view.byteLength < 3 + length) return null;
      return { type, reason: decodeUtf8(view, 3, length) };
    }
    default:
      return null;
  }
}

export function encodePointerMove(x, y) {
  const buffer = new ArrayBuffer(5);
  const view = new DataView(buffer);
  view.setUint8(0, RDP_CLIENT_MESSAGE.POINTER_MOVE);
  view.setUint16(1, x, true);
  view.setUint16(3, y, true);
  return buffer;
}

export function encodePointerButton(button, pressed) {
  const buffer = new ArrayBuffer(3);
  const view = new DataView(buffer);
  view.setUint8(0, RDP_CLIENT_MESSAGE.POINTER_BUTTON);
  view.setUint8(1, button);
  view.setUint8(2, pressed ? 1 : 0);
  return buffer;
}

// delta 单位"格"（一次滚轮刻度），正 = 向上滚动。
export function encodeWheel(delta) {
  const buffer = new ArrayBuffer(3);
  const view = new DataView(buffer);
  view.setUint8(0, RDP_CLIENT_MESSAGE.WHEEL);
  view.setInt16(1, delta, true);
  return buffer;
}

export function encodeKey(scancode, pressed, extended) {
  const buffer = new ArrayBuffer(5);
  const view = new DataView(buffer);
  view.setUint8(0, RDP_CLIENT_MESSAGE.KEY);
  view.setUint16(1, scancode, true);
  view.setUint8(3, pressed ? 1 : 0);
  view.setUint8(4, extended ? 1 : 0);
  return buffer;
}

export function encodeResizeRequest(width, height) {
  const buffer = new ArrayBuffer(5);
  const view = new DataView(buffer);
  view.setUint8(0, RDP_CLIENT_MESSAGE.RESIZE_REQUEST);
  view.setUint16(1, width, true);
  view.setUint16(3, height, true);
  return buffer;
}

export function encodeClipboardText(text) {
  const bytes = textEncoder.encode(String(text ?? ""));
  const buffer = new ArrayBuffer(5 + bytes.length);
  const view = new DataView(buffer);
  view.setUint8(0, RDP_CLIENT_MESSAGE.CLIPBOARD_TEXT);
  view.setUint32(1, bytes.length, true);
  new Uint8Array(buffer, 5).set(bytes);
  return buffer;
}
