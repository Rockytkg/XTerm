export const TERMINAL_FRAME_TYPES = Object.freeze({
  ATTACH: "terminal.attach",
  DETACH: "terminal.detach",
  INPUT_TEXT: "terminal.input.text",
  INPUT_BYTES: "terminal.input.bytes",
  OUTPUT: "terminal.output",
  RAW_OUTPUT: "terminal.raw_output",
  RESIZE: "terminal.resize",
});

export function createAttachFrame(sessionId, onOutput = null) {
  return { type: TERMINAL_FRAME_TYPES.ATTACH, sessionId, onOutput };
}

export function createDetachFrame(sessionId, channelId = null) {
  return { type: TERMINAL_FRAME_TYPES.DETACH, sessionId, channelId };
}

export function createInputTextFrame({ sessionId, channelId, inputSequence, data }) {
  return { type: TERMINAL_FRAME_TYPES.INPUT_TEXT, sessionId, channelId, inputSequence, data };
}

export function createInputBytesFrame({ sessionId, channelId, inputSequence, dataBase64 }) {
  return {
    type: TERMINAL_FRAME_TYPES.INPUT_BYTES,
    sessionId,
    channelId,
    inputSequence,
    dataBase64,
  };
}

export function createResizeFrame({ sessionId, channelId, cols, rows, widthPx, heightPx }) {
  return {
    type: TERMINAL_FRAME_TYPES.RESIZE,
    sessionId,
    channelId,
    cols,
    rows,
    widthPx,
    heightPx,
  };
}

export function createRawOutputFrame({ sessionId, channelId, enabled }) {
  return { type: TERMINAL_FRAME_TYPES.RAW_OUTPUT, sessionId, channelId, enabled };
}

export function normalizeOutputFrame(payload) {
  return { type: TERMINAL_FRAME_TYPES.OUTPUT, payload };
}
