<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import {
  ArrowLeft,
  PaintBucket,
  Plus,
  Regex,
  Trash2,
  Type,
  WholeWord,
} from "@lucide/vue";
import AppTooltip from "../../components/AppTooltip.vue";
import { useHighlightSchemes } from "../../composables/useHighlightSchemes";
import { normalizeHighlightMatchType } from "../../utils/terminalPanelHelpers";

const props = defineProps({
  scheme: { type: Object, required: true },
});

const emit = defineEmits(["back"]);

const { t } = useI18n();
const {
  addRule,
  removeRule,
  updateRule,
  setRuleEffect,
  setRuleMatchType,
  setRuleColor,
  normalizeHighlightEffect,
  getRuleColor,
  getPatternPlaceholder,
  getRulePreviewText,
  getRulePreviewStyle,
} = useHighlightSchemes();

const schemeId = computed(() => props.scheme.id);

const matchTypeOptions = computed(() => [
  { label: t("settings.terminal.highlightMatchText"), value: "text", icon: WholeWord },
  { label: t("settings.terminal.highlightMatchRegex"), value: "regex", icon: Regex },
]);

const effectOptions = computed(() => [
  { label: t("settings.terminal.highlightEffectForeground"), value: "foreground", icon: Type },
  { label: t("settings.terminal.highlightEffectBackground"), value: "background", icon: PaintBucket },
]);
</script>

<template>
  <div class="highlight-editor">
    <div class="highlight-editor-head">
      <button
        type="button"
        class="highlight-back-button"
        @click="emit('back')"
      >
        <ArrowLeft
          :size="13"
          stroke-width="1.8"
        />
        {{ t("settings.terminal.highlightBack") }}
      </button>
      <span class="highlight-editor-title">
        {{ scheme.name || t("settings.terminal.highlightUntitled") }}
      </span>
    </div>

    <div class="highlight-rules-head">
      <span class="settings-label">
        {{ t("settings.terminal.highlightRules") }} ({{ scheme.rules.length }})
      </span>
      <button
        type="button"
        class="ui-button-secondary highlight-action"
        @click="addRule(schemeId)"
      >
        <Plus
          :size="13"
          stroke-width="1.8"
        />
        {{ t("settings.terminal.highlightAddRule") }}
      </button>
    </div>

    <div
      v-if="scheme.rules.length"
      class="highlight-rule-list"
    >
      <div
        v-for="(rule, index) in scheme.rules"
        :key="index"
        class="highlight-rule"
      >
        <AppTooltip
          :content="getRuleColor(rule)"
          side="top"
        >
          <label
            class="highlight-swatch"
            :style="{ backgroundColor: getRuleColor(rule) }"
          >
            <input
              class="highlight-swatch-input"
              :value="getRuleColor(rule)"
              type="color"
              :aria-label="t('settings.terminal.highlightColorValue')"
              @input="setRuleColor(schemeId, index, $event.target.value)"
            >
          </label>
        </AppTooltip>
        <input
          class="ui-input highlight-pattern-input"
          :value="rule.pattern"
          :placeholder="getPatternPlaceholder(rule)"
          :aria-label="t('settings.terminal.highlightPattern')"
          @input="updateRule(schemeId, index, { pattern: $event.target.value })"
        >
        <div class="highlight-seg">
          <AppTooltip
            v-for="option in matchTypeOptions"
            :key="option.value"
            :content="option.label"
            side="top"
          >
            <button
              type="button"
              class="highlight-seg-button"
              :class="{
                'highlight-seg-button-active':
                  normalizeHighlightMatchType(rule?.matchType) === option.value,
              }"
              :aria-label="option.label"
              :aria-pressed="normalizeHighlightMatchType(rule?.matchType) === option.value"
              @click="setRuleMatchType(schemeId, index, option.value)"
            >
              <component
                :is="option.icon"
                :size="13"
                stroke-width="1.8"
              />
            </button>
          </AppTooltip>
        </div>
        <div class="highlight-seg">
          <AppTooltip
            v-for="option in effectOptions"
            :key="option.value"
            :content="option.label"
            side="top"
          >
            <button
              type="button"
              class="highlight-seg-button"
              :class="{
                'highlight-seg-button-active': normalizeHighlightEffect(rule) === option.value,
              }"
              :aria-label="option.label"
              :aria-pressed="normalizeHighlightEffect(rule) === option.value"
              @click="setRuleEffect(schemeId, index, option.value)"
            >
              <component
                :is="option.icon"
                :size="13"
                stroke-width="1.8"
              />
            </button>
          </AppTooltip>
        </div>
        <AppTooltip
          :content="t('settings.terminal.highlightCaseSensitive')"
          side="top"
        >
          <button
            type="button"
            class="highlight-case-toggle"
            :class="{ 'highlight-case-toggle-active': !!rule.caseSensitive }"
            :aria-label="t('settings.terminal.highlightCaseSensitive')"
            :aria-pressed="!!rule.caseSensitive"
            @click="updateRule(schemeId, index, { caseSensitive: !rule.caseSensitive })"
          >
            Aa
          </button>
        </AppTooltip>
        <span
          class="highlight-rule-preview"
          :style="getRulePreviewStyle(rule)"
        >
          {{ getRulePreviewText(rule) }}
        </span>
        <AppTooltip
          :content="t('settings.terminal.highlightRemoveRule')"
          side="top"
        >
          <button
            type="button"
            class="highlight-icon-button highlight-icon-button-danger"
            :aria-label="t('settings.terminal.highlightRemoveRule')"
            @click="removeRule(schemeId, index)"
          >
            <Trash2
              :size="13"
              stroke-width="1.8"
            />
          </button>
        </AppTooltip>
      </div>
    </div>
    <div
      v-else
      class="highlight-rules-empty"
    >
      <span class="settings-hint">{{ t("settings.terminal.highlightNoRules") }}</span>
    </div>
  </div>
</template>
