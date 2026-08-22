import assert from "node:assert/strict";
import test from "node:test";
import {
  formatConnectionError,
  isConnectionNotActiveError,
  parseIpcError,
} from "../src/services/ipc/errors.js";

test("parseIpcError reads the JSON-embedded error contract", () => {
  const parsed = parseIpcError(
    '{"code":"connection_not_active","detail":"connection is not active","retryable":false}',
  );

  assert.equal(parsed.code, "connection_not_active");
  assert.equal(parsed.detail, "connection is not active");
  assert.equal(parsed.retryable, false);
});

test("parseIpcError falls back to a bare string detail", () => {
  const parsed = parseIpcError("connection refused");

  assert.equal(parsed.code, "unknown");
  assert.equal(parsed.detail, "connection refused");
  assert.equal(parsed.retryable, false);
});

test("parseIpcError maps recoverable to retryable", () => {
  const parsed = parseIpcError({ code: "io_timeout", detail: "timed out", recoverable: true });

  assert.equal(parsed.code, "io_timeout");
  assert.equal(parsed.retryable, true);
});

test("parseIpcError preserves structured args", () => {
  const parsed = parseIpcError(
    '{"code":"serial_port_not_found","detail":"port=COM1; missing","retryable":true,"args":{"portName":"COM1","detail":"missing"}}',
  );

  assert.equal(parsed.code, "serial_port_not_found");
  assert.deepEqual(parsed.args, { portName: "COM1", detail: "missing" });
});

test("parseIpcError leaves args undefined when absent", () => {
  const parsed = parseIpcError(
    '{"code":"connection_not_active","detail":"gone","retryable":false}',
  );

  assert.equal(parsed.args, undefined);
});

test("isConnectionNotActiveError accepts the new error code", () => {
  assert.equal(
    isConnectionNotActiveError('{"code":"connection_not_active","detail":"gone"}'),
    true,
  );
});

test("formatConnectionError stays export-compatible for store modules", () => {
  const formatted = formatConnectionError('{"code":"auth_failed","message":"nope"}');

  assert.equal(formatted.code, "auth_failed");
  assert.equal(formatted.message, "nope");
});
