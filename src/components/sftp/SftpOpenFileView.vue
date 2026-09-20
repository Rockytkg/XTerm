<script setup>
import { computed, defineAsyncComponent } from "vue";
import { useI18n } from "vue-i18n";
import { ArrowLeft } from "@lucide/vue";
import { formatBytes } from "../../utils/formatBytes";
// 编辑器/预览外框（sftp-editor* 类）的样式定义在 sftp.scss，组件自行引入以保证脱离 SFTP 面板时可用。
import "../../styles/sftp.scss";
const CodeEditor = defineAsyncComponent(() => import("../CodeEditor.vue"));
const OpenFileViewer = defineAsyncComponent(() => import("./SftpOpenFileViewer.vue"));

const props = defineProps({
  closeFile: { type: Function, required: true },
  convertToEdit: { type: Function, required: true },
  downloadFile: { type: Function, required: true },
  downloadResource: { type: Function, required: true },
  file: { type: Object, default: null },
  fontSizeChange: { type: Function, required: true },
  preferences: { type: Object, required: true },
  resolvedEditorTheme: { type: String, default: "light" },
  resolvedTheme: { type: String, default: "light" },
  saveFile: { type: Function, required: true },
  saveFileAndClose: { type: Function, required: true },
  updateContent: { type: Function, required: true },
});

const { t, locale } = useI18n();

const isEditMode = computed(() => props.file?.mode === "edit");
const isDirty = computed(
  () => props.file?.mode === "edit" && props.file.content !== props.file.savedContent,
);
const canConvertToEdit = computed(
  () => props.file?.mode === "preview" && props.file.editable && !props.file.tooLarge,
);
// 预览库内置中英文案，与应用语言一一对应
const viewerLocale = computed(() => (locale.value === "zh-CN" ? "zh-CN" : "en-US"));
const viewerTheme = computed(() => (props.resolvedTheme === "dark" ? "dark" : "light"));

function formatMtime(file) {
  return file?.mtime ? new Date(file.mtime * 1000).toLocaleString() : "-";
}

// 预览库内部下载链接（fallback 等）经 SftpOpenFileViewer 拦截后回调到这里
function downloadCurrentFile() {
  if (props.file) props.downloadFile(props.file);
}
</script>

<template>
  <div
    v-if="file"
    class="sftp-editor-panel"
  >
    <CodeEditor
      v-if="isEditMode"
      :back-label="t('sftp.editor.backToFiles')"
      :content="file.content"
      :dirty="isDirty"
      :error="file.error"
      :font-family="preferences.editorFontFamily"
      :font-size="preferences.editorFontSize"
      :highlight-current-line="preferences.editorHighlightActiveLine"
      :line-wrapping="preferences.editorLineWrapping"
      :loading="file.loading"
      :loading-label="t('sftp.editor.loading')"
      :path="file.path"
      :resolved-theme="resolvedEditorTheme"
      :save-label="t('actions.save')"
      :saving="file.saving"
      :tab-size="preferences.editorTabSize"
      :title="file.name"
      @back="closeFile()"
      @font-size-change="fontSizeChange"
      @save="saveFile()"
      @save-and-back="saveFileAndClose()"
      @update:content="updateContent($event)"
    />

    <section
      v-else
      class="sftp-editor"
    >
      <header class="sftp-editor-toolbar">
        <button
          type="button"
          class="sftp-editor-back"
          @click="closeFile()"
        >
          <ArrowLeft
            :size="16"
            stroke-width="2"
          />
          <span>{{ t("sftp.editor.backToFiles") }}</span>
        </button>
        <div class="sftp-editor-title">
          <span class="sftp-editor-title-text">{{ file.name }}</span>
          <span class="sftp-editor-path">{{ file.path }}</span>
        </div>
        <div class="sftp-editor-actions">
          <button
            v-if="canConvertToEdit"
            type="button"
            class="sftp-button"
            :disabled="file.loading || !!file.error"
            @click="convertToEdit()"
          >
            {{ t("sftp.preview.edit") }}
          </button>
          <button
            type="button"
            class="sftp-button"
            @click="downloadFile(file)"
          >
            {{ t("sftp.preview.download") }}
          </button>
          <button
            type="button"
            class="sftp-button"
            @click="closeFile()"
          >
            {{ t("sftp.preview.close") }}
          </button>
        </div>
      </header>
      <div
        v-if="file.error"
        class="sftp-editor-error"
      >
        {{ file.error }}
      </div>
      <div
        v-if="file.loading"
        class="sftp-editor-loading"
      >
        {{ t("sftp.editor.loading") }}
      </div>
      <div class="sftp-preview-body">
        <template v-if="!file.loading && !file.error">
          <div
            v-if="file.tooLarge"
            class="sftp-preview-fallback"
          >
            <p class="sftp-preview-fallback-message">
              {{ t("sftp.preview.tooLarge") }}
            </p>
            <dl class="sftp-preview-fallback-meta">
              <dt>{{ t("sftp.name") }}</dt>
              <dd>{{ file.name }}</dd>
              <dt>{{ t("sftp.preview.sizeLabel") }}</dt>
              <dd>{{ formatBytes(file.size) }}</dd>
              <dt>{{ t("sftp.preview.modifiedLabel") }}</dt>
              <dd>{{ formatMtime(file) }}</dd>
            </dl>
            <button
              type="button"
              class="sftp-button"
              @click="downloadFile(file)"
            >
              {{ t("sftp.preview.download") }}
            </button>
          </div>
          <OpenFileViewer
            v-else-if="file.blob"
            :file="file.blob"
            :file-name="file.name"
            :locale="viewerLocale"
            :mime-type="file.mime || undefined"
            :on-download="downloadCurrentFile"
            :on-download-resource="downloadResource"
            :theme="viewerTheme"
          />
        </template>
      </div>
    </section>
  </div>
</template>
