import assert from "node:assert/strict";
import test from "node:test";
import {
  TERMINAL_THEME_NAMES,
  getTerminalStatusPalette,
  getTerminalTheme,
} from "../src/utils/terminalColors.js";

const COLOR_PATTERN = /^(#[0-9a-f]{6}|rgba?\(\d+, \d+, \d+(, 0?\.\d+)?\))$/;

test("every built-in theme ships a complete, well-formed palette", () => {
  assert.equal(TERMINAL_THEME_NAMES.length, 14);
  for (const name of TERMINAL_THEME_NAMES) {
    const theme = getTerminalTheme(name);
    for (const key of ["background", "foreground", "cursor", "cursorAccent"]) {
      assert.match(theme[key], /^#[0-9a-f]{6}$/, `${name}.${key}`);
    }
    for (const key of ["selectionBackground", "selectionInactiveBackground"]) {
      assert.match(theme[key], COLOR_PATTERN, `${name}.${key}`);
    }
    for (const ansiKey of [
      "black",
      "red",
      "green",
      "yellow",
      "blue",
      "magenta",
      "cyan",
      "white",
      "brightBlack",
      "brightRed",
      "brightGreen",
      "brightYellow",
      "brightBlue",
      "brightMagenta",
      "brightCyan",
      "brightWhite",
    ]) {
      assert.match(theme[ansiKey], /^#[0-9a-f]{6}$/, `${name}.${ansiKey}`);
    }
    assert.equal(theme.extendedAnsi.length, 240, `${name}.extendedAnsi`);
  }
});

test("unknown theme names fall back to the default theme", () => {
  assert.equal(getTerminalTheme("does-not-exist"), getTerminalTheme("default"));
  assert.equal(getTerminalTheme(undefined), getTerminalTheme("default"));
});

test("status palette hint never collides with the terminal background", () => {
  for (const name of TERMINAL_THEME_NAMES) {
    const palette = getTerminalStatusPalette(name);
    const theme = getTerminalTheme(name);
    assert.notEqual(
      palette.hint.toLowerCase(),
      theme.background.toLowerCase(),
      `${name} hint would be invisible`,
    );
    for (const key of ["boot", "hint", "success", "error", "info"]) {
      assert.match(palette[key], /^#[0-9a-f]{6}$/, `${name}.${key}`);
    }
  }
});
