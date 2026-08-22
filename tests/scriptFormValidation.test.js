import assert from "node:assert/strict";
import test from "node:test";
import { isValidHttpUrl, validateScriptFields } from "../src/services/scripting/formValidation.js";

test("isValidHttpUrl accepts http/https and rejects the rest", () => {
  assert.equal(isValidHttpUrl("https://example.com/x"), true);
  assert.equal(isValidHttpUrl("http://192.168.1.1:8080"), true);
  assert.equal(isValidHttpUrl("ftp://example.com"), false);
  assert.equal(isValidHttpUrl("not-a-url"), false);
  assert.equal(isValidHttpUrl(""), false);
});

test("validateScriptFields flags missing required values", () => {
  const errors = validateScriptFields([{ key: "hostname", required: true }, { key: "vlan" }], {
    hostname: "  ",
    vlan: "",
  });
  assert.deepEqual(errors, { hostname: "required" });
});

test("built-in rules validate url/email/phone/number", () => {
  const fields = [
    { key: "url", type: "url" },
    { key: "email", type: "email" },
    { key: "phone", type: "phone" },
    { key: "count", type: "number" },
  ];
  assert.deepEqual(
    validateScriptFields(fields, {
      url: "https://example.com",
      email: "ops@example.com",
      phone: "13800138000",
      count: "24",
    }),
    {},
  );
  assert.deepEqual(
    validateScriptFields(fields, {
      url: "example",
      email: "not-an-email",
      phone: "12345",
      count: "abc",
    }),
    {
      url: "invalidUrl",
      email: "invalidEmail",
      phone: "invalidPhone",
      count: "invalidNumber",
    },
  );
});

test("custom pattern validates with RegExp or regex source string", () => {
  const errors = validateScriptFields(
    [
      { key: "vlan", pattern: /^\d{1,4}$/ },
      { key: "code", pattern: "^[A-Z]{2}$" },
    ],
    { vlan: "10086a", code: "ab" },
  );
  assert.deepEqual(errors, { vlan: "patternMismatch", code: "patternMismatch" });
});

test("empty optional fields skip all format rules", () => {
  const errors = validateScriptFields(
    [
      { key: "email", type: "email" },
      { key: "vlan", pattern: /^\d+$/ },
    ],
    { email: "", vlan: "" },
  );
  assert.deepEqual(errors, {});
});

test("boolean fields are never validated", () => {
  const errors = validateScriptFields([{ key: "save", type: "switch", required: true }], {
    save: false,
  });
  assert.deepEqual(errors, {});
});
