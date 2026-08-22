<script setup>
import { ToastProvider, TooltipProvider } from "reka-ui";
import AppShell from "./layouts/AppShell.vue";
import AppToasts from "./components/AppToasts.vue";
import { initializeContextMenuService } from "./services/contextMenu";
import { useWorkspaceStore } from "./stores/workspaceStore";

useWorkspaceStore().init();
initializeContextMenuService();
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
    </ToastProvider>
  </TooltipProvider>
</template>
