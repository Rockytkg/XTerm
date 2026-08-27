import { computed, ref } from "vue";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.workspace.network_interfaces");

export function useNetworkInterfaceOptions({ interfaces, bindIp, staleLabel }) {
  const options = computed(() => {
    const baseOptions = interfaces.value.map((item) => ({
      label: item.label,
      value: item.ip,
    }));
    const selectedBindIp = bindIp.value;
    if (!selectedBindIp) return baseOptions;
    const hasSelected = baseOptions.some((item) => item.value === selectedBindIp);
    if (hasSelected) return baseOptions;
    return [
      {
        label: `${selectedBindIp} (${staleLabel.value})`,
        value: selectedBindIp,
      },
      ...baseOptions,
    ];
  });

  return {
    interfaceOptions: options,
  };
}

// 代理/文件服务视图共用的网卡列表刷新；失败只记录日志，界面保持原状由用户重试。
export function useProxyInterfaceRefresh({ workspace }) {
  const refreshingInterfaces = ref(false);

  async function refreshInterfaces() {
    if (refreshingInterfaces.value) return;
    refreshingInterfaces.value = true;
    try {
      await workspace.refreshProxyInterfaces();
    } catch (error) {
      logger.error("refresh.failed", error);
    } finally {
      refreshingInterfaces.value = false;
    }
  }

  return { refreshingInterfaces, refreshInterfaces };
}
