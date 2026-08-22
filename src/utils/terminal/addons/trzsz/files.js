import { base64ToBytes, bytesToBase64 } from "./bytes";
import { EMPTY_MD5 } from "./constants";
import { TransferError } from "./errors";
import {
  beginDownload,
  chooseUploadEntries,
  ensureDirectory,
  finishDownload,
  finishUploadChecksum,
  getDownloadChecksum,
  listDirectory,
  readFileChunk,
  registerDragPaths,
  writeDownloadChunk,
} from "./ipc";

export function checkDuplicateNames(files) {
  const names = new Set();
  for (const file of files) {
    const name = file.getRelPath().join("/");
    if (names.has(name)) throw new TransferError(`Duplicate name: ${name}`);
    names.add(name);
  }
}

class TauriFileReader {
  constructor(pathId, relPath, entry, directory = false) {
    this._pathId = pathId;
    this._relPath = relPath;
    this._entry = entry;
    this._directory = directory;
    this._offset = 0;
    this._closed = false;
  }

  getPathId() {
    return this._pathId;
  }

  getRelPath() {
    return this._relPath;
  }

  isDir() {
    return this._directory;
  }

  getSize() {
    return Number(this._entry?.size || 0);
  }

  async readFile(buffer) {
    if (this._closed || this._directory || this._offset >= this.getSize()) return new Uint8Array();
    const length = Math.min(buffer.byteLength, this.getSize() - this._offset);
    const chunk = await readFileChunk({
      entryId: this._entry.entryId,
      offset: this._offset,
      length,
    });
    const bytes = base64ToBytes(chunk?.dataBase64);
    this._offset += bytes.length;
    return bytes;
  }

  consumeDigest() {}

  async finishDigest() {
    const result = await finishUploadChecksum(this._entry.entryId);
    return base64ToBytes(result?.digestBase64);
  }

  closeFile() {
    this._closed = true;
  }
}

class TauriFileWriter {
  constructor({ fileName, localName, directory = false, directoryId = "", transfer = null }) {
    this._fileName = fileName;
    this._localName = localName;
    this._directory = directory;
    this._directoryId = directoryId;
    this._transfer = transfer;
    this._closed = false;
    this._chain = Promise.resolve();
  }

  getFileName() {
    return this._fileName;
  }

  getLocalName() {
    return this._localName;
  }

  isDir() {
    return this._directory;
  }

  async writeFile(bytes) {
    if (this._directory || !bytes?.length) return;
    this._chain = this._chain.then(async () => {
      if (!this._transfer) {
        this._transfer = await beginDownload({
          directoryId: this._directoryId,
          fileName: this._fileName,
        });
        this._localName = this._transfer?.entry?.name || this._localName;
      }
      await writeDownloadChunk({
        transferId: this._transfer.transferId,
        dataBase64: bytesToBase64(bytes),
      });
    });
    await this._chain;
  }

  async closeFile() {
    if (this._closed || this._directory) return;
    this._closed = true;
    await this._chain;
    if (this._transfer?.transferId) {
      await finishDownload({ transferId: this._transfer.transferId, aborted: false });
    }
  }

  async getDigest() {
    if (this._directory || !this._transfer?.transferId) return EMPTY_MD5;
    const result = await getDownloadChecksum(this._transfer.transferId);
    return base64ToBytes(result?.digestBase64);
  }

  async deleteFile() {
    if (this._transfer?.transferId) {
      await finishDownload({ transferId: this._transfer.transferId, aborted: true });
    }
    return "";
  }
}

async function collectTauriReaders(entry, pathId, relPath, output) {
  const isDirectory = entry.kind === "directory";
  output.push(new TauriFileReader(pathId, relPath, entry, isDirectory));
  if (!isDirectory) return;
  const children = await listDirectory(entry.entryId);
  for (const child of children) {
    await collectTauriReaders(child, pathId, [...relPath, child.name], output);
  }
}

export async function chooseSendFiles({ directory, messages }) {
  const entries = await chooseUploadEntries({
    directory,
    title: directory ? messages.chooseUploadDirectoryTitle : messages.chooseUploadTitle,
    allFilesLabel: messages.allFilesLabel,
  });
  if (!entries?.length) return undefined;
  const readers = [];
  for (const [index, entry] of entries.entries()) {
    await collectTauriReaders(entry, index, [entry.name], readers);
  }
  return readers;
}

export async function parseDragPaths(paths) {
  const entries = await registerDragPaths(paths);
  if (!entries?.length) return [];
  const readers = [];
  for (const [index, entry] of entries.entries()) {
    await collectTauriReaders(entry, index, [entry.name], readers);
  }
  return readers;
}

export async function openSaveFile(saveParam, encodedName, directory, overwrite) {
  if (!directory) {
    return new TauriFileWriter({
      fileName: encodedName,
      localName: encodedName,
      directoryId: saveParam.root.entryId,
    });
  }
  const info = parseDirectoryFileName(encodedName);
  const rootName = await resolveDownloadRootName(
    saveParam,
    info.path_id,
    info.path_name[0],
    overwrite,
  );
  let parent = saveParam.root;
  let localName = rootName;
  if (info.path_name.length > 1) {
    parent = await ensureNamedDirectory(parent.entryId, rootName);
    for (let index = 1; index < info.path_name.length - 1; index += 1) {
      parent = await ensureNamedDirectory(parent.entryId, info.path_name[index]);
    }
  }
  const fileName = info.path_name[info.path_name.length - 1];
  if (info.is_dir === true) {
    if (info.path_name.length === 1) {
      await ensureNamedDirectory(parent.entryId, rootName);
    } else {
      await ensureNamedDirectory(parent.entryId, fileName);
    }
    return new TauriFileWriter({ fileName, localName, directory: true });
  }
  return new TauriFileWriter({
    fileName,
    localName,
    directoryId: parent.entryId,
  });
}

function parseDirectoryFileName(encodedName) {
  let info;
  try {
    info = JSON.parse(encodedName);
  } catch {
    throw new TransferError(`Invalid name: ${encodedName}`);
  }
  if (
    !Array.isArray(info.path_name) ||
    info.path_name.length < 1 ||
    !Object.hasOwn(info, "path_id")
  ) {
    throw new TransferError(`Invalid name: ${encodedName}`);
  }
  return info;
}

async function resolveDownloadRootName(saveParam, pathId, preferredName, overwrite) {
  if (overwrite) return preferredName;
  if (saveParam.maps.has(pathId)) return saveParam.maps.get(pathId);
  const name = await getAvailableDirectoryName(saveParam.root.entryId, preferredName);
  saveParam.maps.set(pathId, name);
  return name;
}

async function getAvailableDirectoryName(directoryId, preferredName) {
  const entries = await listDirectory(directoryId);
  const existing = new Set(entries.map((entry) => entry.name));
  if (!existing.has(preferredName)) return preferredName;
  for (let index = 1; index < 1000; index += 1) {
    const candidate = `${preferredName} (${index})`;
    if (!existing.has(candidate)) return candidate;
  }
  return preferredName;
}

async function ensureNamedDirectory(directoryId, name) {
  const entries = await listDirectory(directoryId);
  const existing = entries.find((entry) => entry.kind === "directory" && entry.name === name);
  if (existing) return existing;
  return ensureDirectory({ directoryId, name });
}
