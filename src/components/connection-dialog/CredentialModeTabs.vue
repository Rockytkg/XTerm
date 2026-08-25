<script setup>
import { ToggleGroupItem, ToggleGroupRoot } from "reka-ui";
import { KeyRound, Lock, ShieldCheck } from "@lucide/vue";
import { useI18n } from "vue-i18n";

// Shared auth-method segmented tabs: an optional "saved credential" entry
// followed by the given inline methods ("password" / "key").
defineProps({
  modelValue: { type: String, required: true },
  showSaved: { type: Boolean, default: false },
  methods: { type: Array, default: () => ["password"] },
});
const emit = defineEmits(["select"]);
const { t } = useI18n();
</script>

<template>
  <ToggleGroupRoot
    :model-value="modelValue"
    type="single"
    class="conn-seg-tabs"
    @update:model-value="emit('select', $event)"
  >
    <ToggleGroupItem
      v-if="showSaved"
      value="saved"
      class="conn-seg-tab"
    >
      <ShieldCheck
        :size="11"
        stroke-width="2"
      />
      {{ t("connectionDialog.authMethods.savedCredential") }}
    </ToggleGroupItem>
    <ToggleGroupItem
      v-for="method in methods"
      :key="method"
      :value="method"
      class="conn-seg-tab"
    >
      <Lock
        v-if="method === 'password'"
        :size="11"
        stroke-width="2"
      />
      <KeyRound
        v-else
        :size="11"
        stroke-width="2"
      />
      {{ t(`connectionDialog.authMethods.${method}`) }}
    </ToggleGroupItem>
  </ToggleGroupRoot>
</template>
