import {
  Binary,
  Braces,
  Database,
  File,
  FileArchive,
  FileCode,
  FileImage,
  FileMusic,
  FileSpreadsheet,
  FileSymlink,
  FileTerminal,
  FileText,
  FileVideoCamera,
  Folder,
  FolderSymlink,
  Package,
  Presentation,
  ScrollText,
  Shield,
} from "@lucide/vue";

const TEXT_EXTENSIONS = new Set([
  "txt",
  "md",
  "markdown",
  "rst",
  "log",
  "ini",
  "cfg",
  "conf",
  "yaml",
  "yml",
  "toml",
  "properties",
]);
const CODE_EXTENSIONS = new Set([
  "js",
  "jsx",
  "ts",
  "tsx",
  "vue",
  "rs",
  "c",
  "h",
  "hpp",
  "cpp",
  "cc",
  "cs",
  "go",
  "java",
  "kt",
  "kts",
  "swift",
  "py",
  "rb",
  "php",
  "sh",
  "bash",
  "zsh",
  "fish",
  "ps1",
  "bat",
  "cmd",
  "css",
  "scss",
  "sass",
  "less",
  "html",
  "xml",
  "sql",
]);
const DATA_EXTENSIONS = new Set(["json", "jsonc", "json5", "csv", "tsv", "env", "lock"]);
const DOCUMENT_EXTENSIONS = new Set(["doc", "docx", "odt", "pdf"]);
const SHEET_EXTENSIONS = new Set(["xls", "xlsx", "ods"]);
const PRESENTATION_EXTENSIONS = new Set(["ppt", "pptx", "odp", "key"]);
const IMAGE_EXTENSIONS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "bmp",
  "svg",
  "ico",
  "tif",
  "tiff",
  "avif",
]);
const AUDIO_EXTENSIONS = new Set(["mp3", "wav", "ogg", "flac", "aac", "m4a", "opus"]);
const VIDEO_EXTENSIONS = new Set(["mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "ts"]);
const ARCHIVE_EXTENSIONS = new Set(["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz"]);
const DATABASE_EXTENSIONS = new Set(["db", "sqlite", "sqlite3", "mdb"]);
const EXECUTABLE_EXTENSIONS = new Set(["exe", "msi", "app", "apk", "deb", "rpm", "bin"]);
const KEY_EXTENSIONS = new Set(["pem", "key", "crt", "cer", "p12", "pfx", "pub"]);
const JSON_EXTENSIONS = new Set(["json", "jsonc", "json5"]);
const SHELL_EXTENSIONS = new Set(["sh", "bash", "zsh", "fish", "ps1", "bat", "cmd"]);

function extensionOf(name) {
  const value = String(name || "");
  const dot = value.lastIndexOf(".");
  if (dot <= 0 || dot === value.length - 1) return "";
  return value.slice(dot + 1).toLowerCase();
}

export function iconForSftpEntry(entry) {
  if (!entry) return File;
  if (entry.kind === "dir") return Folder;
  if (entry.kind === "symlink") {
    return entry.targetKind === "dir" ? FolderSymlink : FileSymlink;
  }

  const ext = extensionOf(entry.name);
  if (SHEET_EXTENSIONS.has(ext)) return FileSpreadsheet;
  if (PRESENTATION_EXTENSIONS.has(ext)) return Presentation;
  if (IMAGE_EXTENSIONS.has(ext)) return FileImage;
  if (AUDIO_EXTENSIONS.has(ext)) return FileMusic;
  if (VIDEO_EXTENSIONS.has(ext)) return FileVideoCamera;
  if (ARCHIVE_EXTENSIONS.has(ext)) return FileArchive;
  if (DATABASE_EXTENSIONS.has(ext)) return Database;
  if (KEY_EXTENSIONS.has(ext)) return Shield;
  if (EXECUTABLE_EXTENSIONS.has(ext)) return Package;
  if (JSON_EXTENSIONS.has(ext)) return Braces;
  if (SHELL_EXTENSIONS.has(ext)) return FileTerminal;
  if (DATA_EXTENSIONS.has(ext)) return ScrollText;
  if (CODE_EXTENSIONS.has(ext)) return FileCode;
  if (DOCUMENT_EXTENSIONS.has(ext) || TEXT_EXTENSIONS.has(ext)) return FileText;
  if (!ext) return File;
  if (ext.length <= 3) return Binary;
  return File;
}
