import { resolveConnectionFromUri, startDeepLinkHandler } from "../composables/useDeepLinkHandler";
import { router } from "../router";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.workspace.external_sessions");

export function createWorkspaceExternalSessionController({ connectTo, upsertExternalConnection }) {
  let stopDeepLinkHandler = null;

  async function startDeepLinks() {
    if (stopDeepLinkHandler) return;
    stopDeepLinkHandler = await startDeepLinkHandler(openDeepLink);
  }

  function stopDeepLinks() {
    // HMR/store 重建时必须先解绑，否则 onOpenUrl 会重复注册。
    stopDeepLinkHandler?.();
    stopDeepLinkHandler = null;
  }

  async function openDeepLink(uri) {
    try {
      integrateExternalOpen(await resolveConnectionFromUri(uri));
    } catch (error) {
      logger.error("deeplink.open.failed", { uri, error });
    }
  }

  function integrateExternalOpen(result) {
    const connectionId = result?.connectionId;
    if (!connectionId) {
      logger.warn("external_open.missing_connection_id", {
        awaiting: result?.awaiting || null,
      });
      return false;
    }

    const connection = normalizeExternalConnection(result);
    upsertExternalConnection(connection);
    connectTo(connectionId);
    navigateToWorkspace();

    logger.info("external_open.integrated", {
      connectionId,
      protocol: connection.protocol,
    });
    return true;
  }

  function normalizeExternalConnection(result) {
    const endpoint = result?.endpoint ?? {};
    const protocol = endpoint.protocol || result?.protocol || "ssh";
    const host = endpoint.host || "";
    const port = endpoint.port;
    const user = endpoint.user || "";

    return {
      id: result.connectionId,
      protocol,
      host,
      port: port != null ? String(port) : undefined,
      user,
      name: result?.name || formatEndpointName({ user, host, port }),
      authMethod: undefined,
      savedCredentialId: undefined,
      encoding: undefined,
      realtimeEncodingDetection: undefined,
      terminalType: undefined,
      terminalScrollback: undefined,
      backspaceSends: undefined,
      terminalHighlightEnabled: undefined,
      serialQuickAutoBaud: undefined,
      dataBits: undefined,
      flowControl: undefined,
      parity: undefined,
      stopBits: undefined,
      jumpHosts: undefined,
    };
  }

  function navigateToWorkspace() {
    if (router.currentRoute.value.name === "workspace") return;
    router.push({ name: "workspace" }).catch((error) => {
      logger.warn("external_open.navigate_failed", error);
    });
  }

  return {
    startDeepLinks,
    stopDeepLinks,
  };
}

function formatEndpointName({ user, host, port }) {
  const authority = [user, host].filter(Boolean).join("@");
  return authority ? authority + (port ? `:${port}` : "") : "External session";
}
