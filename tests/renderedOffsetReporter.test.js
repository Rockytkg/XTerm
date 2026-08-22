import assert from "node:assert/strict";
import test from "node:test";
import { createRenderedOffsetReporter } from "../src/utils/terminal/renderedOffsetReporter.js";

function createHarness(options = {}) {
  const sent = [];
  let currentTime = options.startTime ?? 1000;
  const reporter = createRenderedOffsetReporter({
    send: (offset) => sent.push(offset),
    now: () => currentTime,
    minIntervalMs: options.minIntervalMs ?? 500,
    minBytesBetweenReports: options.minBytesBetweenReports ?? 4 * 1024 * 1024,
  });
  return {
    reporter,
    sent,
    advance(ms) {
      currentTime += ms;
    },
  };
}

test("does not report before any data was consumed", () => {
  const { reporter, sent, advance } = createHarness();

  reporter.onBackpressureResume();
  advance(1000);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, []);
});

test("first resume after consumption reports the latest offset", () => {
  const { reporter, sent } = createHarness();

  reporter.noteConsumed(100, 4096);
  reporter.noteConsumed(50, 8192);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, [8192]);
});

test("throttles reports inside the interval window", () => {
  const { reporter, sent, advance } = createHarness();

  reporter.noteConsumed(10, 100);
  reporter.onBackpressureResume();
  reporter.noteConsumed(10, 200);
  reporter.onBackpressureResume();
  reporter.noteConsumed(10, 300);
  advance(499);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, [100]);

  advance(1);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, [100, 300]);
});

test("byte threshold forces a report even inside the interval window", () => {
  const { reporter, sent, advance } = createHarness({
    minBytesBetweenReports: 1024,
  });

  reporter.noteConsumed(10, 100);
  reporter.onBackpressureResume();
  advance(10);
  reporter.noteConsumed(2048, 5000);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, [100, 5000]);
});

test("reset stops further reports until new data arrives", () => {
  const { reporter, sent, advance } = createHarness();

  reporter.noteConsumed(10, 100);
  reporter.onBackpressureResume();
  reporter.reset();
  advance(1000);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, [100]);

  reporter.noteConsumed(10, 200);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, [100, 200]);
});

test("ignores invalid offsets", () => {
  const { reporter, sent } = createHarness();

  reporter.noteConsumed(10, Number.NaN);
  reporter.noteConsumed(10, null);
  reporter.onBackpressureResume();

  assert.deepEqual(sent, []);
});
