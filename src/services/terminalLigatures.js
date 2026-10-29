import { createLogger } from "../utils/logger.js";
import { invokeIpc } from "./ipc/core.js";

const logger = createLogger("frontend.terminal.ligatures");
const ligatureCache = new Map();

export function parseFontFamilies(fontFamily) {
  const families = [];
  let current = "";
  let quote = "";
  let escaped = false;

  for (const character of String(fontFamily || "")) {
    if (escaped) {
      current += character;
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (quote) {
      if (character === quote) quote = "";
      else current += character;
    } else if (character === '"' || character === "'") {
      quote = character;
    } else if (character === ",") {
      addFamily(families, current);
      current = "";
    } else {
      current += character;
    }
  }
  if (escaped) current += "\\";
  addFamily(families, current);
  return families;
}

function addFamily(families, family) {
  const normalized = family.trim().replace(/\s+/gu, " ");
  if (normalized) families.push(normalized);
}

export async function resolveTerminalLigatures({ fontFamily, candidates }) {
  const families = parseFontFamilies(fontFamily);
  if (!families.length || !candidates?.length) return null;

  const key = JSON.stringify([families, candidates]);
  if (ligatureCache.has(key)) return ligatureCache.get(key);

  const request = invokeIpc("font_ligatures_resolve", {
    request: { families, candidates },
  });
  ligatureCache.set(key, request);
  try {
    return await request;
  } catch (error) {
    ligatureCache.delete(key);
    logger.warn("resolve.failed", error);
    return null;
  }
}
