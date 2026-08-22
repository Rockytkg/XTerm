import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { storeToRefs } from "pinia";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useToasts } from "./useToasts";
import { invokeIpc } from "../services/ipc/core";
import { normalizeHighlightMatchType } from "../utils/terminalPanelHelpers";
import { createRuntimeId } from "../utils/runtimeIds";
import { TERMINAL_THEME_NAMES } from "../utils/terminalColors";

const DEFAULT_RULE_COLOR = "#fbbf24";

export function useHighlightSchemes() {
  const { t } = useI18n();
  const { preferences } = storeToRefs(useWorkspaceStore());
  const { showToast } = useToasts();

  const schemes = computed({
    get: () =>
      Array.isArray(preferences.value.terminalHighlightSchemes)
        ? preferences.value.terminalHighlightSchemes
        : [],
    set: (value) => {
      preferences.value.terminalHighlightSchemes = value;
    },
  });

  const terminalThemeOptions = computed(() =>
    TERMINAL_THEME_NAMES.map((value) => ({
      label: t(`settings.terminal.themes.${value}`),
      value,
    })),
  );

  function updateSchemes(mutator) {
    const next = schemes.value.map((scheme) => ({
      ...scheme,
      themes: Array.isArray(scheme.themes) ? [...scheme.themes] : [],
      rules: Array.isArray(scheme.rules) ? scheme.rules.map((rule) => ({ ...rule })) : [],
    }));
    mutator(next);
    schemes.value = next;
  }

  function createUniqueSchemeName() {
    const baseName = t("settings.terminal.highlightNewScheme");
    const usedNames = new Set(
      schemes.value.map((scheme) => String(scheme.name || "").trim()).filter(Boolean),
    );
    if (!usedNames.has(baseName)) return baseName;
    let index = 2;
    while (usedNames.has(`${baseName}(${index})`)) {
      index += 1;
    }
    return `${baseName}(${index})`;
  }

  function addScheme({ name, themes } = {}) {
    const id = createRuntimeId();
    const finalName = String(name || "").trim() || createUniqueSchemeName();
    const wanted = Array.isArray(themes) ? [...new Set(themes)] : [];
    updateSchemes((list) => {
      for (const scheme of list) {
        scheme.themes = scheme.themes.filter((theme) => !wanted.includes(theme));
      }
      list.push({ id, name: finalName, themes: wanted, rules: [] });
    });
    showToast({
      type: "success",
      title: t("notifications.highlightSchemeCreated", { name: finalName }),
    });
    return id;
  }

  async function importSchemes() {
    try {
      const imported = await invokeIpc("terminal_highlight_schemes_import");
      if (!imported) return;
      schemes.value = imported;
      showToast({ type: "success", title: t("notifications.highlightSchemesImported") });
    } catch (error) {
      showToast({
        type: "error",
        title: t("notifications.highlightSchemesImportFailed"),
        message: String(error),
      });
    }
  }

  async function exportScheme(schemeId) {
    try {
      const savedPath = await invokeIpc("terminal_highlight_schemes_export", { schemeId });
      if (!savedPath) return;
      showToast({ type: "success", title: t("notifications.highlightSchemesExported") });
    } catch (error) {
      showToast({
        type: "error",
        title: t("notifications.highlightSchemesExportFailed"),
        message: String(error),
      });
    }
  }

  function removeScheme(id) {
    let removedName = "";
    updateSchemes((list) => {
      const index = list.findIndex((scheme) => scheme.id === id);
      if (index < 0) return;
      removedName = list[index].name || t("settings.terminal.highlightUntitled");
      list.splice(index, 1);
    });
    showToast({
      type: removedName ? "success" : "error",
      title: removedName
        ? t("notifications.highlightSchemeDeleted", { name: removedName })
        : t("notifications.highlightSchemeDeleteFailed"),
    });
  }

  function updateScheme(id, patch) {
    updateSchemes((list) => {
      const scheme = list.find((item) => item.id === id);
      if (scheme) Object.assign(scheme, patch);
    });
  }

  function setSchemeThemes(id, themes) {
    const wanted = Array.isArray(themes) ? [...new Set(themes)] : [];
    updateSchemes((list) => {
      for (const scheme of list) {
        scheme.themes =
          scheme.id === id ? [...wanted] : scheme.themes.filter((theme) => !wanted.includes(theme));
      }
    });
  }

  function addRule(id) {
    updateSchemes((list) => {
      const scheme = list.find((item) => item.id === id);
      if (!scheme) return;
      scheme.rules.push({
        matchType: "text",
        pattern: "",
        caseSensitive: false,
        effect: "foreground",
        color: DEFAULT_RULE_COLOR,
      });
    });
  }

  function removeRule(id, ruleIndex) {
    let removed = false;
    updateSchemes((list) => {
      const scheme = list.find((item) => item.id === id);
      if (!scheme) return;
      if (ruleIndex < 0 || ruleIndex >= scheme.rules.length) return;
      scheme.rules.splice(ruleIndex, 1);
      removed = true;
    });
    showToast({
      type: removed ? "success" : "error",
      title: t(
        removed ? "notifications.highlightRuleDeleted" : "notifications.highlightRuleDeleteFailed",
      ),
    });
  }

  function updateRule(id, ruleIndex, patch) {
    updateSchemes((list) => {
      const rule = list.find((scheme) => scheme.id === id)?.rules[ruleIndex];
      if (rule) Object.assign(rule, patch);
    });
  }

  function setRuleEffect(id, ruleIndex, effect) {
    updateRule(id, ruleIndex, { effect: effect === "background" ? "background" : "foreground" });
  }

  function setRuleMatchType(id, ruleIndex, matchType) {
    updateRule(id, ruleIndex, { matchType: matchType === "regex" ? "regex" : "text" });
  }

  function normalizeHexInput(value) {
    const color = String(value || "").trim();
    return /^#[0-9a-fA-F]{6}$/.test(color) ? color.toLowerCase() : "";
  }

  function setRuleColor(id, ruleIndex, value) {
    const color = normalizeHexInput(value);
    if (!color) return;
    updateRule(id, ruleIndex, { color });
  }

  function normalizeHighlightEffect(rule) {
    return rule?.effect === "background" ? "background" : "foreground";
  }

  function getRuleColor(rule) {
    return rule.color || DEFAULT_RULE_COLOR;
  }

  function getPatternPlaceholder(rule) {
    return t(
      normalizeHighlightMatchType(rule?.matchType) === "text"
        ? "settings.terminal.highlightTextPatternPlaceholder"
        : "settings.terminal.highlightRegexPatternPlaceholder",
    );
  }

  function getContrastText(hex) {
    const value = normalizeHexInput(hex);
    if (!value) return "#ffffff";
    const r = parseInt(value.slice(1, 3), 16);
    const g = parseInt(value.slice(3, 5), 16);
    const b = parseInt(value.slice(5, 7), 16);
    const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
    return luminance > 0.6 ? "#1f2430" : "#ffffff";
  }

  function getRulePreviewText(rule) {
    const pattern = String(rule?.pattern || "").trim();
    return pattern || t("settings.terminal.highlightPreviewSample");
  }

  function getRulePreviewStyle(rule) {
    const color = getRuleColor(rule);
    if (normalizeHighlightEffect(rule) === "background") {
      return { backgroundColor: color, color: getContrastText(color), borderColor: color };
    }
    return { color };
  }

  return {
    schemes,
    terminalThemeOptions,
    addScheme,
    importSchemes,
    exportScheme,
    removeScheme,
    updateScheme,
    setSchemeThemes,
    addRule,
    removeRule,
    updateRule,
    setRuleEffect,
    setRuleMatchType,
    setRuleColor,
    normalizeHighlightEffect,
    getRuleColor,
    getPatternPlaceholder,
    getRulePreviewText,
    getRulePreviewStyle,
  };
}
