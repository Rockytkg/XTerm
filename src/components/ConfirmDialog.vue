<script setup>
import "../styles/confirm-dialog.scss";
import { computed, nextTick, ref } from "vue";
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

const dialogIcon = computed(() => props.icon || fallbackIcons[props.tone] || Info);
const cancelLabel = computed(() => props.cancelText || t("actions.cancel"));
const confirmLabel = computed(() => props.confirmText || t("actions.save"));
const toneClass = computed(() => `confirm-dialog-tone-${props.tone}`);
const toneButtonClass = computed(() => `confirm-dialog-confirm-${props.tone}`);

function setOpen(value) {
  if (props.loading && !value) return;
  emit("update:open", value);
  if (!value) emit("cancel");
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
    const target =
      props.tone === "danger" && props.showCancel ? cancelButton.value : confirmButton.value;
    target?.focus?.();
  });
}
</script>

<template>
  <AlertDialogRoot
    :open="open"
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
              {{ title }}
            </AlertDialogTitle>
            <AlertDialogDescription
              v-if="description"
              class="confirm-dialog-description"
            >
              {{ description }}
            </AlertDialogDescription>
          </div>
        </header>

        <slot />

        <footer class="confirm-dialog-footer">
          <AlertDialogCancel
            v-if="showCancel"
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
            v-if="secondaryText"
            type="button"
            class="confirm-dialog-action confirm-dialog-action-secondary"
            :disabled="loading"
            @click="secondary"
          >
            {{ secondaryText }}
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
              :is="confirmIcon"
              v-else-if="confirmIcon"
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
