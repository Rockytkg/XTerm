<script setup>
import { ToastProvider, TooltipProvider } from "reka-ui";
import AppShell from "./layouts/AppShell.vue";
import AppToasts from "./components/AppToasts.vue";
import ContextMenu from "./components/ContextMenu.vue";
import UpdateDialog from "./components/UpdateDialog.vue";
import { initializeContextMenuService } from "./services/contextMenu";
import { scheduleAutoUpdateCheck } from "./composables/useUpdateChecker";
import { useWorkspaceStore } from "./stores/workspaceStore";

useWorkspaceStore().init();
initializeContextMenuService();
scheduleAutoUpdateCheck();
</script>

<template>
  <TooltipProvider
    :delay-duration="60"
    :disable-hoverable-content="true"
    :ignore-non-keyboard-focus="true"
  >
    <ToastProvider
      label="Notifications"
      swipe-direction="up"
      :duration="600000"
    >
      <AppShell />
      <AppToasts />
      <!-- 启动自动检测与设置页手动检测共用的更新提示模态框 -->
      <UpdateDialog />
      <!-- 窗口内右键菜单（所有平台统一的渲染形态） -->
      <ContextMenu />
    </ToastProvider>
  </TooltipProvider>
</template>
