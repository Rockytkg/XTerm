import { onScopeDispose, ref } from "vue";
import { createLogger } from "../utils/logger.js";
import { createRuntimeId } from "../utils/runtimeIds.js";
import { getSetting, setPreference } from "../services/preferences.js";
import { normalizeQuickButton } from "../utils/quickButtons.js";

export { DEFAULT_COLOR, normalizeQuickButton } from "../utils/quickButtons.js";

const quickButtonsLogger = createLogger("frontend.statusbar.quickButtons");

// 快捷按钮整体作为一个 JSON 存入 settings 表（与 userScripts 同一模式），
// 不新增存储表；setting_get/setting_set 为通用 key-value 命令。
// 不能挂在 preferences 对象上：后端 AppPreferences 无此字段，
// 启动时 hydratePreferences 会用后端快照整体替换，字段会丢失。
const QUICK_BUTTONS_SETTING_KEY = "quickButtons";
const PERSIST_DEBOUNCE_MS = 300;

const buttons = ref([]);
const loaded = ref(false);
let persistTimer = null;
let persistChain = Promise.resolve();
let loadPromise = null;

async function loadQuickButtonsOnce() {
  try {
    const raw = await getSetting(QUICK_BUTTONS_SETTING_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      buttons.value = Array.isArray(parsed) ? parsed.map(normalizeQuickButton).filter(Boolean) : [];
    }
  } catch (error) {
    // 读取失败（首次运行无此 key、JSON 损坏）按空列表处理，不阻塞界面。
    quickButtonsLogger.warn("failed to load quick buttons:", error);
    buttons.value = [];
  }
  loaded.value = true;
}

function loadQuickButtons() {
  if (loaded.value) return Promise.resolve();
  if (loadPromise) return loadPromise;
  loadPromise = loadQuickButtonsOnce().finally(() => {
    loadPromise = null;
  });
  return loadPromise;
}

function persistNow() {
  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  const value = JSON.stringify(buttons.value);
  const request = persistChain
    .catch(() => {})
    .then(() => setPreference(QUICK_BUTTONS_SETTING_KEY, value));
  persistChain = request;
  return request;
}

function schedulePersist() {
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    void persistNow().catch((error) => {
      quickButtonsLogger.warn("failed to persist quick buttons:", error);
    });
  }, PERSIST_DEBOUNCE_MS);
}

function upsert(item) {
  const next = normalizeQuickButton(item?.id ? item : { ...item, id: createRuntimeId() });
  if (!next) return;
  const list = [...buttons.value];
  const index = list.findIndex((entry) => entry.id === next.id);
  if (index >= 0) list[index] = next;
  else list.push(next);
  buttons.value = list;
  schedulePersist();
}

function remove(id) {
  buttons.value = buttons.value.filter((entry) => entry.id !== id);
  schedulePersist();
}

export function useQuickButtons() {
  onScopeDispose(() => {
    // debounce 窗口内还有未落盘的变更时，作用域销毁前 flush 一次，避免修改丢失。
    if (persistTimer) {
      void persistNow().catch((error) => {
        quickButtonsLogger.warn("failed to persist quick buttons:", error);
      });
    }
  });
  return { buttons, loaded, loadQuickButtons, upsert, remove };
}
