import { onBeforeUnmount, shallowRef, watch } from "vue";
import {
  readRemoteSftpFile,
  readRemoteSftpFileBase64,
  statRemoteSftpFile,
  writeRemoteSftpFile,
} from "../services/sftp";
import { createSftpSessionScope } from "./sftpSessionScope";
import { remoteNameFromPath } from "./sftpRemoteOperations";
import {
  base64ToBytes,
  classifySftpPreview,
  PREVIEW_BINARY_MAX_BYTES,
  resolveSniffedPreview,
} from "../utils/filePreview";

function safeMessage(error) {
  if (typeof error === "string") return error;
  if (error?.message) return error.message;
  return String(error || "Unknown error");
}

// 单文件打开模型：openFile 为当前打开的预览/编辑文件，再次打开即替换（替换前做脏检查）。
export function useSftpOpenFiles({
  props,
  t,
  refreshCurrentDirectoryIncremental = () => {},
  requestDirtyAction = async () => "cancel",
  requestOverwriteAction = async () => false,
}) {
  // 必须是 shallowRef：tab 更新一律经 patchTab 整体替换，且 isCurrent 用对象同一性
  // 判断当前文件——深响应式的 ref 会让 openFile.value 返回 reactive 代理，代理 !== 原始
  // tab，所有异步结果都会被守卫静默丢弃（表现为永远"正在加载"）。
  const openFile = shallowRef(null);
  const { currentSession, isStaleSession, setDisposed } = createSftpSessionScope(props);

  // 异步响应只在对应文件仍打开且会话未切换时生效，避免串文件/串会话写入
  function isCurrent(tab, session) {
    return openFile.value === tab && !isStaleSession(session);
  }

  function patchTab(tab, patch) {
    if (openFile.value !== tab) return;
    openFile.value = { ...tab, ...patch };
  }

  // patchTab 会整体替换 openFile（新对象），关闭/保存后的同一性判断需按路径+模式识别同一文件
  function isSameOpenFile(a, b) {
    return !!a && !!b && a.path === b.path && a.mode === b.mode;
  }

  // modified/mtime 用 ?? 回退：mtime 为 0（epoch）是合法值，|| 会错误回退
  function statPatch(stat, fallback = {}) {
    return {
      size: Number.isFinite(Number(stat?.size)) ? Number(stat.size) : (fallback.size ?? null),
      modified: stat?.modified ?? fallback.modified ?? null,
      mtime: stat?.modified ?? fallback.mtime ?? null,
    };
  }

  function createTab(entry, mode) {
    const { editable, mime, tooLarge } = classifySftpPreview({
      name: entry.name,
      size: entry.size,
    });
    return {
      path: entry.path,
      name: entry.name || remoteNameFromPath(entry.path),
      mode,
      editable,
      mime,
      content: "",
      blob: null,
      savedContent: "",
      modified: entry.modified ?? null,
      loading: true,
      saving: false,
      error: "",
      tooLarge,
      size: Number.isFinite(Number(entry.size)) ? Number(entry.size) : null,
      mtime: entry.modified ?? null,
    };
  }

  async function loadTextContent(path, session) {
    const [content, stat] = await Promise.all([
      readRemoteSftpFile(session.connectionId, session.sessionId, path),
      statRemoteSftpFile(session.connectionId, session.sessionId, path),
    ]);
    return { content, stat };
  }

  async function loadPreviewTab(tab, session) {
    try {
      const stat = await statRemoteSftpFile(session.connectionId, session.sessionId, tab.path);
      if (!isCurrent(tab, session)) return;
      const meta = statPatch(stat, tab);
      const tooLarge = tab.tooLarge || Number(meta.size ?? 0) > PREVIEW_BINARY_MAX_BYTES;
      if (tooLarge) {
        patchTab(tab, { ...meta, tooLarge: true, loading: false });
        return;
      }

      // 无扩展名文件采用后端嗅探 mime（有扩展名时 resolveSniffedPreview 返回 null，按扩展名匹配）
      const sniffed = resolveSniffedPreview(tab.name, meta.size, stat?.mime);
      const base64 = await readRemoteSftpFileBase64(
        session.connectionId,
        session.sessionId,
        tab.path,
      );
      if (!isCurrent(tab, session)) return;
      patchTab(tab, {
        ...meta,
        tooLarge: false,
        mime: sniffed.mime,
        editable: tab.editable || sniffed.editable,
        blob: new Blob([base64ToBytes(base64)], { type: sniffed.mime || tab.mime }),
        loading: false,
        error: "",
      });
    } catch (error) {
      if (!isCurrent(tab, session)) return;
      patchTab(tab, {
        loading: false,
        error: `${t("sftp.preview.loadFailed")}: ${safeMessage(error)}`,
      });
    }
  }

  // 替换/关闭当前文件前的脏检查：save 保存后继续，discard 直接继续，其余取消
  async function confirmReplace(tab) {
    if (!tab || tab.mode !== "edit" || tab.content === tab.savedContent) return true;
    const action = await requestDirtyAction({ kind: "close", tab });
    if (action === "save") return saveEditor(tab);
    return action === "discard";
  }

  async function replaceOpenFile(entry, mode, session) {
    if (!(await confirmReplace(openFile.value))) return null;
    // 脏检查弹窗等待期间会话可能已切换（会话 watch 已清空 openFile）：
    // 此时再挂新 tab 会停在加载态且属于旧会话，直接放弃
    if (isStaleSession(session)) return null;
    const tab = createTab(entry, mode);
    openFile.value = tab;
    return tab;
  }

  async function openPreview(entries) {
    const files = (Array.isArray(entries) ? entries : [entries]).filter(
      (entry) => entry && entry.kind !== "dir" && entry.path,
    );
    if (!files.length) return;

    const session = currentSession();
    if (!session || isStaleSession(session)) return;

    // 多选传入时逐个替换，最后一个生效
    for (const entry of files) {
      const tab = await replaceOpenFile(entry, "preview", session);
      if (!tab) return;
      await loadPreviewTab(tab, session);
    }
  }

  async function openEditor(entry) {
    if (!entry || entry.kind === "dir") return;

    const existing = openFile.value;
    if (existing?.path === entry.path) {
      if (existing.mode !== "edit") await convertToEdit();
      return;
    }

    const session = currentSession();
    if (!session || isStaleSession(session)) return;

    const tab = await replaceOpenFile(entry, "edit", session);
    if (!tab) return;

    try {
      const { content, stat } = await loadTextContent(entry.path, session);
      if (!isCurrent(tab, session)) return;
      patchTab(tab, {
        content,
        savedContent: content,
        ...statPatch(stat, { modified: entry.modified, mtime: entry.modified }),
        loading: false,
        error: "",
      });
    } catch (error) {
      if (!isCurrent(tab, session)) return;
      patchTab(tab, {
        loading: false,
        error: `${t("sftp.editor.openFailed")}: ${safeMessage(error)}`,
      });
    }
  }

  // saveEditor/convertToEdit 自身会先 patchTab（saving/loading 置位）替换 openFile，
  // 之后的守卫不能再按对象同一性判断——按路径+模式识别同一文件的最新版本，
  // 同时仍能拦住"被替换成别的文件"和会话切换。
  function currentTab(tab, session) {
    const current = openFile.value;
    return isSameOpenFile(current, tab) && !isStaleSession(session) ? current : null;
  }

  async function convertToEdit() {
    const tab = openFile.value;
    if (!tab || tab.mode === "edit" || !tab.editable || tab.tooLarge) {
      return false;
    }

    const session = currentSession();
    if (!session || isStaleSession(session)) return false;
    patchTab(tab, { loading: true, error: "" });
    try {
      const { content, stat } = await loadTextContent(tab.path, session);
      const current = currentTab(tab, session);
      if (!current) return false;
      patchTab(current, {
        mode: "edit",
        content,
        savedContent: content,
        blob: null,
        ...statPatch(stat, current),
        loading: false,
        error: "",
      });
      return true;
    } catch (error) {
      const current = currentTab(tab, session);
      if (!current) return false;
      patchTab(current, {
        loading: false,
        error: `${t("sftp.editor.openFailed")}: ${safeMessage(error)}`,
      });
      return false;
    }
  }

  async function saveEditor(tab = openFile.value) {
    if (!tab || tab.mode !== "edit" || tab.loading || tab.saving) return false;

    const session = currentSession();
    if (!session || isStaleSession(session)) return false;
    patchTab(tab, { saving: true, error: "" });

    try {
      const latest = await statRemoteSftpFile(session.connectionId, session.sessionId, tab.path);
      let current = currentTab(tab, session);
      if (!current) return false;
      if (
        current.modified &&
        latest?.modified &&
        latest.modified !== current.modified &&
        !(await requestOverwriteAction({ kind: "overwrite", tab: current }))
      ) {
        current = currentTab(tab, session);
        if (!current) return false;
        patchTab(current, { saving: false });
        return false;
      }

      const saved = await writeRemoteSftpFile(
        session.connectionId,
        session.sessionId,
        current.path,
        current.content,
      );
      current = currentTab(tab, session);
      if (!current) return false;
      await refreshCurrentDirectoryIncremental();
      current = currentTab(tab, session);
      if (!current) return false;
      const modified = saved?.modified ?? latest?.modified ?? current.modified;
      patchTab(current, {
        savedContent: current.content,
        modified,
        mtime: modified,
        saving: false,
        error: "",
      });
      return true;
    } catch (error) {
      const current = currentTab(tab, session);
      if (!current) return false;
      patchTab(current, {
        saving: false,
        error: `${t("sftp.editor.saveFailed")}: ${safeMessage(error)}`,
      });
      return false;
    }
  }

  async function closeEditor(tab = openFile.value) {
    if (!tab) return false;
    if (!(await confirmReplace(tab))) return false;
    // 脏检查里选择"保存"时 saveEditor 已经 patchTab 替换过 openFile（新对象 !== tab），
    // 不能再按对象同一性判断，否则保存后文件关不掉
    if (isSameOpenFile(openFile.value, tab)) openFile.value = null;
    return true;
  }

  function updateEditorContent(content) {
    const tab = openFile.value;
    if (!tab || tab.mode !== "edit") return;
    patchTab(tab, { content });
  }

  onBeforeUnmount(() => {
    setDisposed(true);
  });

  // 重连（sessionId 变化）或换连接后，打开的文件属于已失效的旧会话：继续保留会让
  // 保存经 currentSession() 写入新会话的同路径文件（跨会话写），且加载中的 tab 永远
  // 停在加载态。直接丢弃；在途的加载/保存由 isCurrent 守卫自然失效。
  watch([() => props.connection?.id, () => props.sessionId], () => {
    openFile.value = null;
  });

  return {
    closeEditor,
    convertToEdit,
    openEditor,
    openFile,
    openPreview,
    saveEditor,
    updateEditorContent,
  };
}
