import { onScopeDispose, ref } from "vue";
import { defineStore } from "pinia";
import { createLogger } from "../utils/logger";
import { createRuntimeId } from "../utils/runtimeIds";
import { getSetting, setPreference } from "../services/preferences";
import { createJsonSettingPersistence } from "../services/jsonSettingPersistence";
import { fetchScriptText } from "../services/scripting/scriptFileLoader";
import { DEFAULT_SCRIPT_BODY } from "../services/scripting/scriptTemplate";
import {
  buildScriptHeader,
  compareScriptVersions,
  parseScriptMetadata,
} from "../services/scripting/scriptMetadata";

const scriptsLogger = createLogger("frontend.scripting.store");

// 脚本整体作为一个 JSON 存入 settings 表（与 terminalHighlightSchemes 同一模式），
// 不新增存储表；setting_get/setting_set 为通用 key-value 命令。
const SCRIPTS_SETTING_KEY = "userScripts";
const AUTHOR_PROFILE_SETTING_KEY = "scriptAuthorProfile";
const UPDATE_INTERVAL_SETTING_KEY = "scriptUpdateIntervalHours";
const DEFAULT_UPDATE_INTERVAL_HOURS = 24;

// 元数据以脚本头（==XTermScript== 块）为准：这里只负责把代码里的头解析出来，
// 与存储字段合并（头里有的字段覆盖存储值），保证导入/手改头都能生效。
function normalizeScript(raw) {
  if (!raw || typeof raw !== "object") return null;
  const code = typeof raw.code === "string" ? raw.code : "";
  const header = parseScriptMetadata(code);
  const name = (header.name || String(raw.name || "")).trim();
  if (!name && !code.trim()) return null;
  return {
    id: String(raw.id || createRuntimeId()),
    code,
    name,
    author: header.author || String(raw.author || ""),
    homepage: header.homepage || String(raw.homepage || ""),
    description: header.description || String(raw.description || ""),
    version: header.version || String(raw.version || ""),
    updateUrl: header.updateUrl || String(raw.updateUrl || ""),
    updateAvailableVersion: String(raw.updateAvailableVersion || ""),
    createdAt: Number(raw.createdAt) || Date.now(),
    updatedAt: Number(raw.updatedAt) || Date.now(),
  };
}

export const useScriptsStore = defineStore("scripts", () => {
  const scriptsPersistence = createJsonSettingPersistence({
    key: SCRIPTS_SETTING_KEY,
    normalize: normalizeScript,
    onLoadError: (error) => scriptsLogger.warn("failed to load user scripts:", error),
    onPersistError: (error) => scriptsLogger.warn("failed to persist user scripts:", error),
    afterLoad: () => Promise.all([loadAuthorProfile(), loadUpdateInterval()]),
    // 仅首次加载后静默检查一轮，不阻塞界面；此后改间隔只重排定时器。
    onLoaded: () => scheduleUpdateChecks({ immediate: true }),
  });
  const scripts = scriptsPersistence.items;
  const loaded = scriptsPersistence.loaded;
  const authorProfile = ref({ author: "", homepage: "" });
  const updateIntervalHours = ref(DEFAULT_UPDATE_INTERVAL_HOURS);
  const updateChecking = ref(false);
  let updateTimer = null;
  let updateCheckPromise = null;

  async function loadAuthorProfile() {
    try {
      const raw = await getSetting(AUTHOR_PROFILE_SETTING_KEY);
      if (raw) {
        const parsed = JSON.parse(raw);
        authorProfile.value = {
          author: String(parsed?.author || ""),
          homepage: String(parsed?.homepage || ""),
        };
      }
    } catch (error) {
      scriptsLogger.warn("failed to load script author profile:", error);
    }
  }

  function saveAuthorProfile(profile) {
    authorProfile.value = {
      author: String(profile?.author || ""),
      homepage: String(profile?.homepage || ""),
    };
    return setPreference(AUTHOR_PROFILE_SETTING_KEY, JSON.stringify(authorProfile.value)).catch(
      (error) => scriptsLogger.warn("failed to save script author profile:", error),
    );
  }

  async function loadUpdateInterval() {
    try {
      const raw = await getSetting(UPDATE_INTERVAL_SETTING_KEY);
      if (raw !== null && raw !== undefined && raw !== "") {
        const hours = Number(raw);
        if (Number.isFinite(hours) && hours >= 0) updateIntervalHours.value = hours;
      }
    } catch (error) {
      scriptsLogger.warn("failed to load script update interval:", error);
    }
  }

  function setUpdateInterval(hours) {
    const next = Number(hours);
    updateIntervalHours.value = Number.isFinite(next) && next >= 0 ? next : 0;
    scheduleUpdateChecks();
    return setPreference(UPDATE_INTERVAL_SETTING_KEY, String(updateIntervalHours.value)).catch(
      (error) => scriptsLogger.warn("failed to save script update interval:", error),
    );
  }

  function scheduleUpdateChecks({ immediate = false } = {}) {
    if (updateTimer) {
      clearInterval(updateTimer);
      updateTimer = null;
    }
    if (updateIntervalHours.value <= 0) return;
    updateTimer = setInterval(
      () => void checkAllUpdates(),
      updateIntervalHours.value * 3600 * 1000,
    );
    if (immediate) void checkAllUpdates();
  }

  onScopeDispose(() => {
    if (updateTimer) {
      clearInterval(updateTimer);
      updateTimer = null;
    }
    scriptsPersistence.flushPendingPersist();
  });

  function replaceScript(id, updater) {
    const index = scripts.value.findIndex((script) => script.id === id);
    if (index < 0) return null;
    const current = scripts.value[index];
    const next = normalizeScript({
      ...updater(current),
      id,
      updatedAt: Date.now(),
    });
    if (!next) return null;
    scripts.value = scripts.value.map((script, i) => (i === index ? next : script));
    scriptsPersistence.schedulePersist();
    return next;
  }

  function patchScriptState(id, patch) {
    const index = scripts.value.findIndex((script) => script.id === id);
    if (index < 0) return null;
    const next = { ...scripts.value[index], ...patch };
    scripts.value = scripts.value.map((script, i) => (i === index ? next : script));
    scriptsPersistence.schedulePersist();
    return next;
  }

  // 新建脚本：按作者信息生成 ==XTermScript== 头 + 模板正文。
  function createScript(metadata = {}) {
    const script = normalizeScript({
      id: createRuntimeId(),
      code: `${buildScriptHeader(metadata)}\n\n${DEFAULT_SCRIPT_BODY}`,
      createdAt: Date.now(),
    });
    scripts.value = [script, ...scripts.value];
    scriptsPersistence.schedulePersist();
    return script;
  }

  // 导入脚本：元数据从头块解析，缺省时回退到文件名。
  function importScript(fileName, code) {
    const header = parseScriptMetadata(code);
    const script = normalizeScript({
      id: createRuntimeId(),
      code,
      name: header.name || String(fileName || "").replace(/\.js$/i, ""),
      createdAt: Date.now(),
    });
    scripts.value = [script, ...scripts.value];
    scriptsPersistence.schedulePersist();
    return script;
  }

  function updateScript(id, patch) {
    return replaceScript(id, (script) => ({ ...script, ...patch }));
  }

  function removeScript(id) {
    scripts.value = scripts.value.filter((script) => script.id !== id);
    scriptsPersistence.schedulePersist();
  }

  function getScript(id) {
    return scripts.value.find((script) => script.id === id) || null;
  }

  // 油猴式更新检测：拉 @updateURL 的内容，比较 @version。
  async function checkScriptUpdate(script) {
    if (!script?.updateUrl) return { status: "no-url" };
    try {
      const remoteCode = await fetchScriptText(script.updateUrl);
      const remote = parseScriptMetadata(remoteCode);
      if (!remote.version) return { status: "no-version" };
      const current = getScript(script.id);
      if (!current || current.updateUrl !== script.updateUrl) return { status: "stale" };
      if (compareScriptVersions(remote.version, current.version) > 0) {
        patchScriptState(current.id, { updateAvailableVersion: remote.version });
        return { status: "available", version: remote.version };
      }
      if (current.updateAvailableVersion) {
        patchScriptState(current.id, { updateAvailableVersion: "" });
      }
      return { status: "latest" };
    } catch (error) {
      scriptsLogger.warn("script update check failed:", error);
      return { status: "error", error: String(error) };
    }
  }

  function checkAllUpdates() {
    if (updateCheckPromise) return updateCheckPromise;
    updateChecking.value = true;
    updateCheckPromise = (async () => {
      const targets = scripts.value.filter((script) => script.updateUrl);
      const results = await Promise.all(targets.map((script) => checkScriptUpdate(script)));
      return {
        checked: targets.length,
        available: results.filter((result) => result.status === "available").length,
        errors: results.filter((result) => result.status === "error").length,
      };
    })().finally(() => {
      updateChecking.value = false;
      updateCheckPromise = null;
    });
    return updateCheckPromise;
  }

  // 应用更新：重新拉取远程代码并整体替换（createdAt 保留，头块版本随代码更新）。
  async function applyScriptUpdate(id) {
    const script = getScript(id);
    if (!script?.updateUrl) throw new Error("script has no update url");
    const updateUrl = script.updateUrl;
    const remoteCode = await fetchScriptText(updateUrl);
    const current = getScript(id);
    if (!current || current.updateUrl !== updateUrl) {
      throw new Error("script changed while the update was downloading");
    }
    return replaceScript(id, (current) => ({
      ...current,
      code: remoteCode,
      updateAvailableVersion: "",
    }));
  }

  return {
    applyScriptUpdate,
    authorProfile,
    checkAllUpdates,
    createScript,
    getScript,
    importScript,
    loadScripts: scriptsPersistence.load,
    loaded,
    persistNow: scriptsPersistence.persistNow,
    removeScript,
    saveAuthorProfile,
    scripts,
    setUpdateInterval,
    updateChecking,
    updateIntervalHours,
    updateScript,
  };
});
