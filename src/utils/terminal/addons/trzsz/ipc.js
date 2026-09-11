import { invokeLoggedIpc } from "../../../../services/ipc/core.js";

const cmd = (name) => (request) => invokeLoggedIpc(name, { request });

export function registerDragPaths(paths) {
  return invokeLoggedIpc("trzsz_register_drag_paths", { paths });
}

export const chooseUploadEntries = cmd("trzsz_choose_upload_entries");
export const chooseDownloadDirectory = cmd("trzsz_choose_download_directory");
export const listDirectory = (entryId) =>
  invokeLoggedIpc("trzsz_list_directory", { request: { entryId } });
export const readFileChunk = cmd("trzsz_read_file_chunk");
export const ensureDirectory = cmd("trzsz_ensure_directory");
export const beginDownload = cmd("trzsz_begin_download");
export const writeDownloadChunk = cmd("trzsz_write_download_chunk");
export const finishDownload = ({ transferId, aborted = false }) =>
  invokeLoggedIpc("trzsz_finish_download", { request: { transferId, aborted } });
export const finishUploadChecksum = (entryId) =>
  invokeLoggedIpc("trzsz_finish_upload_checksum", { request: { entryId } });
export const getDownloadChecksum = (checksumId) =>
  invokeLoggedIpc("trzsz_get_download_checksum", { request: { checksumId } });
