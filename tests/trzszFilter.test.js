import assert from "node:assert/strict";
import test from "node:test";

import { LocalTrzszFilter } from "../src/utils/terminal/addons/trzsz/filter.js";

test("drag upload completion waits for cleanup and transfer mode restoration", async () => {
  const events = [];
  const filter = new LocalTrzszFilter({
    sendToServer: () => {},
    writeToTerminal: () => {},
    onTransferModeChange: async (enabled) => {
      events.push(`mode:${enabled}`);
    },
  });
  filter._upload = async () => {
    filter._transfer.cleanup = async () => {
      await Promise.resolve();
      events.push("cleanup");
    };
  };
  filter._uploadResolve = () => events.push("resolved");

  await filter._detectAndHandle("::TRZSZ:TRANSFER:R:1.2.3");

  assert.deepEqual(events, ["mode:true", "cleanup", "mode:false", "resolved"]);
});
