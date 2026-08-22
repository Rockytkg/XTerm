import { createI18n } from "vue-i18n";
import zhCN from "./locales/zh-CN.js";
import enUS from "./locales/en-US.js";

const DEFAULT_LOCALE = "zh-CN";

const languageConfigs = [
  { label: "简体中文", value: "zh-CN", messages: zhCN },
  { label: "English", value: "en-US", messages: enUS },
];

export const languageOptions = languageConfigs.map(({ label, value }) => ({ label, value }));

export const i18n = createI18n({
  legacy: false,
  locale: DEFAULT_LOCALE,
  fallbackLocale: "en-US",
  messages: Object.fromEntries(languageConfigs.map(({ value, messages }) => [value, messages])),
});

export function resolveLocale(locale) {
  if (typeof locale !== "string") return DEFAULT_LOCALE;
  const normalizedLocale = locale.trim().replace(/^["']|["']$/g, "");
  return languageConfigs.some((config) => config.value === normalizedLocale)
    ? normalizedLocale
    : DEFAULT_LOCALE;
}

function setGlobalLocale(locale) {
  const globalLocale = i18n.global.locale;
  if (globalLocale && typeof globalLocale === "object" && "value" in globalLocale) {
    globalLocale.value = locale;
  } else {
    i18n.global.locale = locale;
  }
}

export function currentLocale() {
  const globalLocale = i18n.global.locale;
  return resolveLocale(
    globalLocale && typeof globalLocale === "object" && "value" in globalLocale
      ? globalLocale.value
      : globalLocale,
  );
}

/**
 * Resolves a locale and returns it via a microtask so callers can await it
 * uniformly. Messages are statically bundled, so no actual network preload
 * is necessary.
 */
export function loadLocaleMessages(locale) {
  return Promise.resolve(resolveLocale(locale));
}

export function commitLocale(locale) {
  const resolvedLocale = resolveLocale(locale);
  setGlobalLocale(resolvedLocale);
  document.documentElement.lang = resolvedLocale;
  return resolvedLocale;
}
