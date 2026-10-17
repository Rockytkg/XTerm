import assert from "node:assert/strict";
import test from "node:test";
import {
  buildConnectionProfile,
  createProtocolDraft,
} from "../src/components/connection-dialog/connectionDialogModel.js";
import { shouldSendBackspaceAsBs } from "../src/utils/terminal/inputRouting.js";
import { BACKSPACE_SENDS } from "../src/utils/terminalSessionOptions.js";

test("serial and telnet default to AUTO backspace, ssh stays DEL", () => {
  assert.equal(createProtocolDraft("serial").backspaceSends, BACKSPACE_SENDS.AUTO);
  assert.equal(createProtocolDraft("telnet").backspaceSends, BACKSPACE_SENDS.AUTO);
  assert.equal(createProtocolDraft("ssh").backspaceSends, BACKSPACE_SENDS.DEL);
});

test("serial profiles omit the AUTO default and persist explicit modes", () => {
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
  const autoProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, backspaceSends: BACKSPACE_SENDS.AUTO },
  });
  assert.equal(autoProfile.options.backspaceSends, undefined);

  const bsProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, backspaceSends: BACKSPACE_SENDS.BS },
  });
  assert.equal(bsProfile.options.backspaceSends, BACKSPACE_SENDS.BS);
});

test("explicit BS intercepts the physical key; AUTO/DEL pass through to the backend", () => {
  const event = { type: "keydown", key: "Backspace" };
  assert.equal(
    shouldSendBackspaceAsBs(event, { backspaceSends: BACKSPACE_SENDS.BS }, "session-1"),
    true,
  );
  assert.equal(
    shouldSendBackspaceAsBs(event, { backspaceSends: BACKSPACE_SENDS.AUTO }, "session-1"),
    false,
  );
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
