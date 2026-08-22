import {
  getNetworkInterfaces,
  getProxyConfig,
  getProxyStats,
  setProxyBindIp,
  startProxy,
  stopProxy,
  updateProxyPort,
} from "../services/proxy";
import { observeProxyStats } from "../events/proxyEventBus";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.workspace.proxy");

/**
 * Extracted proxy module — manages proxy config, stats, and control.
 * Kept as a plain module (not a separate Pinia store) so migration is
 * incremental and the workspaceStore API surface doesn't change.
 */
export function createWorkspaceProxyModule({ proxyConfig, proxyStats, proxyInterfaces }) {
  let refreshPromise = null;
  let configRevision = 0;
  let mutationChain = Promise.resolve();
  let pendingMutations = 0;

  async function refreshProxyInterfaces() {
    // 网络接口可能随时增删（VPN、热点等），不做 TTL 缓存，
    // 每次调用都重新枚举；refreshPromise 仅做并发去重。
    if (refreshPromise) {
      return refreshPromise;
    }
    logger.debug("proxy.interfaces.refresh.start");
    refreshPromise = getNetworkInterfaces()
      .then((interfaces) => {
        proxyInterfaces.value = Array.isArray(interfaces) ? interfaces : [];
        logger.info("proxy.interfaces.refresh.success", {
          interfaceCount: proxyInterfaces.value.length,
        });
        return proxyInterfaces.value;
      })
      .catch((error) => {
        logger.error("proxy.interfaces.refresh.failed", error);
        throw error;
      })
      .finally(() => {
        refreshPromise = null;
      });
    return refreshPromise;
  }

  async function hydrateProxy() {
    logger.debug("proxy.hydrate.start");
    const revision = configRevision;
    try {
      const [config, stats, interfaces] = await Promise.all([
        getProxyConfig(),
        getProxyStats(),
        refreshProxyInterfaces(),
      ]);
      if (revision !== configRevision || pendingMutations > 0) return;
      applyProxyConfig(config);
      applyProxyStats(stats);
      proxyInterfaces.value = Array.isArray(interfaces) ? interfaces : proxyInterfaces.value;
      logger.info("proxy.hydrate.success", {
        running: proxyConfig.value.running,
        port: proxyConfig.value.port,
        bindIp: proxyConfig.value.bindIp,
        interfaceCount: proxyInterfaces.value.length,
      });
    } catch (error) {
      logger.error("proxy.hydrate.failed", error);
      throw error;
    }
  }

  function applyProxyConfig(config) {
    proxyConfig.value = {
      bindIp: config.bindIp,
      port: config.port,
      running: config.running === true,
    };
    proxyStats.value = {
      ...proxyStats.value,
      bindIp: config.bindIp,
      port: config.port,
      running: config.running === true,
    };
    return proxyConfig.value;
  }

  function applyProxyStats(stats) {
    proxyStats.value = stats;
    return stats;
  }

  function mutateProxy(operation) {
    configRevision += 1;
    pendingMutations += 1;
    const run = async () => {
      try {
        return applyProxyConfig(await operation());
      } catch (error) {
        try {
          const [config, stats] = await Promise.all([getProxyConfig(), getProxyStats()]);
          applyProxyConfig(config);
          applyProxyStats(stats);
        } catch (refreshError) {
          logger.error("proxy.reconcile.failed", refreshError);
        }
        throw error;
      } finally {
        pendingMutations -= 1;
      }
    };
    const result = mutationChain.then(run, run);
    mutationChain = result.catch(() => undefined);
    return result;
  }

  function startProxyServer(port = proxyConfig.value.port, bindIp = proxyConfig.value.bindIp) {
    const request = { port, bindIp };
    return mutateProxy(async () => {
      await refreshProxyInterfaces();
      return startProxy(request.port, request.bindIp);
    });
  }

  function stopProxyServer() {
    return mutateProxy(stopProxy);
  }

  function updateProxyServerPort(port) {
    return mutateProxy(() => updateProxyPort(port));
  }

  function updateProxyServerBindIp(bindIp) {
    return mutateProxy(async () => {
      await refreshProxyInterfaces();
      return setProxyBindIp(bindIp);
    });
  }

  async function startObserving() {
    return observeProxyStats((payload) => {
      if (!payload) return;
      if (pendingMutations > 0) return;
      configRevision += 1;
      applyProxyStats(payload);
      applyProxyConfig(payload);
    });
  }

  return {
    applyProxyConfig,
    applyProxyStats,
    hydrateProxy,
    refreshProxyInterfaces,
    startObserving,
    startProxyServer,
    stopProxyServer,
    updateProxyServerBindIp,
    updateProxyServerPort,
  };
}
