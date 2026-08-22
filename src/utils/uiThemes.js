/**
 * UI theme presets ("skins") applied on top of the base light/dark tokens.
 *
 * A preset is selected independently for light and dark mode; the resolved
 * preset for the current effective mode is exposed to CSS as the
 * `data-ui-theme` attribute on <html> (`"<preset>-<mode>"`), so a preset can
 * ship distinct light and dark palettes.
 */

export const UI_THEME_PRESETS = ["default", "solarized", "github"];

export function normalizeUiThemePreset(value) {
  return UI_THEME_PRESETS.includes(value) ? value : "default";
}

export function resolveUiThemeAttribute(preset, mode) {
  const normalized = normalizeUiThemePreset(preset);
  if (normalized === "default") return "";
  return `${normalized}-${mode === "dark" ? "dark" : "light"}`;
}

/**
 * Apply the resolved UI theme to the document. `mode` is the effective
 * light/dark mode (already resolved from the "auto" preference), so this must
 * be re-run whenever the effective mode changes.
 */
export function applyUiTheme(mode, options = {}) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const preset = normalizeUiThemePreset(mode === "dark" ? options.presetDark : options.presetLight);
  const attribute = resolveUiThemeAttribute(preset, mode);
  if (attribute) {
    root.dataset.uiTheme = attribute;
  } else {
    delete root.dataset.uiTheme;
  }
}
