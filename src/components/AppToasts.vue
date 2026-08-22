<script setup>
import { ToastClose, ToastDescription, ToastRoot, ToastTitle, ToastViewport } from "reka-ui";
import { AlertTriangle, CheckCircle2, Info, LoaderCircle, X } from "@lucide/vue";
import { useToasts } from "../composables/useToasts";
import "../styles/toast.scss";

const { dismissToast, toasts } = useToasts();

const icons = {
  error: AlertTriangle,
  info: Info,
  loading: LoaderCircle,
  success: CheckCircle2,
};
</script>

<template>
  <ToastViewport class="toast-viewport">
    <ToastRoot
      v-for="toast in toasts"
      :key="toast.id"
      v-model:open="toast.open"
      class="toast-root"
      :class="`toast-${toast.type}`"
      :duration="toast.duration"
      @update:open="
        (value) => {
          if (!value) dismissToast(toast.id);
        }
      "
    >
      <div class="toast-icon">
        <component
          :is="icons[toast.type] || Info"
          :size="17"
          stroke-width="1.9"
          :class="{ 'animate-spin': toast.type === 'loading' }"
        />
      </div>
      <div class="toast-body">
        <ToastTitle
          v-if="toast.title"
          class="toast-title"
        >
          {{ toast.title }}
        </ToastTitle>
        <ToastDescription
          v-if="toast.message"
          class="toast-desc"
        >
          {{ toast.message }}
        </ToastDescription>
      </div>
      <ToastClose
        class="toast-close"
        :aria-label="$t('actions.close')"
      >
        <X
          :size="14"
          stroke-width="1.9"
        />
      </ToastClose>
    </ToastRoot>
  </ToastViewport>
</template>
