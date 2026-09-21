import assert from "node:assert/strict";
import test from "node:test";
import {
  CONNECTION_PROTOCOLS,
  isVncProtocol,
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

test("vnc is a first-class password-only protocol", () => {
  assert.ok(CONNECTION_PROTOCOLS.includes("vnc"));
  assert.equal(isVncProtocol("vnc"), true);
  assert.equal(isVncProtocol("VNC"), true);
  assert.equal(isVncProtocol("ssh"), false);
  assert.equal(requiresPasswordCredential("vnc"), true);
  assert.equal(supportsSavedCredential("vnc"), true);
  assert.equal(requiresHostKeyVerification("vnc"), false);
  assert.deepEqual(protocolDisplayClass("vnc"), {
    "session-card-status-serial": false,
    "session-card-status-telnet": false,
    "session-card-status-ssh": false,
    "session-card-status-vnc": true,
    "session-card-status-rdp": false,
  });
});

test("vnc protocol draft carries display defaults", () => {
  const draft = createProtocolDraft("vnc");
  assert.equal(draft.port, 5900);
  assert.equal(draft.authMethod, "password");
  assert.equal(draft.viewOnly, false);
  assert.equal(draft.shared, true);
  assert.equal(draft.quality, 6);
  assert.equal(draft.compression, 2);
  assert.equal(draft.scaleMode, "fit");
  assert.equal(draft.clipboardSync, true);
  assert.equal(draft.resizeSession, false);
});

test("vnc draft restores values from an existing profile", () => {
  const profile = {
    name: "ci-desktop",
    host: "builder.local",
    port: "5901",
    details: {
      protocol: "vnc",
      authMethod: "password",
      savedCredentialId: "cred-1",
      viewOnly: true,
      shared: false,
      quality: 3,
      compression: 8,
      scaleMode: "clip",
      clipboardSync: false,
      resizeSession: true,
    },
  };
  const draft = createProtocolDraft("vnc", profile);
  assert.equal(draft.host, "builder.local");
  assert.equal(draft.port, "5901");
  assert.equal(draft.savedCredentialId, "cred-1");
  assert.equal(draft.viewOnly, true);
  assert.equal(draft.shared, false);
  assert.equal(draft.quality, 3);
  assert.equal(draft.compression, 8);
  assert.equal(draft.scaleMode, "clip");
  assert.equal(draft.clipboardSync, false);
  assert.equal(draft.resizeSession, true);
});

test("building a vnc profile writes display options into details", () => {
  const profile = buildConnectionProfile({
    id: "conn-1",
    protocol: "vnc",
    form: {
      name: "desktop",
      host: "vnc.example.com",
      port: "5900",
      user: "",
      authMethod: "password",
      viewOnly: false,
      shared: true,
      quality: 7,
      compression: 1,
      scaleMode: "none",
      clipboardSync: true,
      resizeSession: false,
    },
    savedCredentialId: "cred-9",
  });
  assert.equal(profile.protocol, "vnc");
  assert.equal(profile.host, "vnc.example.com");
  assert.equal(profile.port, "5900");
  assert.deepEqual(profile.details, {
    protocol: "vnc",
    authMethod: "password",
    savedCredentialId: "cred-9",
    viewOnly: false,
    shared: true,
    quality: 7,
    compression: 1,
    scaleMode: "none",
    clipboardSync: true,
    resizeSession: false,
  });
});

test("sidebar vnc toggles merge into profile details, not options", () => {
  const profile = {
    id: "conn-1",
    protocol: "vnc",
    options: { terminalHighlightEnabled: true },
    details: { protocol: "vnc", viewOnly: false, scaleMode: "fit", clipboardSync: true },
  };
  const merged = mergeConnectionProfileOptions(profile, {
    vncViewOnly: true,
    vncScaleMode: "clip",
    vncClipboardSync: undefined,
  });
  assert.equal(merged.details.viewOnly, true);
  assert.equal(merged.details.scaleMode, "clip");
  assert.equal("clipboardSync" in merged.details, false);
  assert.deepEqual(merged.options, { terminalHighlightEnabled: true });
  // 原 profile 不被修改（不可变更新）
  assert.equal(profile.details.viewOnly, false);
});
