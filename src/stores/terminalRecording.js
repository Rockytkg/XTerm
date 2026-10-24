import { getTerminalCodePointWidth } from "../utils/terminal/unicodeWidth.js";

const RECORDING_CLEAR_LINE = "\uE000";
const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);
const RE_CSI = new RegExp(`${ESC}\\[[0-?]*[ -/]*[@-~]`, "g");
const RE_OSC = new RegExp(`${ESC}\\][^${BEL}]*(?:${BEL}|${ESC}\\\\)`, "g");
const RE_ESC = new RegExp(`${ESC}[@-Z\\\\-_]`, "g");
const RE_CLEAR_LINE = new RegExp(`${ESC}\\[[0-2]?K`, "g");

// 无换行的超长单行输出会让 lineCells 无界增长，超过上限的单元直接丢弃，等换行/reset 清空。
const MAX_LINE_CELLS = 8192;

export function createTerminalRecordingNormalizer() {
  let lineCells = [];
  let cellWidths = [];
  let cursor = 0;

  function reset() {
    lineCells = [];
    cellWidths = [];
    cursor = 0;
  }

  function stripControlSequences(data) {
    return String(data || "")
      .replace(RE_CLEAR_LINE, RECORDING_CLEAR_LINE)
      .replace(RE_CSI, "")
      .replace(RE_OSC, "")
      .replace(RE_ESC, "");
  }

  function writeSpace(column) {
    lineCells[column] = " ";
    cellWidths[column] = 1;
  }

  function clearWideCell(column) {
    let start = column;
    if (lineCells[column] === "" && cellWidths[column] === 0) start -= 1;
    if (cellWidths[start] !== 2) return;
    writeSpace(start);
    writeSpace(start + 1);
  }

  function writeSizedCell(character, width) {
    if (cursor + width > MAX_LINE_CELLS) return;
    clearWideCell(cursor);
    if (width === 2) clearWideCell(cursor + 1);
    while (lineCells.length < cursor) {
      writeSpace(lineCells.length);
    }
    lineCells[cursor] = character;
    cellWidths[cursor] = width;
    cursor += 1;
    if (width === 2) {
      lineCells[cursor] = "";
      cellWidths[cursor] = 0;
      cursor += 1;
    }
  }

  function writeCharacter(character) {
    const width = getTerminalCodePointWidth(character.codePointAt(0));
    if (width > 0) {
      writeSizedCell(character, width);
      return;
    }

    let previous = cursor - 1;
    if (previous >= 0 && lineCells[previous] === "" && cellWidths[previous] === 0) {
      previous -= 1;
    }
    const previousIsLeadingMark =
      previous >= 0 && lineCells[previous] !== "" && cellWidths[previous] === 0;
    if (previous >= 0 && !previousIsLeadingMark) {
      lineCells[previous] = `${lineCells[previous] || " "}${character}`;
      return;
    }
    writeSizedCell(character, 1);
    cellWidths[cursor - 1] = 0;
  }

  function commitLine(lines) {
    lines.push(`${lineCells.join("").trimEnd()}\n`);
    reset();
  }

  function normalize(data, { flush = false } = {}) {
    const lines = [];
    for (const character of stripControlSequences(data)) {
      if (character === "\r") {
        cursor = 0;
      } else if (character === "\n") {
        commitLine(lines);
      } else if (character === "\b") {
        cursor = Math.max(0, cursor - 1);
      } else if (character === RECORDING_CLEAR_LINE) {
        reset();
      } else if (character === "\t") {
        const spaces = 8 - (cursor % 8);
        for (let index = 0; index < spaces; index += 1) {
          writeSizedCell(" ", 1);
        }
      } else if (character >= " " && character !== "\u007F") {
        writeCharacter(character);
      }
    }

    if (flush && lineCells.length > 0) {
      commitLine(lines);
    }
    return lines.join("");
  }

  return { normalize, reset };
}
