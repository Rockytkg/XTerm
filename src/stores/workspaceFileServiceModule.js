import {
  getFileServiceConfig,
  setFileServiceBindIp,
  setFileServiceSharedDir,
  setFileServiceCredentials,
  setFileServicePassword,
  startFileService,
  stopFileService,
} from "../services/fileService";
import { observeFileServiceConfig, observeFileTransfers } from "../events/fileServiceEventBus";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.workspace.file_service");

function numeric(value, fallback = 0) {
  const next = Number(value);
  return Number.isFinite(next) ? next : fallback;
}

function transferMetrics(existing, payload) {
  const now = Date.now();
  const previousTransferred = numeric(existing?.transferred);
  const transferred = numeric(payload.transferred);
  const lastAt = numeric(existing?.lastAt, numeric(payload.startedAtMs, now));
  const elapsedMs = Math.max(1, now - lastAt);
  const deltaBytes = Math.max(0, transferred - previousTransferred);
  const instantSpeed = Math.round((deltaBytes * 1000) / elapsedMs);
  const previousSpeed = numeric(existing?.speedBytesPerSec);
  const speedBytesPerSec =
    payload.done || payload.error
      ? 0
      : previousSpeed > 0
        ? Math.round(previousSpeed * 0.65 + instantSpeed * 0.35)
        : instantSpeed;
  const total = numeric(payload.total);
  const remainingBytes = total > 0 ? Math.max(0, total - transferred) : 0;
  const etaSeconds =
    payload.done || payload.error || speedBytesPerSec <= 0 || remainingBytes <= 0
      ? null
      : Math.ceil(remainingBytes / speedBytesPerSec);
  return {
    etaSeconds,
    lastAt: now,
    remainingBytes,
    speedBytesPerSec,
  };
}

export function createWorkspaceFileServiceModule({ fileServiceConfig, fileServiceTransfers }) {
  let configRevision = 0;
  let mutationChain = Promise.resolve();
  let pendingMutations = 0;

  async function hydrateFileService() {
    logger.debug("file_service.hydrate.start");
    const revision = configRevision;
    try {
      const config = await getFileServiceConfig();
      if (revision !== configRevision || pendingMutations > 0) return;
      applyFileServiceConfig(config);
      logger.info("file_service.hydrate.success", {
        running: fileServiceConfig.value.running,
        bindIp: fileServiceConfig.value.bindIp,
      });
    } catch (error) {
      logger.error("file_service.hydrate.failed", error);
      throw error;
    }
  }

  function applyFileServiceConfig(config) {
    // 新契约的快照不再携带明文口令（只有 passwordSet 标记）；
    // 防御性剥离 password 字段，确保明文永远不进入 Pinia。
    const { password: _password, ...snapshot } = config || {};
    fileServiceConfig.value = snapshot;
    return config;
  }

  function mutateFileService(operation) {
    configRevision += 1;
    pendingMutations += 1;
    const run = async () => {
      try {
        return applyFileServiceConfig(await operation());
      } catch (error) {
        try {
          applyFileServiceConfig(await getFileServiceConfig());
        } catch (refreshError) {
          logger.error("file_service.reconcile.failed", refreshError);
        }
        throw error;
      } finally {
        pendingMutations -= 1;
      }
    };
    const result = mutationChain.then(run, run);
    mutationChain = result.catch(() => undefined);
    return result;
  }

  function startFileServiceServer(protocol) {
    const request = {
      protocol,
      bindIp: fileServiceConfig.value.bindIp,
      sharedDir: fileServiceConfig.value.sharedDir,
    };
    return mutateFileService(() => startFileService(request));
  }

  function stopFileServiceServer() {
    return mutateFileService(stopFileService);
  }

  function updateFileServiceBindIp(bindIp) {
    return mutateFileService(() => setFileServiceBindIp(bindIp));
  }

  function updateFileServiceSharedDir(sharedDir) {
    return mutateFileService(() => setFileServiceSharedDir(sharedDir));
  }

  function updateFileServiceUsername(username) {
    return mutateFileService(() => setFileServiceCredentials(username));
  }

  function updateFileServicePassword(password) {
    // file_service_set_password 返回 void：提交后重新拉取快照以刷新 passwordSet。
    return mutateFileService(async () => {
      await setFileServicePassword(password);
      return getFileServiceConfig();
    });
  }

  function applyTransfer(payload) {
    if (!payload?.transferId) return;
    const existing = fileServiceTransfers.value.find((item) => item.id === payload.transferId);
    const updatedAtMs = numeric(payload.updatedAtMs, Date.now());
    if (existing) {
      const existingUpdatedAtMs = numeric(existing.updatedAtMs);
      const existingTerminal = existing.done || Boolean(existing.error);
      const incomingTerminal = payload.done || Boolean(payload.error);
      if (
        updatedAtMs < existingUpdatedAtMs ||
        (existingTerminal && !incomingTerminal) ||
        (updatedAtMs === existingUpdatedAtMs &&
          numeric(payload.transferred) < numeric(existing.transferred))
      ) {
        return;
      }
    }
    const metrics = transferMetrics(existing, payload);
    const next = {
      id: payload.transferId,
      name: payload.name || existing?.name || "transfer",
      direction: payload.direction || existing?.direction || "read",
      peer: payload.peer || existing?.peer || "",
      transferred: numeric(payload.transferred),
      total: numeric(payload.total),
      startedAtMs: numeric(payload.startedAtMs, numeric(existing?.startedAtMs, Date.now())),
      updatedAtMs,
      done: !!payload.done,
      error: payload.error || "",
      ...metrics,
    };
    const transfers = [...fileServiceTransfers.value];
    const index = transfers.findIndex((item) => item.id === payload.transferId);
    if (index >= 0) {
      transfers[index] = { ...transfers[index], ...next };
    } else {
      transfers.unshift(next);
    }
    fileServiceTransfers.value = transfers.slice(0, 24);
  }

  async function startObserving() {
    const results = await Promise.allSettled([
      observeFileTransfers(applyTransfer),
      observeFileServiceConfig((config) => {
        if (pendingMutations > 0) return;
        configRevision += 1;
        applyFileServiceConfig(config);
      }),
    ]);
    const failed = results.find((result) => result.status === "rejected");
    if (failed) {
      for (const result of results) {
        if (result.status === "fulfilled") result.value();
      }
      throw failed.reason;
    }
    const disposers = results.map((result) => result.value);
    return () => disposers.forEach((dispose) => dispose());
  }

  function clearFileTransfers() {
    fileServiceTransfers.value = fileServiceTransfers.value.filter(
      (item) => !item.done && !item.error,
    );
  }

  return {
    clearFileTransfers,
    hydrateFileService,
    startObserving,
    startFileServiceServer,
    stopFileServiceServer,
    updateFileServiceBindIp,
    updateFileServiceSharedDir,
    updateFileServiceUsername,
    updateFileServicePassword,
  };
}
