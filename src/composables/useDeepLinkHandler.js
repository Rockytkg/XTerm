import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { invokeDetailedIpc } from "../services/ipc/core";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.deeplink");

/**
 * Bridge the native deep-link plugin to the workspace-level connection flow.
 * Plugin lifecycle stays here; connection opening and navigation stay outside.
 */
export async function startDeepLinkHandler(handleUrl) {
  const unlisten = await onOpenUrl((urls) => {
    processUrls(urls, handleUrl);
  });

  processUrls(await readStartupUrls(), handleUrl, true);

  logger.debug("deeplink.listener.ready");
  return () => unlisten?.();
}

async function readStartupUrls() {
  try {
    return (await getCurrent()) ?? [];
  } catch (error) {
    logger.error("deeplink.startup_urls.failed", error);
    return [];
  }
}

export function resolveConnectionFromUri(uri) {
  return invokeDetailedIpc("terminal_resolve_uri", { uri }, { level: "info" });
}

function processUrls(urls, handleUrl, defer = false) {
  const supported = normalizeUrls(urls);

  logger.info("deeplink.open", { count: supported.length });
  for (const url of supported) {
    if (defer) {
      queueMicrotask(() => handleUrl(url));
    } else {
      handleUrl(url);
    }
  }
}

function normalizeUrls(urls) {
  return (Array.isArray(urls) ? urls : []).filter((url) => isSupportedUrl(url));
}

function isSupportedUrl(url) {
  if (typeof url !== "string" || url.length === 0) return false;
  return url.startsWith("ssh://") || url.startsWith("telnet://");
}
