import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";

// 与后端读取上限保持一致：编辑走 sftp_read_file（5 MiB），预览走 sftp_read_file_bytes（20 MiB）
export const PREVIEW_TEXT_MAX_BYTES = 5 * 1024 * 1024;
export const PREVIEW_BINARY_MAX_BYTES = 20 * 1024 * 1024;

const PREVIEW_IMAGE_MIME_BY_EXTENSION = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  svg: "image/svg+xml",
  ico: "image/x-icon",
  tif: "image/tiff",
  tiff: "image/tiff",
  avif: "image/avif",
};
const PREVIEW_AUDIO_MIME_BY_EXTENSION = {
  mp3: "audio/mpeg",
  wav: "audio/wav",
  ogg: "audio/ogg",
  flac: "audio/flac",
  aac: "audio/aac",
  m4a: "audio/mp4",
  opus: "audio/ogg",
};
const PREVIEW_VIDEO_MIME_BY_EXTENSION = {
  mp4: "video/mp4",
  mkv: "video/x-matroska",
  avi: "video/x-msvideo",
  mov: "video/quicktime",
  wmv: "video/x-ms-wmv",
  webm: "video/webm",
  m4v: "video/mp4",
  // .ts 按 TypeScript 源码处理（与 isEditableTextFile 一致），不作为 MPEG-TS 视频：
  // 该 mime 会成为预览 Blob 的 type，而预览库优先采用 blob.type 匹配插件，会把源码送进视频插件
  m2ts: "video/mp2t",
};

// 预览 Blob 的 mime 提示：基础识别靠文件名扩展名，未知类型留空由预览库自行探测；
// 后端魔数嗅探命中具体类型时覆盖此映射（见 resolveSniffedPreview）
const PREVIEW_MIME_BY_EXTENSION = {
  ...PREVIEW_IMAGE_MIME_BY_EXTENSION,
  ...PREVIEW_AUDIO_MIME_BY_EXTENSION,
  ...PREVIEW_VIDEO_MIME_BY_EXTENSION,
  pdf: "application/pdf",
};

export const PREVIEW_IMAGE_EXTENSIONS = new Set(Object.keys(PREVIEW_IMAGE_MIME_BY_EXTENSION));
export const PREVIEW_AUDIO_EXTENSIONS = new Set(Object.keys(PREVIEW_AUDIO_MIME_BY_EXTENSION));
export const PREVIEW_VIDEO_EXTENSIONS = new Set(Object.keys(PREVIEW_VIDEO_MIME_BY_EXTENSION));

// language-data 未覆盖的常见纯文本扩展名兜底
const EDITABLE_TEXT_FALLBACK_EXTENSIONS = new Set([
  "txt",
  "log",
  "md",
  "markdown",
  "rst",
  "json",
  "jsonc",
  "json5",
  "yaml",
  "yml",
  "xml",
  "csv",
  "tsv",
  "ini",
  "cfg",
  "conf",
  "env",
  "toml",
  "properties",
  "lock",
]);

export function extensionOfFileName(name) {
  const value = String(name || "");
  const dot = value.lastIndexOf(".");
  if (dot <= 0 || dot === value.length - 1) return "";
  return value.slice(dot + 1).toLowerCase();
}

// 是否可转入文本编辑：语言匹配优先于视频扩展名（.ts 是 TypeScript 而非 MPEG-TS）
export function isEditableTextFile(name) {
  const fileName = String(name || "");
  if (!fileName) return false;
  if (LanguageDescription.matchFilename(languages, fileName)) return true;
  return EDITABLE_TEXT_FALLBACK_EXTENSIONS.has(extensionOfFileName(fileName));
}

// 预览标签分类：editable 供"转编辑"入口与图标，tooLarge 为预览读取上限预判
export function classifySftpPreview({ name, size } = {}) {
  const bytes = Number(size);
  const tooLarge = Number.isFinite(bytes) && bytes > PREVIEW_BINARY_MAX_BYTES;
  const editable =
    isEditableTextFile(name) && !(Number.isFinite(bytes) && bytes > PREVIEW_TEXT_MAX_BYTES);
  return {
    editable,
    mime: PREVIEW_MIME_BY_EXTENSION[extensionOfFileName(name)] || "",
    tooLarge,
  };
}

// 后端内容嗅探结果的应用规则：
// - 魔数（infer）命中的具体类型优先于扩展名映射——扩展名可篡改，内容才是真实类型
//   （如压缩包改名 .hcl 仍应按压缩包预览，且不得因文本扩展名出现"转编辑"入口）；
// - text/plain 只是 content_inspector "是文本"的弱信号，不覆盖扩展名
//   （避免 .md 被嗅探成 text/plain 后失去按扩展名的渲染）；
// - 无扩展名文件采用嗅探 mime，文本可转编辑。
export function resolveSniffedPreview(name, size, sniffedMime) {
  const bytes = Number(size);
  const withinEditLimit = !(Number.isFinite(bytes) && bytes > PREVIEW_TEXT_MAX_BYTES);
  const mime = typeof sniffedMime === "string" && sniffedMime ? sniffedMime : null;
  if (mime && mime !== "text/plain") {
    return { mime, editable: false };
  }
  if (extensionOfFileName(name)) {
    return { mime: null, editable: isEditableTextFile(name) && withinEditLimit };
  }
  return { mime, editable: !!mime && mime.startsWith("text/") && withinEditLimit };
}

export function base64ToBytes(base64) {
  const binary = atob(String(base64 || ""));
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

// 分块转换避免 String.fromCharCode 展开大数组时超出参数数量上限
export function bytesToBase64(bytes) {
  let binary = "";
  const chunkSize = 0x8000;
  for (let index = 0; index < bytes.length; index += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize));
  }
  return btoa(binary);
}

// 归一化 IPC 原始字节响应：自定义协议通道恒为 ArrayBuffer；
// JSON 通道（macOS/iOS 的 WKWebView 或自定义协议失效回退）下退化为数字数组
export function bytesFromIpcResult(result) {
  if (result instanceof ArrayBuffer) return new Uint8Array(result);
  if (ArrayBuffer.isView(result)) {
    return new Uint8Array(result.buffer, result.byteOffset, result.byteLength);
  }
  if (Array.isArray(result)) return Uint8Array.from(result);
  return new Uint8Array(0);
}
