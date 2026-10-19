import { BACKSPACE_SENDS, TELNET_ENTER_SENDS } from "../terminalSessionOptions.js";
import { isSerialProtocol, isTelnetProtocol } from "../connectionProtocols.js";
import { bytesToBase64, stringToBytes } from "./addons/trzsz/bytes.js";

export function binaryStringToBase64(value) {
  return bytesToBase64(stringToBytes(String(value || "")));
}

export function shouldSendBackspaceAsBs(event, connection, sessionId) {
  return (
    event.type === "keydown" &&
    event.key === "Backspace" &&
    !!sessionId &&
    // 默认 BS：只有显式选择 DEL(^?) 的会话才放行为 xterm 原生 \x7f。
    // 历史 profile 里的 "AUTO" 等未知值同样落到默认 BS。
    connection?.backspaceSends !== BACKSPACE_SENDS.DEL &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey
  );
}

// CR NUL 是 RFC 854 对"裸 CR"的合法编码：整链只有一个终止符，把 CR 与 LF 各算
// 一次提交的设备链路（嵌套 telnet 的中间跳等）不会把一次回车执行为两次。
// SecureCRT 与 BSD/netkit telnet 的 telnet 回车默认同样发 CR NUL。
const TELNET_CR_NUL = "\r\0";

// 串口线上没有 NVT 编码层，回车惯例就是单个 CR（TeraTerm 等串口工具的默认发送）。
const SERIAL_CR = "\r";

/**
 * 把输入里的每个换行写法折叠为会话所在链路的单个终止符，覆盖按键、粘贴、
 * 快捷按钮与脚本等所有走 onData 的来源：
 * - telnet：默认 CR NUL（显式 telnetEnterSends=CRLF 时不改写，由后端引擎编成
 *   NVT 标准 CR LF）；后端引擎把 \r\0 按协商状态编成 CR NUL（NVT）或透传
 *   （BINARY），一次回车在全链路上只有一个终止符。
 * - serial：折叠为单个 CR。
 * - 其余协议（SSH 等）保持原样，由对端 PTY/桌面自行解释。
 */
export function resolveTerminalEnterInput(data, connection) {
  const newline = sessionNewlineEncoding(connection);
  if (newline === null) return data;
  if (typeof data !== "string" || (!data.includes("\r") && !data.includes("\n"))) return data;
  return collapseNewlineSpellings(data, newline);
}

function sessionNewlineEncoding(connection) {
  const protocol = connection?.protocol;
  if (isTelnetProtocol(protocol)) {
    return connection?.telnetEnterSends === TELNET_ENTER_SENDS.CRLF ? null : TELNET_CR_NUL;
  }
  if (isSerialProtocol(protocol)) {
    return SERIAL_CR;
  }
  return null;
}

// 换行的三种写法（CR LF / 孤 CR / 孤 LF）与已是 CR NUL 的输入统一折叠；
// 折叠结果再入函数是幂等的（CR NUL 不会叠加出第二个 NUL）。
function collapseNewlineSpellings(data, newline) {
  let mapped = "";
  for (let index = 0; index < data.length; index += 1) {
    const char = data[index];
    const next = data[index + 1];
    if (char === "\r" && (next === "\n" || next === "\0")) {
      mapped += newline;
      index += 1;
    } else if (char === "\r" || char === "\n") {
      mapped += newline;
    } else {
      mapped += char;
    }
  }
  return mapped;
}
