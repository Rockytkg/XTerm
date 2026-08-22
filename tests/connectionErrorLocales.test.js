import assert from "node:assert/strict";
import test from "node:test";
import enUS from "../src/i18n/locales/en-US.js";
import zhCN from "../src/i18n/locales/zh-CN.js";

test("connection error summaries do not embed the raw diagnostic detail", () => {
  for (const locale of [zhCN, enUS]) {
    for (const summary of Object.values(locale.connectionErrors)) {
      assert.equal(summary.includes("{detail}"), false);
    }
  }
});

test("serial port not found keeps the localized summary separate from its diagnostic", () => {
  assert.equal(zhCN.connectionErrors.serial_port_not_found, "串口 {portName} 未找到");
  assert.equal(enUS.connectionErrors.serial_port_not_found, "Serial port {portName} not found");
});
