import { reactive, toRefs } from "vue";
import { checkForUpdates, openExternalUrl } from "../services/appInfo";
import { preferences } from "./useAppPreferences";
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
  return status;
}

// 自动检测的弹窗策略：总开关关闭、或用户已对当前最新版点过"暂不提醒"时，
// 只更新状态不弹窗；出现更新的版本后恢复提醒。
function shouldAutoNotify(status) {
  if (!status?.updateAvailable) return false;
  if (preferences.updateNotificationsEnabled === false) return false;
  return status.latestVersion !== preferences.updateSkippedVersion;
}

// 启动后延迟静默检测一次；仅在发现新版本时弹窗，失败不打扰用户。
// 唯一调用点在 App.vue setup，main.js 启动门控已完成偏好水合，可直接读 preferences。
export function scheduleAutoUpdateCheck(delayMs = 4000) {
  if (autoCheckScheduled) return;
  autoCheckScheduled = true;
  setTimeout(() => {
    runUpdateCheck()
      .then((status) => {
        if (shouldAutoNotify(status)) state.dialogOpen = true;
      })
      .catch((error) => logger.error("updates.auto-check.failed", error));
  }, delayMs);
}

// 设置页手动检测：显式用户意图，命中更新即弹模态框，不受总开关与跳过版本限制。
export function runManualUpdateCheck() {
  return runUpdateCheck().then((status) => {
    if (status?.updateAvailable) state.dialogOpen = true;
    return status;
  });
}

export function closeUpdateDialog() {
  state.dialogOpen = false;
}

// "暂不提醒"：记录当前最新版本号，此后自动检测对该版本保持静默；
// 写入 preferences 后由偏好持久化 watch 自动落盘。
export function skipUpdateVersion() {
  const version = state.status?.latestVersion;
  if (version) preferences.updateSkippedVersion = version;
  closeUpdateDialog();
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
