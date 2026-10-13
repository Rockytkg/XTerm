import { onScopeDispose } from "vue";
import { createLogger } from "../utils/logger.js";
import { createRuntimeId } from "../utils/runtimeIds.js";
import { createJsonSettingPersistence } from "../services/jsonSettingPersistence.js";
import { normalizeQuickButton } from "../utils/quickButtons.js";

export { DEFAULT_COLOR, normalizeQuickButton } from "../utils/quickButtons.js";

const quickButtonsLogger = createLogger("frontend.statusbar.quickButtons");

// 快捷按钮整体作为一个 JSON 存入 settings 表（与 userScripts 同一模式），
// 不新增存储表；setting_get/setting_set 为通用 key-value 命令。
// 不能挂在 preferences 对象上：后端 AppPreferences 无此字段，
// 启动时 hydratePreferences 会用后端快照整体替换，字段会丢失。
const QUICK_BUTTONS_SETTING_KEY = "quickButtons";

const quickButtonsPersistence = createJsonSettingPersistence({
  key: QUICK_BUTTONS_SETTING_KEY,
  normalize: normalizeQuickButton,
  onLoadError: (error) => quickButtonsLogger.warn("failed to load quick buttons:", error),
  onPersistError: (error) => quickButtonsLogger.warn("failed to persist quick buttons:", error),
});

const buttons = quickButtonsPersistence.items;
const loaded = quickButtonsPersistence.loaded;
const loadQuickButtons = quickButtonsPersistence.load;

function upsert(item) {
  const next = normalizeQuickButton(item?.id ? item : { ...item, id: createRuntimeId() });
  if (!next) return;
  const list = [...buttons.value];
  const index = list.findIndex((entry) => entry.id === next.id);
  if (index >= 0) list[index] = next;
  else list.push(next);
  buttons.value = list;
  quickButtonsPersistence.schedulePersist();
}

function remove(id) {
  buttons.value = buttons.value.filter((entry) => entry.id !== id);
  quickButtonsPersistence.schedulePersist();
}

export function useQuickButtons() {
  onScopeDispose(() => {
    quickButtonsPersistence.flushPendingPersist();
  });
  return { buttons, loaded, loadQuickButtons, upsert, remove };
}
