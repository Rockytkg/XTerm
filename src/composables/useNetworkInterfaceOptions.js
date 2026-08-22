import { computed } from "vue";

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
