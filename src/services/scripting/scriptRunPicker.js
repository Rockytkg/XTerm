import { ref } from "vue";

// 工作区工具栏“运行脚本”按钮打开的脚本选择器开关，挂在 AppShell。
export const scriptRunPickerOpen = ref(false);

export function openScriptRunPicker() {
  scriptRunPickerOpen.value = true;
}

export function closeScriptRunPicker() {
  scriptRunPickerOpen.value = false;
}
