import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";
import {
  createSshCredentialPromptController,
  isCredentialPromptError,
} from "../src/stores/sshCredentialPromptController.js";

register("./helpers/mockServiceRunnerLoader.mjs", import.meta.url);

const { resetServiceRunnerState, resolveNext, serviceRunnerState } =
  await import("./helpers/mockServiceRunner.mjs");
const { connectionPatchForCredential, createSshConnectionFlow } =
  await import("../src/stores/sshConnectionFlow.js");

function createCredentialFlow(connection) {
  const controller = createSshCredentialPromptController({
    getConnection: (id) => (id === connection.id ? connection : null),
  });
  const reconnects = [];
  const refreshes = [];
  const flow = createSshConnectionFlow({
    connectionRuntime: { isCurrent: () => true },
    dispatchConnectionEvent: () => {},
    getConnection: (id) => (id === connection.id ? connection : null),
    getSessionInstance: (sessionId) =>
      sessionId === `${connection.protocol}-session` ? { connectionId: connection.id } : null,
    hostKeyPromptController: { cancelPromptForConnection: () => {} },
    refreshConnectionList: async () => refreshes.push(true),
    reconnectSession: (...args) => reconnects.push(args),
    sshCredentialPromptController: controller,
  });
  return { controller, flow, reconnects, refreshes };
}

function passwordInput(username = "") {
  return {
    authMethod: "password",
    username,
    password: "new-secret",
    privateKey: "",
    passphrase: "",
    credentialPersistence: "temporary",
  };
}

async function resolveService(command, value) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (serviceRunnerState.pending.some((entry) => entry.command === command)) {
      resolveNext(command, value);
      return;
    }
    await new Promise((resolve) => setImmediate(resolve));
  }
  throw new Error(`service ${command} was not requested`);
}

test.beforeEach(() => {
  resetServiceRunnerState();
});

test("credential errors are recognized only for their own protocol", () => {
  assert.equal(isCredentialPromptError({ code: "vnc_auth_failed" }, "vnc"), true);
  assert.equal(isCredentialPromptError({ code: "vnc_auth_failed" }, "rdp"), false);
  assert.equal(isCredentialPromptError({ code: "rdp_username_required" }, "rdp"), true);
  assert.equal(isCredentialPromptError({ code: "rdp_connect_failed" }, "rdp"), false);
});

test("VNC and RDP prompts retain protocol-specific credential requirements", () => {
  const connections = {
    "vnc-connection": {
      id: "vnc-connection",
      protocol: "vnc",
      host: "vnc.local",
      port: "5901",
      savedCredentialId: "credential-1",
      source: "profile",
    },
    "rdp-connection": {
      id: "rdp-connection",
      protocol: "rdp",
      host: "rdp.local",
      savedCredentialId: "credential-2",
      source: "profile",
    },
  };
  const controller = createSshCredentialPromptController({
    getConnection: (id) => connections[id],
  });

  assert.equal(controller.setPromptForError("vnc-connection", { code: "vnc_auth_required" }), true);
  assert.equal(controller.sshCredentialPrompt.value.reason, "missing");
  assert.equal(controller.sshCredentialPrompt.value.usernameRequired, false);
  assert.equal(controller.sshCredentialPrompt.value.canUpdateCredential, false);
  assert.equal(controller.sshCredentialPrompt.value.connection.protocol, "vnc");

  assert.equal(controller.setPromptForError("vnc-connection", { code: "vnc_auth_failed" }), true);
  assert.equal(controller.sshCredentialPrompt.value.canUpdateCredential, true);

  assert.equal(
    controller.setPromptForError("rdp-connection", { code: "rdp_username_required" }),
    true,
  );
  assert.equal(controller.sshCredentialPrompt.value.usernameRequired, true);
  assert.equal(controller.sshCredentialPrompt.value.connection.port, "3389");
});

for (const protocol of ["vnc", "rdp"]) {
  test(`${protocol} temporary credential answers reconnect with the protocol password option`, async () => {
    const connection = {
      id: `${protocol}-connection`,
      protocol,
      host: `${protocol}.local`,
      source: "profile",
    };
    const { controller, flow, reconnects } = createCredentialFlow(connection);
    const sessionId = `${protocol}-session`;

    assert.equal(
      flow.handleOpenError(
        connection.id,
        { code: `${protocol}_auth_failed`, detail: "rejected" },
        { attemptToken: 7, sessionId },
      ),
      true,
    );
    assert.equal(controller.sshCredentialPrompt.value.attemptToken, 7);

    assert.equal(await flow.answerCredentialPrompt(passwordInput("administrator")), true);
    assert.deepEqual(reconnects, [[sessionId, { [`${protocol}Password`]: "new-secret" }]]);
    assert.equal(controller.sshCredentialPrompt.value, null);
  });
}

test("passthrough VNC failures return the answer to the desktop instead of double reconnecting", async () => {
  const connection = {
    id: "vnc-connection",
    protocol: "vnc",
    host: "vnc.local",
    source: "profile",
  };
  const { controller, flow, reconnects } = createCredentialFlow(connection);
  const answers = [];
  const onAnswer = (input) => answers.push(input);

  assert.equal(
    flow.requestCredentialPrompt(
      "vnc-session",
      { code: "vnc_auth_failed", detail: "rejected" },
      { onAnswer, usernameRequired: true },
    ),
    true,
  );
  assert.equal(controller.sshCredentialPrompt.value.onAnswer, onAnswer);
  assert.equal(controller.sshCredentialPrompt.value.usernameRequired, true);

  assert.equal(await flow.answerCredentialPrompt(passwordInput("operator")), true);
  assert.deepEqual(answers, [passwordInput("operator")]);
  assert.deepEqual(reconnects, []);
});

function fullVncProfile(connection) {
  return {
    id: connection.id,
    protocol: "vnc",
    name: "desktop",
    host: "vnc.local",
    port: "5900",
    user: "",
    options: { runtimeMetrics: false },
    details: {
      protocol: "vnc",
      authMethod: "password",
      savedCredentialId: connection.savedCredentialId || undefined,
      viewOnly: true,
      shared: false,
      quality: 3,
      compression: 8,
      scaleMode: "clip",
      clipboardSync: false,
      resizeSession: true,
    },
  };
}

test("create-new persistence saves the credential and reconnects through the shared flow", async () => {
  const connection = {
    id: "vnc-connection",
    protocol: "vnc",
    host: "vnc.local",
    name: "desktop",
    user: "",
    authMethod: "password",
    savedCredentialId: "",
    source: "profile",
  };
  const { controller, flow, reconnects, refreshes } = createCredentialFlow(connection);
  assert.equal(
    flow.handleOpenError(
      connection.id,
      { code: "vnc_auth_required" },
      { sessionId: "vnc-session" },
    ),
    true,
  );

  const answering = flow.answerCredentialPrompt({
    ...passwordInput(),
    credentialPersistence: "createNew",
  });
  await resolveService("credentials_create", { id: "new-credential" });
  await resolveService("connection_get", fullVncProfile(connection));
  await resolveService("connection_update", null);

  assert.equal(await answering, true);
  assert.deepEqual(reconnects, [["vnc-session"]]);
  assert.deepEqual(refreshes, [true]);
  assert.equal(controller.sshCredentialPrompt.value, null);
  assert.deepEqual(serviceRunnerState.pending, []);
});

test("update-existing persistence keeps the credential link and reconnects once", async () => {
  const connection = {
    id: "vnc-connection",
    protocol: "vnc",
    host: "vnc.local",
    name: "desktop",
    user: "",
    authMethod: "password",
    savedCredentialId: "credential-1",
    source: "profile",
  };
  const { flow, reconnects, refreshes } = createCredentialFlow(connection);
  assert.equal(
    flow.handleOpenError(connection.id, { code: "vnc_auth_failed" }, { sessionId: "vnc-session" }),
    true,
  );

  const answering = flow.answerCredentialPrompt({
    ...passwordInput(),
    credentialPersistence: "updateExisting",
  });
  const credential = { id: "credential-1", credType: "password", name: "desktop password" };
  await resolveService("credentials_list", [credential]);
  await resolveService("credentials_usages", []);
  await resolveService("workspace_bootstrap", { connections: [connection] });
  await resolveService("credentials_list", [credential]);
  await resolveService("credentials_update", { id: "credential-1" });
  await resolveService("connection_get", fullVncProfile(connection));

  assert.equal(await answering, true);
  assert.deepEqual(reconnects, [["vnc-session"]]);
  assert.deepEqual(refreshes, [true]);
  assert.equal(serviceRunnerState.calls.includes("connection_update"), false);
  assert.deepEqual(serviceRunnerState.pending, []);
});

test("credential profile patches preserve VNC display and terminal options", () => {
  const profile = {
    id: "vnc-connection",
    protocol: "vnc",
    name: "desktop",
    host: "vnc.local",
    port: "5900",
    user: "",
    options: { runtimeMetrics: false, terminalType: "xterm-256color" },
    details: {
      protocol: "vnc",
      authMethod: "password",
      savedCredentialId: "old-credential",
      viewOnly: true,
      shared: false,
      quality: 3,
      compression: 8,
      scaleMode: "clip",
      clipboardSync: false,
      resizeSession: true,
    },
  };

  const patched = connectionPatchForCredential(profile, passwordInput(), "new-credential");
  assert.deepEqual(patched.options, profile.options);
  assert.deepEqual(patched.details, {
    ...profile.details,
    savedCredentialId: "new-credential",
  });
  assert.equal(patched.user, "");
});
