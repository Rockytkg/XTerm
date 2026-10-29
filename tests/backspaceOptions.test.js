import assert from "node:assert/strict";
import test from "node:test";
import {
  buildConnectionProfile,
  createProtocolDraft,
} from "../src/components/connection-dialog/connectionDialogModel.js";
import { shouldSendBackspaceAsBs } from "../src/utils/terminal/inputRouting.js";
import { BACKSPACE_SENDS } from "../src/utils/terminalSessionOptions.js";

test("all protocols default to BS backspace", () => {
  assert.equal(createProtocolDraft("serial").backspaceSends, BACKSPACE_SENDS.BS);
  assert.equal(createProtocolDraft("telnet").backspaceSends, BACKSPACE_SENDS.BS);
  assert.equal(createProtocolDraft("ssh").backspaceSends, BACKSPACE_SENDS.BS);
});

test("profiles omit the BS default and persist explicit DEL", () => {
  const base = {
    id: "conn-1",
    protocol: "serial",
    savedCredentialId: "",
    form: {
      name: "switch",
      serialPort: "COM3",
      baudRate: "9600",
      dataBits: 8,
      flowControl: "none",
      parity: "none",
      stopBits: 1,
      terminalType: "xterm-256color",
      encoding: "auto",
    },
  };
  const defaultProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, backspaceSends: BACKSPACE_SENDS.BS },
  });
  assert.equal(defaultProfile.options.backspaceSends, undefined);

  const delProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, backspaceSends: BACKSPACE_SENDS.DEL },
  });
  assert.equal(delProfile.options.backspaceSends, BACKSPACE_SENDS.DEL);
});

test("backspace sends BS by default; only explicit DEL passes through", () => {
  const event = { type: "keydown", key: "Backspace" };
  assert.equal(shouldSendBackspaceAsBs(event, {}, "session-1"), true);
  assert.equal(
    shouldSendBackspaceAsBs(event, { backspaceSends: BACKSPACE_SENDS.BS }, "session-1"),
    true,
  );
  // 历史 profile 里的 AUTO 等未知值落到默认 BS。
  assert.equal(shouldSendBackspaceAsBs(event, { backspaceSends: "AUTO" }, "session-1"), true);
  assert.equal(
    shouldSendBackspaceAsBs(event, { backspaceSends: BACKSPACE_SENDS.DEL }, "session-1"),
    false,
  );
  // 修饰键组合（如 Ctrl+Backspace 删除整词）不被拦截。
  assert.equal(
    shouldSendBackspaceAsBs(
      { ...event, ctrlKey: true },
      { backspaceSends: BACKSPACE_SENDS.BS },
      "session-1",
    ),
    false,
  );
});

test("backspace is not remapped while an IME composition is active", () => {
  // xterm 6 先调 customKeyEventHandler 再处理 composition：没有守卫时，
  // 中文输入法用退格编辑拼音串会同时破坏组合串并向后端发一次删除。
  const connection = { backspaceSends: BACKSPACE_SENDS.BS };
  assert.equal(
    shouldSendBackspaceAsBs(
      { type: "keydown", key: "Backspace", isComposing: true },
      connection,
      "session-1",
    ),
    false,
  );
  assert.equal(
    shouldSendBackspaceAsBs(
      { type: "keydown", key: "Backspace", keyCode: 229 },
      connection,
      "session-1",
    ),
    false,
  );
});
