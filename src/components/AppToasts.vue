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
    <!-- duration 固定 0（禁用 reka 内建关闭定时器）：倒计时由 useToasts 单一持有，
         因为 updateToast 需要在时长不变时也重新计时（如 loading→success 同默认时长），
         库内定时器只在 duration 变化时重排，无法表达该语义；此前双计时并存还会在
         悬停时互相矛盾（库暂停、手写照常引爆）。open 为受控属性，关闭统一经
         update:open → dismissToast。 -->
    <ToastRoot
      v-for="toast in toasts"
      :key="toast.id"
      :open="toast.open"
      class="toast-root"
      :class="`toast-${toast.type}`"
      :duration="0"
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
