<script setup>
import "../styles/confirm-dialog.scss";
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogOverlay,
  AlertDialogPortal,
  AlertDialogRoot,
  AlertDialogTitle,
} from "reka-ui";
import { AlertTriangle, CheckCircle2, Info, LoaderCircle } from "@lucide/vue";

const props = defineProps({
  open: { type: Boolean, default: false },
  title: { type: String, required: true },
  description: { type: String, default: "" },
  confirmText: { type: String, default: "" },
  cancelText: { type: String, default: "" },
  secondaryText: { type: String, default: "" },
  showCancel: { type: Boolean, default: true },
  tone: {
    type: String,
    default: "danger",
    validator: (value) => ["danger", "warning", "info", "success"].includes(value),
  },
  loading: { type: Boolean, default: false },
  icon: { type: [Object, Function], default: null },
  confirmIcon: { type: [Object, Function], default: null },
});

const emit = defineEmits(["update:open", "confirm", "cancel", "secondary"]);
const { t } = useI18n();
const cancelButton = ref(null);
const confirmButton = ref(null);

const fallbackIcons = {
  danger: AlertTriangle,
  warning: AlertTriangle,
  info: Info,
  success: CheckCircle2,
};

// 关闭动画期间冻结渲染内容。典型调用方在关闭的同时就清空触发数据
// （pendingDelete = null 之类），若模板直接响应 props，标题/描述会先于
// 弹壳消失，留下一个空框完成退出动画。因此打开期间把内容 props 快照到
// latchedContent，关闭后直到下次打开前都沿用这份快照。
const latchedContent = ref(null);

function snapshotContent() {
  return {
    title: props.title,
    description: props.description,
    confirmText: props.confirmText,
    cancelText: props.cancelText,
    secondaryText: props.secondaryText,
    showCancel: props.showCancel,
    tone: props.tone,
    icon: props.icon,
    confirmIcon: props.confirmIcon,
  };
}

watch(
  () => [
    props.open,
    props.title,
    props.description,
    props.confirmText,
    props.cancelText,
    props.secondaryText,
    props.showCancel,
    props.tone,
    props.icon,
    props.confirmIcon,
  ],
  () => {
    if (props.open) latchedContent.value = snapshotContent();
  },
  { immediate: true },
);

const dialogIcon = computed(() => {
  const content = latchedContent.value;
  return content?.icon || fallbackIcons[content?.tone] || Info;
});
const cancelLabel = computed(() => latchedContent.value?.cancelText || t("actions.cancel"));
const confirmLabel = computed(() => latchedContent.value?.confirmText || t("actions.save"));
const toneClass = computed(() => `confirm-dialog-tone-${latchedContent.value?.tone || "danger"}`);
const toneButtonClass = computed(
  () => `confirm-dialog-confirm-${latchedContent.value?.tone || "danger"}`,
);

// 取消/ESC/遮罩路径延迟向父组件传播关闭：先让根组件进入 closed 状态播放
// 退出动画，动画结束后才 emit update:open。这样父组件的触发数据（含 slot
// 内容）在动画期间保持原样。时长需覆盖 --motion-duration-quick（70ms）。
const CLOSE_PROPAGATION_DELAY = 120;
const closing = ref(false);
let closeTimer = null;
const renderedOpen = computed(() => props.open && !closing.value);

function clearCloseTimer() {
  if (closeTimer) {
    clearTimeout(closeTimer);
    closeTimer = null;
  }
}

watch(
  () => props.open,
  (open) => {
    // 父组件主动重开时丢弃挂起的关闭传播
    if (open) clearCloseTimer();
    closing.value = false;
  },
);

function setOpen(value) {
  if (value) {
    clearCloseTimer();
    closing.value = false;
    emit("update:open", true);
    return;
  }
  if (props.loading || closing.value || !props.open) return;
  closing.value = true;
  closeTimer = setTimeout(() => {
    closeTimer = null;
    emit("update:open", false);
    emit("cancel");
    // 父组件未响应关闭（未把 open 置 false）时恢复显示，避免弹窗卡在隐藏态
    closing.value = false;
  }, CLOSE_PROPAGATION_DELAY);
}

function confirm() {
  if (props.loading) return;
  emit("confirm");
}

function secondary() {
  if (props.loading) return;
  emit("secondary");
}

function preventCloseWhileLoading(event) {
  if (props.loading) event.preventDefault();
}

function focusInitialAction(event) {
  event.preventDefault();
  nextTick(() => {
    const content = latchedContent.value;
    const target =
      content?.tone === "danger" && content?.showCancel ? cancelButton.value : confirmButton.value;
    target?.focus?.();
  });
}

onBeforeUnmount(clearCloseTimer);
</script>

<template>
  <AlertDialogRoot
    :open="renderedOpen"
    @update:open="setOpen"
  >
    <AlertDialogPortal>
      <AlertDialogOverlay
        class="dialog-overlay confirm-dialog-overlay z-[70] bg-[color-mix(in_oklch,var(--overlay-bg)_90%,transparent)]"
      />
      <AlertDialogContent
        class="dialog-content confirm-dialog-content focus:outline-none"
        :class="toneClass"
        @escape-key-down="preventCloseWhileLoading"
        @open-auto-focus="focusInitialAction"
      >
        <header class="confirm-dialog-header">
          <div
            class="confirm-dialog-tone-icon"
            aria-hidden="true"
          >
            <component
              :is="dialogIcon"
              :size="20"
              stroke-width="1.9"
            />
          </div>
          <div class="confirm-dialog-copy">
            <AlertDialogTitle class="confirm-dialog-title">
              {{ latchedContent?.title }}
            </AlertDialogTitle>
            <AlertDialogDescription
              v-if="latchedContent?.description"
              class="confirm-dialog-description"
            >
              {{ latchedContent.description }}
            </AlertDialogDescription>
          </div>
        </header>

        <slot />

        <footer class="confirm-dialog-footer">
          <AlertDialogCancel
            v-if="latchedContent?.showCancel ?? true"
            as-child
          >
            <button
              ref="cancelButton"
              type="button"
              class="confirm-dialog-action confirm-dialog-action-secondary"
              :disabled="loading"
            >
              {{ cancelLabel }}
            </button>
          </AlertDialogCancel>
          <button
            v-if="latchedContent?.secondaryText"
            type="button"
            class="confirm-dialog-action confirm-dialog-action-secondary"
            :disabled="loading"
            @click="secondary"
          >
            {{ latchedContent.secondaryText }}
          </button>
          <button
            ref="confirmButton"
            type="button"
            class="confirm-dialog-action confirm-dialog-action-primary"
            :class="toneButtonClass"
            :disabled="loading"
            @click="confirm"
          >
            <LoaderCircle
              v-if="loading"
              class="confirm-dialog-spinner"
              :size="13"
              stroke-width="2"
            />
            <component
              :is="latchedContent?.confirmIcon"
              v-else-if="latchedContent?.confirmIcon"
              :size="13"
              stroke-width="1.9"
            />
            {{ confirmLabel }}
          </button>
        </footer>
      </AlertDialogContent>
    </AlertDialogPortal>
  </AlertDialogRoot>
</template>
