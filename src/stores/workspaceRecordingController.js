import { ref } from "vue";
import { i18n } from "../i18n";
import { invokeLoggedIpc } from "../services/ipc/core";
import { registerRecordingBridge } from "../services/scripting/bridges";
import { createTerminalRecordingNormalizer } from "./terminalRecording";
import { timestampForFileName } from "./workspaceUtils";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.workspace.recording");

const RECORDING_APPEND_FLUSH_MS = 120;
const RECORDING_APPEND_MAX_CHARS = 32 * 1024;

function createBlankRecording() {
  return {
    active: false,
    path: "",
    error: "",
    startedAt: "",
    recordingWriteQueue: Promise.resolve(),
    pendingRecordingText: "",
    pendingRecordingTimer: null,
    recordingNormalizer: createTerminalRecordingNormalizer(),
  };
}

// ── session-scoped helpers ──────────────────────────────────────────

function queueAppend(session, path, data) {
  session.recordingWriteQueue = session.recordingWriteQueue
    .catch((previousError) => {
      logger.error("recording.write.failed", previousError);
    })
    .then(() => invokeLoggedIpc("session_recording_append", { request: { path, data } }));
  return session.recordingWriteQueue;
}

function clearTimer(session) {
  if (!session.pendingRecordingTimer) return;
  clearTimeout(session.pendingRecordingTimer);
  session.pendingRecordingTimer = null;
}

function flushPending(session) {
  clearTimer(session);
  const path = session.path;
  const data = session.pendingRecordingText;
  session.pendingRecordingText = "";
  if (!session.active || !path || !data) return Promise.resolve();
  return queueAppend(session, path, data).catch((error) => {
    logger.error("recording.append.failed", error);
    session.active = false;
    session.error = String(error);
  });
}

function scheduleFlush(session) {
  if (session.pendingRecordingTimer) return;
  session.pendingRecordingTimer = setTimeout(() => {
    session.pendingRecordingTimer = null;
    void flushPending(session);
  }, RECORDING_APPEND_FLUSH_MS);
}

// ── public controller ───────────────────────────────────────────────

export function createWorkspaceRecordingController() {
  /** @type {Map<string, ReturnType<typeof createBlankRecording>>} */
  const recordings = new Map();
  const sessionRecordings = ref(new Map());

  function updateSnapshot() {
    const snapshot = new Map();
    for (const [id, session] of recordings) {
      snapshot.set(id, {
        active: session.active,
        path: session.path,
        error: session.error,
        startedAt: session.startedAt,
      });
    }
    sessionRecordings.value = snapshot;
  }

  function getSessionRecording(connectionId) {
    if (!connectionId) return null;
    return recordings.get(connectionId) ?? null;
  }

  function ensureSessionRecording(connectionId) {
    if (!connectionId) return null;
    let session = recordings.get(connectionId);
    if (!session) {
      session = createBlankRecording();
      recordings.set(connectionId, session);
    }
    return session;
  }

  function isRecording(connectionId) {
    return !!getSessionRecording(connectionId)?.active;
  }

  // 返回值：成功为写入路径，用户取消或失败为空串（脚本侧据此区分"取消"）。
  async function startSessionRecording(connectionId) {
    if (!connectionId) {
      logger.warn("recording.start.no_connection_id");
      return "";
    }

    const session = ensureSessionRecording(connectionId);

    const startedAt = new Date();
    const selectedPath = await invokeLoggedIpc("session_recording_choose_file", {
      request: {
        defaultFileName: `${timestampForFileName(startedAt)}.txt`,
        title: i18n.global.t("recordingDialog.title"),
        textFilesLabel: i18n.global.t("recordingDialog.textFiles"),
        allFilesLabel: i18n.global.t("recordingDialog.allFiles"),
      },
    });
    if (!selectedPath) {
      recordings.delete(connectionId);
      updateSnapshot();
      return "";
    }

    logger.info("recording.start", connectionId, selectedPath);
    session.recordingWriteQueue = Promise.resolve();
    clearTimer(session);
    session.pendingRecordingText = "";
    session.recordingNormalizer.reset();
    session.active = true;
    session.path = selectedPath;
    session.error = "";
    session.startedAt = startedAt.toISOString();
    updateSnapshot();
    return selectedPath;
  }

  // 返回值：刚结束记录的写入路径；本就没有记录时为空串。
  async function stopSessionRecording(connectionId) {
    const session = getSessionRecording(connectionId);
    if (!session) return "";

    logger.info("recording.stop", connectionId, session.path);
    const path = session.path;
    if (path) {
      await flushPending(session);
      const pendingText = session.recordingNormalizer.normalize("", { flush: true });
      if (pendingText) {
        await queueAppend(session, path, pendingText).catch((error) => {
          logger.error("recording.flush.failed", error);
        });
      }
    }
    session.active = false;
    session.path = "";
    session.error = "";
    session.startedAt = "";
    clearTimer(session);
    session.pendingRecordingText = "";
    session.recordingNormalizer.reset();
    updateSnapshot();
    return path;
  }

  async function toggleSessionRecording(connectionId) {
    if (isRecording(connectionId)) {
      await stopSessionRecording(connectionId);
      return true;
    }
    return !!((await startSessionRecording(connectionId)) || "");
  }

  function recordTerminalChunk(connectionId, data) {
    if (!connectionId || !data) return;
    const session = getSessionRecording(connectionId);
    if (!session || !session.active || !session.path) return;

    const text = session.recordingNormalizer.normalize(data);
    if (!text) return;
    session.pendingRecordingText += text;
    if (session.pendingRecordingText.length >= RECORDING_APPEND_MAX_CHARS) {
      void flushPending(session);
      return;
    }
    scheduleFlush(session);
  }

  async function closeSessionRecording(connectionId) {
    const session = getSessionRecording(connectionId);
    if (!session) return;

    if (session.active) {
      await stopSessionRecording(connectionId);
    }
    recordings.delete(connectionId);
    updateSnapshot();
  }

  // 注册给脚本引擎：脚本经 recordingBridge 按会话 id 启停记录，
  // 路径仍由原生保存对话框决定。
  registerRecordingBridge({
    start: startSessionRecording,
    stop: stopSessionRecording,
    isActive: isRecording,
  });

  return {
    recordTerminalChunk,
    sessionRecordings,
    startSessionRecording,
    stopSessionRecording,
    toggleSessionRecording,
    closeSessionRecording,
  };
}
