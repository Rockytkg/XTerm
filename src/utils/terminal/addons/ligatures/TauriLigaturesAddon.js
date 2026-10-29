import { resolveTerminalLigatures } from "../../../../services/terminalLigatures.js";
import { DEFAULT_LIGATURES, LigatureMatcher } from "./ligatureMatcher.js";

export class TauriLigaturesAddon {
  constructor({
    fallbackLigatures = DEFAULT_LIGATURES,
    fontFeatureSettings = '"calt" on',
    loadLigatures = resolveTerminalLigatures,
  } = {}) {
    this._fallbackLigatures = [...fallbackLigatures];
    this._fontFeatureSettings = fontFeatureSettings;
    this._loadLigatures = loadLigatures;
    this._matcher = new LigatureMatcher(this._fallbackLigatures);
    this._fontFamily = null;
    this._fontGeneration = 0;
  }

  activate(terminal) {
    if (!terminal.element) {
      throw new Error("Cannot activate TauriLigaturesAddon before open is called");
    }
    this._terminal = terminal;
    this._element = terminal.element;
    this._previousFontFeatureSettings = this._element.style.fontFeatureSettings;
    this._element.style.fontFeatureSettings = this._fontFeatureSettings;
    this._characterJoinerId = terminal.registerCharacterJoiner((text) => {
      const fontFamily = terminal.options.fontFamily || "";
      if (fontFamily !== this._fontFamily) this._loadFont(fontFamily);
      return this._matcher.findRanges(text);
    });
    this._loadFont(terminal.options.fontFamily || "");
  }

  dispose() {
    this._fontGeneration += 1;
    if (this._characterJoinerId !== undefined) {
      this._terminal?.deregisterCharacterJoiner(this._characterJoinerId);
      this._characterJoinerId = undefined;
    }
    if (this._element) {
      this._element.style.fontFeatureSettings = this._previousFontFeatureSettings;
    }
    this._terminal = null;
    this._element = null;
  }

  _loadFont(fontFamily) {
    this._fontFamily = fontFamily;
    this._matcher = new LigatureMatcher(this._fallbackLigatures);
    const generation = ++this._fontGeneration;
    if (!fontFamily) return;

    Promise.resolve()
      .then(() =>
        this._loadLigatures({
          fontFamily,
          candidates: this._fallbackLigatures,
        }),
      )
      .then((ligatures) => {
        if (
          generation !== this._fontGeneration ||
          fontFamily !== this._terminal?.options.fontFamily ||
          !Array.isArray(ligatures)
        ) {
          return;
        }
        this._matcher = new LigatureMatcher(ligatures);
        this._terminal.refresh(0, this._terminal.rows - 1);
      })
      .catch(() => {});
  }
}
