<script setup>
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { ShieldCheck, ShieldQuestion } from "@lucide/vue";

defineProps({
  prompt: { type: Object, required: true },
  title: { type: String, default: "Host Key Verification" },
  description: { type: String, default: "" },
  algorithmLabel: { type: String, default: "Algorithm" },
  fingerprintLabel: { type: String, default: "Fingerprint" },
  cancelLabel: { type: String, default: "Cancel" },
  onceLabel: { type: String, default: "Trust Once" },
  saveLabel: { type: String, default: "Trust & Save" },
});

const emit = defineEmits(["answer"]);
</script>

<template>
  <DialogRoot :open="true">
    <DialogPortal>
      <DialogOverlay class="dialog-overlay host-key-dialog-overlay" />
      <DialogContent
        class="dialog-content host-key-dialog"
        @escape-key-down.prevent
        @pointer-down-outside.prevent
        @interact-outside.prevent
      >
        <header class="host-key-dialog-header">
          <div
            class="host-key-dialog-icon"
            aria-hidden="true"
          >
            <ShieldQuestion
              :size="22"
              stroke-width="1.8"
            />
          </div>
          <div>
            <DialogTitle class="host-key-dialog-title">
              {{ title }}
            </DialogTitle>
            <DialogDescription class="host-key-dialog-desc">
              {{ description }}
            </DialogDescription>
          </div>
        </header>
        <dl class="host-key-details">
          <div>
            <dt>{{ algorithmLabel }}</dt>
            <dd>{{ prompt.algorithm }}</dd>
          </div>
          <div>
            <dt>{{ fingerprintLabel }}</dt>
            <dd>{{ prompt.fingerprint }}</dd>
          </div>
        </dl>
        <footer class="host-key-dialog-actions">
          <button
            type="button"
            class="ui-button-secondary"
            @click="emit('answer', 'cancel')"
          >
            {{ cancelLabel }}
          </button>
          <button
            type="button"
            class="ui-button-secondary"
            @click="emit('answer', 'once')"
          >
            {{ onceLabel }}
          </button>
          <button
            type="button"
            class="ui-button-primary host-key-save"
            @click="emit('answer', 'save')"
          >
            <ShieldCheck
              :size="14"
              stroke-width="1.9"
            />
            {{ saveLabel }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
