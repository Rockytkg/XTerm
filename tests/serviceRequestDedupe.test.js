import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

register("./helpers/mockServiceRunnerLoader.mjs", import.meta.url);

const { serviceRunnerState, resetServiceRunnerState, resolveNext, rejectNext } =
  await import("./helpers/mockServiceRunner.mjs");
const { loadCredentials, loadCredentialUsages } = await import("../src/services/credentials.js");
const { loadWorkspaceBootstrap } = await import("../src/services/workspace.js");

test.beforeEach(() => {
  resetServiceRunnerState();
});

test("concurrent credential reads share one IPC request", async () => {
  const first = loadCredentials();
  const second = loadCredentials();
  assert.strictEqual(first, second);
  assert.deepEqual(serviceRunnerState.calls, ["credentials_list"]);
  resolveNext("credentials_list", [{ id: "credential-1" }]);
  assert.deepEqual(await Promise.all([first, second]), [
    [{ id: "credential-1" }],
    [{ id: "credential-1" }],
  ]);
});

test("concurrent usage and workspace reads each share their own request", async () => {
  const usages = loadCredentialUsages();
  const usagesAgain = loadCredentialUsages();
  const workspace = loadWorkspaceBootstrap();
  const workspaceAgain = loadWorkspaceBootstrap();
  assert.strictEqual(usages, usagesAgain);
  assert.strictEqual(workspace, workspaceAgain);
  assert.deepEqual(serviceRunnerState.calls, ["credentials_usages", "workspace_bootstrap"]);
  resolveNext("credentials_usages", []);
  resolveNext("workspace_bootstrap", { connections: [] });
  await Promise.all([usages, usagesAgain, workspace, workspaceAgain]);
});

test("a failed shared request is cleared so a later read can retry", async () => {
  const first = loadCredentials();
  const second = loadCredentials();
  assert.strictEqual(first, second);
  rejectNext("credentials_list", new Error("temporary failure"));
  await assert.rejects(first, /temporary failure/);

  const retry = loadCredentials();
  assert.notStrictEqual(retry, first);
  assert.deepEqual(serviceRunnerState.calls, ["credentials_list", "credentials_list"]);
  resolveNext("credentials_list", []);
  assert.deepEqual(await retry, []);
});
