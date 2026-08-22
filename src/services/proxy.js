import { createLogger } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const proxyLogger = createLogger("frontend.proxy.service");

const runProxyRequest = createServiceRunner({
  logger: proxyLogger,
  module: "proxy",
});

export function getProxyConfig() {
  return runProxyRequest("get_proxy_config", undefined, {
    action: "proxy.config.get",
    level: "debug",
    successLevel: "debug",
  });
}

export function getProxyStats() {
  return runProxyRequest("get_proxy_stats", undefined, {
    action: "proxy.stats.get",
    level: "debug",
    successLevel: "debug",
  });
}

export function getNetworkInterfaces() {
  return runProxyRequest("get_network_interfaces", undefined, {
    action: "proxy.interfaces.get",
    level: "debug",
    successLevel: "debug",
  });
}

export function startProxy(port, bindIp) {
  return runProxyRequest(
    "start_proxy",
    { port, bindIp },
    {
      action: "proxy.start",
      context: { port, bindIp },
      summarizePayload: () => ({ port, bindIp }),
    },
  );
}

export function stopProxy() {
  return runProxyRequest("stop_proxy", undefined, {
    action: "proxy.stop",
    level: "warn",
    successLevel: "info",
  });
}

export function updateProxyPort(newPort) {
  return runProxyRequest(
    "update_port",
    { newPort },
    {
      action: "proxy.port.update",
      context: { port: newPort },
      summarizePayload: () => ({ newPort }),
    },
  );
}

export function setProxyBindIp(bindIp) {
  return runProxyRequest(
    "set_proxy_bind_ip",
    { bindIp },
    {
      action: "proxy.bind_ip.set",
      context: { bindIp },
      summarizePayload: () => ({ bindIp }),
    },
  );
}
