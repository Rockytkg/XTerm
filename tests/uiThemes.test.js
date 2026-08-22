import assert from "node:assert/strict";
import test from "node:test";
import { normalizeUiThemePreset, resolveUiThemeAttribute } from "../src/utils/uiThemes.js";

test("normalizeUiThemePreset keeps known presets and falls back to default", () => {
  assert.equal(normalizeUiThemePreset("solarized"), "solarized");
  assert.equal(normalizeUiThemePreset("github"), "github");
  assert.equal(normalizeUiThemePreset("default"), "default");
  assert.equal(normalizeUiThemePreset("unknown"), "default");
  assert.equal(normalizeUiThemePreset(""), "default");
  assert.equal(normalizeUiThemePreset(undefined), "default");
});

test("resolveUiThemeAttribute maps preset and effective mode to the CSS attribute", () => {
  assert.equal(resolveUiThemeAttribute("default", "light"), "");
  assert.equal(resolveUiThemeAttribute("default", "dark"), "");
  assert.equal(resolveUiThemeAttribute("solarized", "light"), "solarized-light");
  assert.equal(resolveUiThemeAttribute("solarized", "dark"), "solarized-dark");
  assert.equal(resolveUiThemeAttribute("github", "light"), "github-light");
  assert.equal(resolveUiThemeAttribute("github", "dark"), "github-dark");
  assert.equal(resolveUiThemeAttribute("bogus", "dark"), "");
});
