import assert from "node:assert/strict";
import test from "node:test";
import {
  buildConnectionProfile,
  createProtocolDraft,
} from "../src/components/connection-dialog/connectionDialogModel.js";
import { mergeConnectionProfileOptions } from "../src/utils/connectionProfileOptions.js";
import { resolveTerminalEnterInput } from "../src/utils/terminal/inputRouting.js";
import { TELNET_ENTER_SENDS } from "../src/utils/terminalSessionOptions.js";

function telnetProfileBase() {
  return {
    id: "conn-1",
    protocol: "telnet",
    savedCredentialId: "",
    form: {
      name: "switch",
      host: "192.0.2.1",
      port: 23,
      user: "admin",
      terminalType: "xterm-256color",
      encoding: "auto",
      backspaceSends: "BS",
    },
  };
}

test("telnet drafts default to CR enter", () => {
  assert.equal(createProtocolDraft("telnet").telnetEnterSends, TELNET_ENTER_SENDS.CR);
  assert.equal(createProtocolDraft("ssh").telnetEnterSends, TELNET_ENTER_SENDS.CR);
});

test("profiles omit the CR default and persist explicit CR LF", () => {
  const base = telnetProfileBase();
  const defaultProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, telnetEnterSends: TELNET_ENTER_SENDS.CR },
  });
  assert.equal(defaultProfile.options.telnetEnterSends, undefined);

  const crlfProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, telnetEnterSends: TELNET_ENTER_SENDS.CRLF },
  });
  assert.equal(crlfProfile.options.telnetEnterSends, TELNET_ENTER_SENDS.CRLF);
});

test("option patches round-trip telnetEnterSends through the whitelist", () => {
  const profile = { options: { backspaceSends: "DEL" } };
  const updated = mergeConnectionProfileOptions(profile, {
    telnetEnterSends: TELNET_ENTER_SENDS.CRLF,
  });
  assert.deepEqual(updated.options, {
    backspaceSends: "DEL",
    telnetEnterSends: TELNET_ENTER_SENDS.CRLF,
  });

  const cleared = mergeConnectionProfileOptions(updated, { telnetEnterSends: undefined });
  assert.deepEqual(cleared.options, { backspaceSends: "DEL" });
});

test("telnet enter input defaults to CR NUL and honors explicit CR LF", () => {
  // 默认（未配置）与显式 CR 均改写为 CR NUL；未知历史值归一到默认。
  assert.equal(resolveTerminalEnterInput("\r", { protocol: "telnet" }), "\r\0");
  assert.equal(
    resolveTerminalEnterInput("\r", {
      protocol: "telnet",
      telnetEnterSends: TELNET_ENTER_SENDS.CR,
    }),
    "\r\0",
  );
  assert.equal(
    resolveTerminalEnterInput("\r", { protocol: "telnet", telnetEnterSends: "AUTO" }),
    "\r\0",
  );
  // 显式 CR LF 保留 NVT 标准行为（引擎再把 \r 编成 CR LF）。
  assert.equal(
    resolveTerminalEnterInput("\r", {
      protocol: "telnet",
      telnetEnterSends: TELNET_ENTER_SENDS.CRLF,
    }),
    "\r",
  );
  // 非 telnet/serial 协议（SSH 等）即使误配该选项也不改写。
  assert.equal(
    resolveTerminalEnterInput("\r", { protocol: "ssh", telnetEnterSends: TELNET_ENTER_SENDS.CR }),
    "\r",
  );
});

test("default CR mode collapses every newline spelling to a single CR NUL", () => {
  const connection = { protocol: "telnet" };
  // 粘贴/快捷按钮/脚本输入与按键走同一条 onData 链路，同样只能有一个终止符。
  assert.equal(resolveTerminalEnterInput("display version\r\n", connection), "display version\r\0");
  assert.equal(resolveTerminalEnterInput("a\rb\nc", connection), "a\r\0b\r\0c");
  // 已是 CR NUL 的输入不再叠加 NUL（折叠幂等）。
  assert.equal(resolveTerminalEnterInput("x\r\0", connection), "x\r\0");
  assert.equal(resolveTerminalEnterInput("plain", connection), "plain");
});

test("serial enter input collapses newline spellings to a single CR", () => {
  const connection = { protocol: "serial" };
  // 串口没有 NVT 层：Enter 本就是单个 CR，粘贴的 CRLF/LF 折叠后一行只提交一次。
  assert.equal(resolveTerminalEnterInput("\r", connection), "\r");
  assert.equal(resolveTerminalEnterInput("display version\r\n", connection), "display version\r");
  assert.equal(resolveTerminalEnterInput("a\rb\nc", connection), "a\rb\rc");
  // 串口不使用 CR NUL 编码：已有的 NUL 一并去掉。
  assert.equal(resolveTerminalEnterInput("x\r\0", connection), "x\r");
  assert.equal(resolveTerminalEnterInput("plain", connection), "plain");
});
