import assert from "node:assert/strict";
import test from "node:test";
import {
  base64ToBytes,
  classifySftpPreview,
  isEditableTextFile,
  PREVIEW_BINARY_MAX_BYTES,
  PREVIEW_TEXT_MAX_BYTES,
  PREVIEW_VIDEO_EXTENSIONS,
  resolveSniffedPreview,
} from "../src/utils/filePreview.js";

test("isEditableTextFile detects text via language-data and fallback extensions", () => {
  assert.equal(isEditableTextFile("main.py"), true);
  assert.equal(isEditableTextFile("app.js"), true);
  assert.equal(isEditableTextFile("style.vue"), true);
  assert.equal(isEditableTextFile("server.log"), true);
  assert.equal(isEditableTextFile("nginx.conf"), true);
  assert.equal(isEditableTextFile("notes.txt"), true);
});

test("isEditableTextFile treats .ts as TypeScript text, not MPEG-TS video", () => {
  assert.equal(isEditableTextFile("index.ts"), true);
});

test("isEditableTextFile rejects binary and empty names", () => {
  assert.equal(isEditableTextFile("photo.png"), false);
  assert.equal(isEditableTextFile("archive.zip"), false);
  assert.equal(isEditableTextFile("setup.exe"), false);
  assert.equal(isEditableTextFile(""), false);
  assert.equal(isEditableTextFile(), false);
});

test("classifySftpPreview resolves mime by extension and leaves unknown empty", () => {
  assert.deepEqual(classifySftpPreview({ name: "photo.png" }), {
    editable: false,
    mime: "image/png",
    tooLarge: false,
  });
  assert.equal(classifySftpPreview({ name: "cover.JPG" }).mime, "image/jpeg");
  assert.equal(classifySftpPreview({ name: "song.mp3" }).mime, "audio/mpeg");
  assert.equal(classifySftpPreview({ name: "movie.mp4" }).mime, "video/mp4");
  assert.equal(classifySftpPreview({ name: "doc.pdf" }).mime, "application/pdf");
  assert.deepEqual(classifySftpPreview({ name: "archive.zip" }), {
    editable: false,
    mime: "",
    tooLarge: false,
  });
});

test("classifySftpPreview marks text files editable", () => {
  assert.equal(classifySftpPreview({ name: "notes.txt" }).editable, true);
  assert.equal(classifySftpPreview({ name: "main.py" }).editable, true);
  assert.equal(classifySftpPreview({ name: "index.ts" }).editable, true);
});

test("classifySftpPreview flags files over the preview limit as tooLarge", () => {
  const overLimit = classifySftpPreview({ name: "big.png", size: PREVIEW_BINARY_MAX_BYTES + 1 });
  assert.equal(overLimit.tooLarge, true);
  assert.equal(overLimit.mime, "image/png");

  const atLimit = classifySftpPreview({ name: "ok.png", size: PREVIEW_BINARY_MAX_BYTES });
  assert.equal(atLimit.tooLarge, false);

  const bigText = classifySftpPreview({ name: "big.txt", size: PREVIEW_BINARY_MAX_BYTES + 1 });
  assert.equal(bigText.tooLarge, true);
  assert.equal(bigText.editable, false);

  const unknownSize = classifySftpPreview({ name: "photo.png", size: undefined });
  assert.equal(unknownSize.tooLarge, false);
});

test("classifySftpPreview disables edit for text over the editor limit", () => {
  const overEditLimit = classifySftpPreview({ name: "big.txt", size: PREVIEW_TEXT_MAX_BYTES + 1 });
  assert.equal(overEditLimit.editable, false);
  assert.equal(overEditLimit.tooLarge, false);

  const atEditLimit = classifySftpPreview({ name: "ok.txt", size: PREVIEW_TEXT_MAX_BYTES });
  assert.equal(atEditLimit.editable, true);
});

// 回归：.ts 是 TypeScript 源码而非 MPEG-TS 视频——该 mime 会成为预览 Blob 的 type，
// 预览库优先用 blob.type 匹配插件，video/mp2t 会把源码文件送进视频插件（无法正常预览）。
test("classifySftpPreview treats .ts as TypeScript source, not MPEG-TS video", () => {
  const result = classifySftpPreview({ name: "index.ts", size: 100 });
  assert.equal(result.mime, "");
  assert.equal(result.editable, true);
  assert.equal(PREVIEW_VIDEO_EXTENSIONS.has("ts"), false);
  assert.equal(classifySftpPreview({ name: "movie.m2ts" }).mime, "video/mp2t");
});

test("resolveSniffedPreview adopts sniffed text mime for extensionless files", () => {
  assert.deepEqual(resolveSniffedPreview("nginx", 1024, "text/plain"), {
    mime: "text/plain",
    editable: true,
  });
  assert.deepEqual(resolveSniffedPreview(".bashrc", 512, "text/plain"), {
    mime: "text/plain",
    editable: true,
  });
});

test("resolveSniffedPreview keeps sniffed binary mime but not editable", () => {
  assert.deepEqual(resolveSniffedPreview("logo", 2048, "image/png"), {
    mime: "image/png",
    editable: false,
  });
});

test("resolveSniffedPreview handles missing sniff result", () => {
  assert.deepEqual(resolveSniffedPreview("data", 100, null), { mime: null, editable: false });
  assert.deepEqual(resolveSniffedPreview("data", 100, undefined), { mime: null, editable: false });
});

test("resolveSniffedPreview disables edit for sniffed text over the editor limit", () => {
  const overLimit = resolveSniffedPreview("huge", PREVIEW_TEXT_MAX_BYTES + 1, "text/plain");
  assert.deepEqual(overLimit, { mime: "text/plain", editable: false });

  const atLimit = resolveSniffedPreview("huge", PREVIEW_TEXT_MAX_BYTES, "text/plain");
  assert.equal(atLimit.editable, true);
});

test("resolveSniffedPreview ignores sniffed mime when the name has an extension", () => {
  assert.deepEqual(resolveSniffedPreview("notes.md", 100, "text/plain"), {
    mime: null,
    editable: true,
  });
  assert.deepEqual(resolveSniffedPreview("photo.png", 100, "image/png"), {
    mime: null,
    editable: false,
  });
});

test("base64ToBytes round-trips arbitrary bytes", () => {
  const original = Uint8Array.from({ length: 256 }, (_, index) => index);
  let binary = "";
  for (const byte of original) binary += String.fromCharCode(byte);
  const base64 = Buffer.from(binary, "binary").toString("base64");

  const decoded = base64ToBytes(base64);
  assert.ok(decoded instanceof Uint8Array);
  assert.deepEqual([...decoded], [...original]);
});

test("base64ToBytes decodes empty input to empty bytes", () => {
  assert.equal(base64ToBytes("").length, 0);
});
