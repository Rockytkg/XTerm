import { invokeLoggedIpc } from "../../../../services/ipc/core";

export function registerDragPaths(paths) {
  return invokeLoggedIpc("trzsz_register_drag_paths", { paths });
}

export function chooseUploadEntries(request = {}) {
  return invokeLoggedIpc("trzsz_choose_upload_entries", { request });
}

export function chooseDownloadDirectory(request = {}) {
  return invokeLoggedIpc("trzsz_choose_download_directory", { request });
}

export function listDirectory(entryId) {
  return invokeLoggedIpc("trzsz_list_directory", { request: { entryId } });
}

export function readFileChunk({ entryId, offset, length }) {
  return invokeLoggedIpc("trzsz_read_file_chunk", {
    request: { entryId, offset, length },
  });
}

export function ensureDirectory({ directoryId, name }) {
  return invokeLoggedIpc("trzsz_ensure_directory", {
    request: { directoryId, name },
  });
}

export function beginDownload({ directoryId, fileName }) {
  return invokeLoggedIpc("trzsz_begin_download", {
    request: { directoryId, fileName },
  });
}

export function writeDownloadChunk({ transferId, dataBase64 }) {
  return invokeLoggedIpc("trzsz_write_download_chunk", {
    request: { transferId, dataBase64 },
  });
}

export function finishDownload({ transferId, aborted = false }) {
  return invokeLoggedIpc("trzsz_finish_download", {
    request: { transferId, aborted },
  });
}

export function finishUploadChecksum(entryId) {
  return invokeLoggedIpc("trzsz_finish_upload_checksum", { request: { entryId } });
}

export function getDownloadChecksum(checksumId) {
  return invokeLoggedIpc("trzsz_get_download_checksum", { request: { checksumId } });
}
