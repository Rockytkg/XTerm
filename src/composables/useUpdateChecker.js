import { reactive, toRefs } from "vue";
import { checkForUpdates, openExternalUrl } from "../services/appInfo";
import { pickUpdateAssetUrl } from "../utils/updateAssets";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.updates");

// 模块级共享状态：App.vue 挂载的 UpdateDialog 与设置页手动检查共用同一份
// 结果与弹窗开关，保证"启动自动检测"和"手动检测"弹出的是同一个模态框。
const state = reactive({
  status: null,
  dialogOpen: false,
});

let autoCheckScheduled = false;

async function runUpdateCheck() {
  const status = await checkForUpdates();
  state.status = status;
  if (status?.updateAvailable) {
    state.dialogOpen = true;
  }
  return status;
}

// 启动后延迟静默检测一次；仅在发现新版本时弹窗，失败不打扰用户。
export function scheduleAutoUpdateCheck(delayMs = 4000) {
  if (autoCheckScheduled) return;
  autoCheckScheduled = true;
  setTimeout(() => {
    runUpdateCheck().catch((error) => logger.error("updates.auto-check.failed", error));
  }, delayMs);
}

// 设置页手动检测：检测到更新同样弹模态框。
export function runManualUpdateCheck() {
  return runUpdateCheck();
}

export function closeUpdateDialog() {
  state.dialogOpen = false;
}

export async function downloadUpdate() {
  const url = pickUpdateAssetUrl(state.status);
  if (!url) return;
  await openExternalUrl(url);
}

export function openUpdateReleasePage() {
  const url = state.status?.releaseUrl;
  if (url) return openExternalUrl(url);
  return Promise.resolve();
}

export function useUpdateChecker() {
  return toRefs(state);
}
