import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

register("./helpers/mockSftpServiceLoader.mjs", import.meta.url);

const { createSftpSessionScope } = await import("../src/composables/sftpSessionScope.js");

function createProps() {
  return {
    connection: { id: "c1", capabilities: { sftp: true } },
    sessionId: "s1",
  };
}

test("会话快照校验连接、会话、能力与销毁状态", () => {
  const props = createProps();
  const scope = createSftpSessionScope(props);
  const session = scope.currentSession("/home");

  assert.deepEqual(session, { connectionId: "c1", sessionId: "s1", path: "/home" });
  assert.equal(scope.isStaleSession(session), false);

  props.connection = { ...props.connection, capabilities: { sftp: false } };
  assert.equal(scope.currentSession(), null);
  assert.equal(scope.isStaleSession(session), true);

  props.connection = { ...props.connection, capabilities: { sftp: true } };
  assert.equal(scope.isStaleSession(session), false);
  props.sessionId = "s2";
  assert.equal(scope.isStaleSession(session), true);

  props.sessionId = "s1";
  scope.setDisposed(true);
  assert.equal(scope.isDisposed(), true);
  assert.equal(scope.isStaleSession(session), true);
});

test("空会话与缺失能力不会生成可用快照", () => {
  const props = createProps();
  props.sessionId = "";
  const scope = createSftpSessionScope(props);

  assert.equal(scope.currentSession(), null);
  assert.equal(scope.isStaleSession(null), true);
});
