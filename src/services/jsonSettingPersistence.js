import { ref } from "vue";
import { getSetting, setPreference } from "./preferences";

const PERSIST_DEBOUNCE_MS = 300;

/**
 * "JSON-in-settings" 持久化骨架：整个列表作为一个 JSON 字符串存入 settings 表
 * （setting_get/setting_set 为通用 key-value 命令，不新增存储表）。
 * 提供 load 的 in-flight Promise 去重、300ms 防抖 + persistChain 串行落盘；
 * 作用域销毁前的 flush 由调用方把 flushPendingPersist 挂到自己的 onScopeDispose
 * （模块级单例在创建时没有活动作用域，不能在此自注册）。
 * normalize 与日志文案（onLoadError/onPersistError）保留在调用方。
 */
export function createJsonSettingPersistence({
  key,
  normalize,
  onLoadError,
  onPersistError,
  afterLoad,
  onLoaded,
}) {
  const items = ref([]);
  const loaded = ref(false);
  let persistTimer = null;
  let persistChain = Promise.resolve();
  let loadPromise = null;
  // 读取失败（JSON 损坏或后端读失败）后禁止回写：空列表防抖落盘会把库中
  // 仍可人工恢复的原始数据永久覆盖。此时界面只读展示空列表。
  let persistBlocked = false;

  async function loadOnce() {
    try {
      const raw = await getSetting(key);
      if (raw) {
        const parsed = JSON.parse(raw);
        items.value = Array.isArray(parsed) ? parsed.map(normalize).filter(Boolean) : [];
      }
    } catch (error) {
      // 读取失败按空列表处理，不阻塞界面。
      persistBlocked = true;
      onLoadError?.(error);
      items.value = [];
    }
    await afterLoad?.();
    loaded.value = true;
    onLoaded?.();
  }

  function load() {
    if (loaded.value) return Promise.resolve();
    if (loadPromise) return loadPromise;
    loadPromise = loadOnce().finally(() => {
      loadPromise = null;
    });
    return loadPromise;
  }

  function persistNow() {
    if (persistBlocked) return Promise.resolve();
    if (persistTimer) {
      clearTimeout(persistTimer);
      persistTimer = null;
    }
    const value = JSON.stringify(items.value);
    const request = persistChain.catch(() => {}).then(() => setPreference(key, value));
    persistChain = request;
    return request;
  }

  function schedulePersist() {
    if (persistBlocked) return;
    if (persistTimer) clearTimeout(persistTimer);
    persistTimer = setTimeout(() => {
      persistTimer = null;
      void persistNow().catch((error) => onPersistError?.(error));
    }, PERSIST_DEBOUNCE_MS);
  }

  // debounce 窗口内还有未落盘的变更时，销毁前 flush 一次，避免修改丢失。
  function flushPendingPersist() {
    if (persistTimer) {
      void persistNow().catch((error) => onPersistError?.(error));
    }
  }

  return {
    flushPendingPersist,
    items,
    load,
    loaded,
    persistNow,
    schedulePersist,
  };
}
