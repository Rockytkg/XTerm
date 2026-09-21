import assert from "node:assert/strict";
import test from "node:test";
import {
  CONNECTION_PROTOCOLS,
  isRdpProtocol,
  protocolDisplayClass,
  requiresHostKeyVerification,
  requiresPasswordCredential,
  supportsSavedCredential,
} from "../src/utils/connectionProtocols.js";
import {
  buildConnectionProfile,
  createProtocolDraft,
} from "../src/components/connection-dialog/connectionDialogModel.js";
import { mergeConnectionProfileOptions } from "../src/utils/connectionProfileOptions.js";

test("rdp is a first-class password-only protocol", () => {
  assert.ok(CONNECTION_PROTOCOLS.includes("rdp"));
  assert.equal(isRdpProtocol("rdp"), true);
  assert.equal(isRdpProtocol("RDP"), true);
  assert.equal(isRdpProtocol("vnc"), false);
  assert.equal(requiresPasswordCredential("rdp"), true);
  assert.equal(supportsSavedCredential("rdp"), true);
  assert.equal(requiresHostKeyVerification("rdp"), false);
  assert.deepEqual(protocolDisplayClass("rdp"), {
    "session-card-status-serial": false,
    "session-card-status-telnet": false,
    "session-card-status-ssh": false,
    "session-card-status-vnc": false,
    "session-card-status-rdp": true,
  });
});

test("rdp protocol draft carries display defaults", () => {
  const draft = createProtocolDraft("rdp");
  assert.equal(draft.port, 3389);
  assert.equal(draft.authMethod, "password");
  assert.equal(draft.domain, "");
  assert.equal(draft.scaleMode, "fit");
  assert.equal(draft.clipboardSync, true);
  assert.equal(draft.resizeSession, false);
});

test("rdp draft restores values from an existing profile", () => {
  const profile = {
    name: "ci-windows",
    host: "builder.local",
    port: "3390",
    user: "admin",
    details: {
      protocol: "rdp",
      authMethod: "password",
      savedCredentialId: "cred-1",
      domain: "CORP",
      scaleMode: "clip",
      clipboardSync: false,
      resizeSession: true,
    },
  };
  const draft = createProtocolDraft("rdp", profile);
  assert.equal(draft.host, "builder.local");
  assert.equal(draft.port, "3390");
  assert.equal(draft.user, "admin");
  assert.equal(draft.savedCredentialId, "cred-1");
  assert.equal(draft.domain, "CORP");
  assert.equal(draft.scaleMode, "clip");
  assert.equal(draft.clipboardSync, false);
  assert.equal(draft.resizeSession, true);
});

test("building an rdp profile writes display options into details", () => {
  const profile = buildConnectionProfile({
    id: "conn-1",
    protocol: "rdp",
    form: {
      name: "desktop",
      host: "rdp.example.com",
      port: "3389",
      user: "admin",
      authMethod: "password",
      domain: "CORP",
      scaleMode: "none",
      clipboardSync: true,
      resizeSession: true,
    },
    savedCredentialId: "cred-9",
  });
  assert.equal(profile.protocol, "rdp");
  assert.equal(profile.host, "rdp.example.com");
  assert.equal(profile.port, "3389");
  assert.equal(profile.user, "admin");
  assert.deepEqual(profile.details, {
    protocol: "rdp",
    authMethod: "password",
    savedCredentialId: "cred-9",
    domain: "CORP",
    scaleMode: "none",
    clipboardSync: true,
    resizeSession: true,
  });
});

test("building an rdp profile drops an empty domain", () => {
  const profile = buildConnectionProfile({
    id: "conn-1",
    protocol: "rdp",
    form: {
      name: "desktop",
      host: "rdp.example.com",
      port: "3389",
      user: "admin",
      domain: "  ",
      scaleMode: "fit",
      clipboardSync: true,
      resizeSession: false,
    },
  });
  assert.equal("domain" in profile.details, false);
});

test("sidebar rdp toggles merge into profile details, not options", () => {
  const profile = {
    id: "conn-1",
    protocol: "rdp",
    options: { terminalHighlightEnabled: true },
    details: { protocol: "rdp", domain: "CORP", scaleMode: "fit", clipboardSync: true },
  };
  const merged = mergeConnectionProfileOptions(profile, {
    rdpDomain: "LAB",
    rdpScaleMode: "clip",
    rdpClipboardSync: undefined,
  });
  assert.equal(merged.details.domain, "LAB");
  assert.equal(merged.details.scaleMode, "clip");
  assert.equal("clipboardSync" in merged.details, false);
  assert.deepEqual(merged.options, { terminalHighlightEnabled: true });
  // 原 profile 不被修改（不可变更新）
  assert.equal(profile.details.domain, "CORP");
});
