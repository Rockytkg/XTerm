import assert from "node:assert/strict";
import test from "node:test";
import { createHostKeyPromptController } from "../src/stores/hostKeyPromptController.js";

test("host-key prompts require the open request session identity", () => {
  const controller = createHostKeyPromptController();

  assert.equal(controller.setPrompt({ connectionId: "connection-1" }), false);
  assert.equal(controller.hostKeyPrompt.value, null);

  assert.equal(
    controller.setPrompt({
      connectionId: "connection-1",
      sessionId: "pending-session-1",
      fingerprint: "SHA256:test",
    }),
    true,
  );
  assert.equal(controller.hostKeyPrompt.value.sessionId, "pending-session-1");
});
