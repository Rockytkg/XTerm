import assert from "node:assert/strict";
import test from "node:test";
import { computed } from "vue";
import { createWorkspaceConnectionCatalog } from "../src/stores/workspaceConnectionCatalog.js";
import {
  buildConnectionProfile,
  createProtocolDraft,
} from "../src/components/connection-dialog/connectionDialogModel.js";
import { mergeConnectionProfileOptions } from "../src/utils/connectionProfileOptions.js";

test("connection option patches preserve the full profile shape", () => {
  const profile = {
    id: "profile-1",
    protocol: "ssh",
    name: "router",
    host: "192.0.2.1",
    user: "admin",
    options: { terminalType: "xterm-256color", backspaceSends: "del" },
    details: { protocol: "ssh", authMethod: "password" },
  };

  const updated = mergeConnectionProfileOptions(profile, {
    terminalType: "vt100",
    encoding: "gbk",
  });

  assert.deepEqual(updated.options, {
    terminalType: "vt100",
    backspaceSends: "del",
    encoding: "gbk",
  });
  assert.strictEqual(updated.details, profile.details);
  assert.equal(updated.host, profile.host);
});

test("undefined option patches remove defaults before persistence", () => {
  const updated = mergeConnectionProfileOptions(
    { options: { encoding: "gbk", terminalMorePromptCleanup: true } },
    { encoding: undefined },
  );

  assert.deepEqual(updated.options, { terminalMorePromptCleanup: true });
});

test("catalog patches immediately update projected connection values", () => {
  const catalog = createWorkspaceConnectionCatalog();
  catalog.setProfiles([{ id: "profile-1", terminalType: "xterm-256color" }]);
  const terminalType = computed(() => catalog.profileConnections.value[0]?.terminalType);

  assert.equal(terminalType.value, "xterm-256color");
  assert.equal(catalog.patchRecord("profile-1", { terminalType: "vt100" }), true);
  assert.equal(terminalType.value, "vt100");
});

test("SSH legacy algorithms are opt-in and round-trip through the connection dialog", () => {
  const base = {
    id: "profile-ssh",
    protocol: "ssh",
    savedCredentialId: "",
    form: {
      name: "legacy router",
      host: "192.0.2.10",
      port: 22,
      user: "admin",
      terminalType: "xterm-256color",
      encoding: "auto",
      backspaceSends: "BS",
    },
  };

  assert.equal(createProtocolDraft("ssh").sshAllowLegacyAlgorithms, false);
  const defaultProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, sshAllowLegacyAlgorithms: false },
  });
  assert.equal(defaultProfile.options.sshAllowLegacyAlgorithms, undefined);

  const enabledProfile = buildConnectionProfile({
    ...base,
    form: { ...base.form, sshAllowLegacyAlgorithms: true },
  });
  assert.equal(enabledProfile.options.sshAllowLegacyAlgorithms, true);
  assert.equal(createProtocolDraft("ssh", enabledProfile).sshAllowLegacyAlgorithms, true);

  const cleared = mergeConnectionProfileOptions(enabledProfile, {
    sshAllowLegacyAlgorithms: undefined,
  });
  assert.equal("sshAllowLegacyAlgorithms" in cleared.options, false);
});
