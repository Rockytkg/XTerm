import assert from "node:assert/strict";
import test from "node:test";
import { pickUpdateAssetUrl } from "../src/utils/updateAssets.js";

const RELEASE_URL = "https://github.com/Rockytkg/xterm/releases/tag/v1.2.3";

function asset(name) {
  return { name, downloadUrl: `https://example.com/dl/${name}` };
}

test("windows x64 prefers the nsis installer", () => {
  const url = pickUpdateAssetUrl({
    platform: "windows",
    arch: "x86_64",
    releaseUrl: RELEASE_URL,
    assets: [
      asset("XTerm_1.2.3_x64_en-US.msi"),
      asset("XTerm_1.2.3_x64-setup.exe"),
      asset("XTerm_1.2.3_aarch64-setup.exe"),
    ],
  });

  assert.equal(url, "https://example.com/dl/XTerm_1.2.3_x64-setup.exe");
});

test("windows aarch64 matches arm64 assets only", () => {
  const url = pickUpdateAssetUrl({
    platform: "windows",
    arch: "aarch64",
    releaseUrl: RELEASE_URL,
    assets: [asset("XTerm_1.2.3_x64-setup.exe"), asset("XTerm_1.2.3_arm64-setup.exe")],
  });

  assert.equal(url, "https://example.com/dl/XTerm_1.2.3_arm64-setup.exe");
});

test("macos aarch64 matches dmg before zip", () => {
  const url = pickUpdateAssetUrl({
    platform: "macos",
    arch: "aarch64",
    releaseUrl: RELEASE_URL,
    assets: [asset("XTerm-1.2.3-darwin-arm64.zip"), asset("XTerm_1.2.3_aarch64.dmg")],
  });

  assert.equal(url, "https://example.com/dl/XTerm_1.2.3_aarch64.dmg");
});

test("linux x86_64 prefers appimage", () => {
  const url = pickUpdateAssetUrl({
    platform: "linux",
    arch: "x86_64",
    releaseUrl: RELEASE_URL,
    assets: [
      asset("xterm_1.2.3_amd64.deb"),
      asset("xterm-1.2.3-1.x86_64.rpm"),
      asset("xterm_1.2.3_amd64.AppImage"),
    ],
  });

  assert.equal(url, "https://example.com/dl/xterm_1.2.3_amd64.AppImage");
});

test("falls back to release page when no asset matches", () => {
  const url = pickUpdateAssetUrl({
    platform: "windows",
    arch: "x86_64",
    releaseUrl: RELEASE_URL,
    assets: [asset("checksums.txt")],
  });

  assert.equal(url, RELEASE_URL);
});

test("falls back to release page when assets are missing", () => {
  assert.equal(
    pickUpdateAssetUrl({ platform: "linux", arch: "x86_64", releaseUrl: RELEASE_URL }),
    RELEASE_URL,
  );
  assert.equal(pickUpdateAssetUrl(null), "");
});
