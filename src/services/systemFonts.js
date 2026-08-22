import { createLogger } from "../utils/logger";
import { invokeIpc } from "./ipc/core";

const logger = createLogger("frontend.system_fonts.service");

const DEFAULT_FONT_CHUNK_SIZE = 10;

function normalizeFonts(fonts) {
  return Array.isArray(fonts) ? fonts.filter(Boolean) : [];
}

export async function* streamSystemFonts({
  chunkSize = DEFAULT_FONT_CHUNK_SIZE,
  refresh = false,
  signal,
} = {}) {
  let streamId = "";
  try {
    let chunk = await invokeIpc("system_fonts_stream_start", {
      request: { chunkSize, refresh },
    });
    streamId = chunk?.streamId || "";
    while (chunk && !signal?.aborted) {
      const fonts = normalizeFonts(chunk.fonts);
      if (fonts.length || chunk.done) {
        yield {
          done: chunk.done === true,
          error: chunk.error || "",
          fonts,
          pending: chunk.pending === true,
        };
      }
      if (chunk.done === true || !streamId) return;
      chunk = await invokeIpc("system_fonts_stream_next", {
        request: { streamId },
      });
    }
  } catch (error) {
    if (!signal?.aborted) {
      logger.error("system-fonts.stream.failed", error);
      yield { done: true, error: String(error), fonts: [], pending: false };
    }
  } finally {
    if (streamId) {
      await invokeIpc("system_fonts_stream_cancel", {
        request: { streamId },
      }).catch((error) => logger.warn("system-fonts.stream.cancel.failed", error));
    }
  }
}
