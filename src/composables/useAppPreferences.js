import { computed, nextTick, reactive, ref, watch } from "vue";
import { commitLocale, loadLocaleMessages, resolveLocale } from "../i18n";
import { getPreferences, resetPreferencesStore, setPreference } from "../services/preferences";
import { createLogger } from "../utils/logger";
import { noop } from "../utils/noop";
import { motionEnabled, runViewTransition, setMotionPreferenceEnabled } from "../utils/motion";
import { applyUiTheme } from "../utils/uiThemes";

const logger = createLogger("frontend.preferences");

const PREFERENCES_SAVE_DEBOUNCE_MS = 300;
const THEME_TRANSITION_CLASS = "theme-transition-running";

const preferences = reactive({});
let preferencesLoaded = false;
let preferencesSaveTimer = 0;
let persistedPreferenceSnapshot = {};
let themeTransitionActive = false;
let localeApplySequence = 0;
let initialPreferencesReady = null;
let preferenceWatchersPaused = false;
let systemThemeQuery = null;
let stopSystemThemeListener = null;

const systemPrefersDark = ref(false);

function serializePreference(value) {
  if (value !== null && typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function replacePreferenceState(next) {
  for (const key of Object.keys(preferences)) {
    if (!(key in next)) delete preferences[key];
  }
  Object.assign(preferences, next);
}

/**
 * Full serialization of every registered preference key — used to seed the
 * persisted snapshot at startup / reset.
 */
function serializedPreferenceSnapshot(source) {
  const snapshot = {};
  for (const [key, value] of Object.entries(source)) {
    snapshot[key] = serializePreference(value);
  }
  return snapshot;
}

/**
 * Compute which preferences have changed vs the last persisted snapshot,
 * and return only the changed entries.
 */
function computePreferenceChanges(source) {
  const changed = {};
  for (const [key, value] of Object.entries(source)) {
    const serialized = serializePreference(value);
    if (persistedPreferenceSnapshot[key] !== serialized) {
      changed[key] = serialized;
    }
  }
  return changed;
}

function resolveTheme(theme) {
  if (theme === "dark") return "dark";
  if (theme === "auto") return "auto";
  return "light";
}

function updateSystemThemePreference(event = systemThemeQuery) {
  systemPrefersDark.value = !!event?.matches;
}

function startSystemThemeListener() {
  if (systemThemeQuery || typeof window === "undefined" || !window.matchMedia) return;
  systemThemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
  updateSystemThemePreference(systemThemeQuery);
  systemThemeQuery.addEventListener("change", updateSystemThemePreference);
  stopSystemThemeListener = () => {
    systemThemeQuery?.removeEventListener("change", updateSystemThemePreference);
    systemThemeQuery = null;
    stopSystemThemeListener = null;
  };
}

function resolveEffectiveTheme(theme, prefersDark) {
  const resolvedTheme = resolveTheme(theme);
  if (resolvedTheme === "auto") return prefersDark ? "dark" : "light";
  return resolvedTheme;
}

function applyTheme(theme) {
  const resolvedTheme = resolveTheme(theme);
  if (resolvedTheme === "auto") {
    delete document.documentElement.dataset.theme;
    document.documentElement.style.colorScheme = "light dark";
    return;
  }
  document.documentElement.dataset.theme = resolvedTheme;
  document.documentElement.style.colorScheme = resolvedTheme;
}

async function applyLocale(locale) {
  const resolvedLocale = resolveLocale(locale);
  const sequence = ++localeApplySequence;
  await loadLocaleMessages(resolvedLocale);
  if (sequence === localeApplySequence) {
    commitLocale(resolvedLocale);
    if (preferences.locale !== resolvedLocale) preferences.locale = resolvedLocale;
  }
}

function applyUiFontSize(value) {
  const fontSize = Number(value);
  if (!Number.isFinite(fontSize)) {
    document.documentElement.style.removeProperty("--ui-font-size");
    return;
  }
  const normalized = Math.min(18, Math.max(12, fontSize));
  document.documentElement.style.setProperty("--ui-font-size", `${normalized}px`);
}

function applyUiThemePreference(source) {
  const mode = resolveEffectiveTheme(source.theme, systemPrefersDark.value);
  applyUiTheme(mode, {
    presetLight: source.uiThemeLight,
    presetDark: source.uiThemeDark,
  });
}

async function applyPreferenceSideEffects(source) {
  setMotionPreferenceEnabled(source.enableAnimations);
  applyTheme(source.theme);
  applyUiThemePreference(source);
  applyUiFontSize(source.uiFontSize);
  await applyLocale(source.locale).catch((error) => {
    logger.error("locale.apply.failed", error);
  });
}

function getThemeTransitionMetrics(event) {
  const root = document.documentElement;
  const source = event?.currentTarget ?? event?.target;
  const rect =
    source && typeof source.getBoundingClientRect === "function"
      ? source.getBoundingClientRect()
      : null;
  const viewportInline = window.innerWidth || root.clientWidth;
  const viewportBlock = window.innerHeight || root.clientHeight;
  const x = rect ? rect.left + rect.width / 2 : viewportInline;
  const y = rect ? rect.top + rect.height / 2 : 0;
  const radius = Math.ceil(
    Math.hypot(Math.max(x, viewportInline - x), Math.max(y, viewportBlock - y)),
  );

  return { radius, x, y };
}

function prepareThemeTransition(event) {
  const root = document.documentElement;
  const { radius, x, y } = getThemeTransitionMetrics(event);

  root.style.setProperty("--theme-transition-x", `${x}px`);
  root.style.setProperty("--theme-transition-y", `${y}px`);
  root.style.setProperty("--theme-transition-radius", `${radius}px`);
}

function finishThemeTransition() {
  const root = document.documentElement;

  root.classList.remove(THEME_TRANSITION_CLASS);
  root.style.removeProperty("--theme-transition-x");
  root.style.removeProperty("--theme-transition-y");
  root.style.removeProperty("--theme-transition-radius");
  themeTransitionActive = false;
}

async function hydratePreferences() {
  logger.info("hydrate.start");
  let saved;
  try {
    saved = await getPreferences();
  } catch (error) {
    logger.error("hydrate.failed", error);
    throw error;
  }
  delete saved.terminalInitialCols;
  delete saved.terminalInitialRows;
  delete saved.terminalFontScale;
  const normalized = { ...(saved || {}) };
  preferencesLoaded = false;
  preferenceWatchersPaused = true;
  try {
    replacePreferenceState(normalized);
    await applyPreferenceSideEffects(normalized);
    persistedPreferenceSnapshot = serializedPreferenceSnapshot(normalized);
    preferencesLoaded = true;
  } finally {
    preferenceWatchersPaused = false;
  }
  logger.info("hydrate.success", {
    theme: preferences.theme,
    locale: preferences.locale,
  });
}

export function initializePreferences() {
  if (!initialPreferencesReady) {
    initialPreferencesReady = hydratePreferences().catch((error) => {
      initialPreferencesReady = null;
      throw error;
    });
  }
  return initialPreferencesReady;
}

function schedulePreferenceSave(value) {
  window.clearTimeout(preferencesSaveTimer);
  preferencesSaveTimer = window.setTimeout(() => {
    // diff 计算（全量 JSON.stringify）放在去抖回调内：高频 watch 触发时只做计时器重置。
    const snapshot = computePreferenceChanges(value);
    if (Object.keys(snapshot).length === 0) {
      preferencesSaveTimer = 0;
      return;
    }
    logger.debug("persist.start", { keys: Object.keys(snapshot) });
    for (const [key, value] of Object.entries(snapshot)) {
      setPreference(key, value)
        .then(() => {
          persistedPreferenceSnapshot[key] = value;
        })
        .catch((error) => {
          logger.error("persist.failed", {
            key,
            error,
          });
        });
    }
  }, PREFERENCES_SAVE_DEBOUNCE_MS);
}

const stopPreferencePersistenceWatch = watch(
  () => ({ ...preferences }),
  (value) => {
    if (preferencesLoaded) {
      schedulePreferenceSave(value);
    }
  },
  { flush: "post" },
);

const stopLocaleWatch = watch(
  () => preferences.locale,
  (locale) => {
    if (preferenceWatchersPaused || !preferencesLoaded) return;
    applyLocale(locale).catch((error) => {
      logger.error("locale.apply.failed", error);
    });
  },
);

const stopThemeWatch = watch(
  () => preferences.theme,
  (theme) => {
    if (!preferenceWatchersPaused && preferencesLoaded) applyTheme(theme);
  },
);

const stopAnimationWatch = watch(
  () => preferences.enableAnimations,
  (enabled) => {
    if (!preferenceWatchersPaused && preferencesLoaded) setMotionPreferenceEnabled(enabled);
  },
);

const stopUiFontSizeWatch = watch(
  () => preferences.uiFontSize,
  (value) => {
    if (!preferenceWatchersPaused && preferencesLoaded) applyUiFontSize(value);
  },
);

// The resolved `data-ui-theme` attribute depends on the effective light/dark
// mode, so it must also re-apply when the theme preference or the OS-level
// color scheme (used by "auto") changes.
const stopUiThemeWatch = watch(
  [
    () => preferences.uiThemeLight,
    () => preferences.uiThemeDark,
    () => preferences.theme,
    systemPrefersDark,
  ],
  () => {
    if (!preferenceWatchersPaused && preferencesLoaded) applyUiThemePreference(preferences);
  },
);

// `terminalHighlightSchemes` is the only deeply-nested preference — watch it
// individually with `deep: true` to catch per-rule edits.
const stopHighlightSchemesWatch = watch(
  () => preferences.terminalHighlightSchemes,
  () => {
    if (preferencesLoaded) schedulePreferenceSave(preferences);
  },
  { deep: true },
);

export function useAppPreferences() {
  initializePreferences().catch(noop);
  startSystemThemeListener();

  const resolvedTheme = computed(() =>
    resolveEffectiveTheme(preferences.theme, systemPrefersDark.value),
  );
  const isDark = computed(() => resolvedTheme.value === "dark");

  function toggleTheme(event) {
    const cycle = { light: "dark", dark: "auto", auto: "light" };
    const next = cycle[preferences.theme] ?? "light";
    logger.info("theme.toggle", {
      current: preferences.theme,
      next,
    });

    if (themeTransitionActive || !motionEnabled()) {
      preferences.theme = next;
      applyTheme(next);
      return;
    }

    themeTransitionActive = true;
    prepareThemeTransition(event);
    runViewTransition(
      async () => {
        preferences.theme = next;
        applyTheme(next);
        await nextTick();
      },
      { className: THEME_TRANSITION_CLASS },
    )
      .finally(finishThemeTransition)
      .catch(() => {});
  }

  function resetPreferences() {
    logger.warn("reset.start");
    window.clearTimeout(preferencesSaveTimer);
    preferencesSaveTimer = 0;
    return resetPreferencesStore()
      .then(async (defaults) => {
        const normalized = { ...(defaults || {}) };
        preferencesLoaded = false;
        preferenceWatchersPaused = true;
        try {
          replacePreferenceState(normalized);
          await applyPreferenceSideEffects(normalized);
          persistedPreferenceSnapshot = serializedPreferenceSnapshot(normalized);
          preferencesLoaded = true;
        } finally {
          preferenceWatchersPaused = false;
        }
      })
      .catch((error) => {
        logger.error("reset.failed", error);
        throw error;
      });
  }

  return {
    isDark,
    preferences,
    resolvedTheme,
    resetPreferences,
    toggleTheme,
  };
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    stopPreferencePersistenceWatch();
    stopLocaleWatch();
    stopThemeWatch();
    stopAnimationWatch();
    stopUiFontSizeWatch();
    stopUiThemeWatch();
    stopHighlightSchemesWatch();
    stopSystemThemeListener?.();
    window.clearTimeout(preferencesSaveTimer);
    preferencesSaveTimer = 0;
  });
}
