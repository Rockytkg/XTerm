<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { basicSetup, EditorView } from "codemirror";
import { autocompletion, closeBrackets } from "@codemirror/autocomplete";
import { redo, undo } from "@codemirror/commands";
import { bracketMatching, LanguageDescription, syntaxTree } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { linter, lintGutter } from "@codemirror/lint";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { EditorSelection, EditorState, Compartment } from "@codemirror/state";
import { highlightActiveLine, keymap } from "@codemirror/view";
import { ArrowLeft } from "@lucide/vue";
import { editorChromeTheme, editorThemeExtension } from "../utils/editorTheme";
import { isPrimaryModifier } from "../utils/platform";
import { formatScriptWithCursor } from "../services/scripting/scriptSyntax.js";
// 编辑器外框（sftp-editor*/sftp-button 类）的样式定义在 sftp.scss，组件自行引入以保证脱离 SFTP 面板时可用。
import "../styles/sftp.scss";

const props = defineProps({
  backLabel: { type: String, required: true },
  content: { type: String, default: "" },
  dirty: { type: Boolean, default: false },
  error: { type: String, default: "" },
  fontFamily: { type: String, default: "" },
  fontSize: { type: Number, default: 14 },
  formatLabel: { type: String, default: "Format" },
  formattingEnabled: { type: Boolean, default: false },
  highlightCurrentLine: { type: Boolean, default: true },
  lineWrapping: { type: Boolean, default: true },
  loading: { type: Boolean, default: false },
  loadingLabel: { type: String, required: true },
  path: { type: String, required: true },
  readonly: { type: Boolean, default: false },
  resolvedTheme: { type: String, default: "light" },
  saveLabel: { type: String, required: true },
  saving: { type: Boolean, default: false },
  tabSize: { type: Number, default: 2 },
  title: { type: String, required: true },
});

const emit = defineEmits([
  "back",
  "fontSizeChange",
  "format-error",
  "formatted",
  "save",
  "saveAndBack",
  "update:content",
]);
const { t, locale } = useI18n();

const editorRoot = ref(null);
const readonlyCompartment = new Compartment();
const featureCompartment = new Compartment();
const languageCompartment = new Compartment();
const phrasesCompartment = new Compartment();
const themeCompartment = new Compartment();

let view = null;
let languageLoadToken = 0;
// 最近一次 emit 给父组件的内容。父组件（v-model 写法）会把同一字符串写回 content prop，
// watch 中做引用比较即可 O(1) 跳过这种回写，避免每次按键全量 toString 比较。
let lastEmittedContent = props.content;
const formatting = ref(false);

// 状态栏数据：光标位置、选区长度、总行数、语言名，随编辑/选区更新。
const cursorLine = ref(1);
const cursorCol = ref(1);
const selectionLength = ref(0);
const lineCount = ref(1);
const languageName = ref("");

const javascriptLinter = linter((editorView) => {
  const diagnostics = [];
  syntaxTree(editorView.state).iterate({
    enter(node) {
      if (!node.type.isError) return;
      const length = editorView.state.doc.length;
      if (!length) return;
      const from = Math.min(node.from, length - 1);
      diagnostics.push({
        from,
        to: Math.min(length, Math.max(node.to, from + 1)),
        severity: "error",
        message: t("scripts.syntaxError"),
      });
    },
  });
  return diagnostics;
});

async function loadLanguageExtension(path) {
  const description = LanguageDescription.matchFilename(languages, path);
  if (!description) return null;
  const extension = await description.load();
  return {
    name: description.name,
    // JavaScript 文件附带语法错误诊断；格式化按钮仍由 formattingEnabled 单独控制。
    support: /\.js$/i.test(path) ? [extension, javascriptLinter, lintGutter()] : extension,
  };
}

function configureLanguage(path) {
  const token = ++languageLoadToken;
  loadLanguageExtension(path)
    .then((result) => {
      if (!view || token !== languageLoadToken) return;
      languageName.value = result?.name || "";
      view.dispatch({
        effects: languageCompartment.reconfigure(result?.support || []),
      });
    })
    .catch(() => {
      if (!view || token !== languageLoadToken) return;
      languageName.value = "";
      view.dispatch({
        effects: languageCompartment.reconfigure([]),
      });
    });
}

async function formatContent() {
  if (!view || formatting.value || !props.formattingEnabled || props.readonly || props.loading) {
    return;
  }
  formatting.value = true;
  try {
    const original = view.state.doc.toString();
    const result = await formatScriptWithCursor(original, view.state.selection.main.head, {
      tabWidth: props.tabSize,
    });
    if (view.state.doc.toString() !== original) return;
    const changed = result.formatted !== original;
    if (changed) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: result.formatted },
        selection: EditorSelection.cursor(Math.min(result.cursorOffset, result.formatted.length)),
        scrollIntoView: true,
      });
    }
    emit("formatted", changed);
  } catch (error) {
    emit("format-error", error);
  } finally {
    formatting.value = false;
  }
}

function readonlyExtension(readonly) {
  return [EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)];
}

function featureExtensions() {
  return [
    EditorState.tabSize.of(Math.min(8, Math.max(1, Math.round(Number(props.tabSize) || 2)))),
    props.lineWrapping ? EditorView.lineWrapping : [],
    props.highlightCurrentLine ? highlightActiveLine() : [],
  ];
}

function searchPhrasesExtension() {
  return EditorState.phrases.of({
    Find: t("sftp.editor.search.find"),
    Replace: t("sftp.editor.search.replace"),
    all: t("sftp.editor.search.all"),
    "by word": t("sftp.editor.search.byWord"),
    close: t("sftp.editor.search.close"),
    "current match": t("sftp.editor.search.currentMatch"),
    "Go to line": t("sftp.editor.search.goToLine"),
    go: t("sftp.editor.search.go"),
    "match case": t("sftp.editor.search.matchCase"),
    next: t("sftp.editor.search.next"),
    "on line": t("sftp.editor.search.onLine"),
    previous: t("sftp.editor.search.previous"),
    regexp: t("sftp.editor.search.regexp"),
    replace: t("sftp.editor.search.replaceButton"),
    "replace all": t("sftp.editor.search.replaceAll"),
    "replaced $ matches": t("sftp.editor.search.replacedMatches"),
    "replaced match on line $": t("sftp.editor.search.replacedMatchOnLine"),
  });
}

const FONT_SIZE_MIN = 10;
const FONT_SIZE_MAX = 28;

function clampFontSize(value) {
  return Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, Math.round(Number(value) || 14)));
}

function cssFontFamily(fontFamily) {
  const value = String(fontFamily || "").trim();
  if (!value) return "monospace";
  return `"${value.replaceAll('"', '\\"')}", monospace`;
}

// Ctrl+滚轮缩放的字号覆盖值（null = 跟随 prop），变化时通知父级持久化。
// 字号经 --sftp-editor-font-size CSS 变量生效（见模板 style 绑定与 sftp.scss），
// 无需重建任何编辑器扩展。
const zoomedFontSize = ref(null);
const effectiveFontSize = computed(() => clampFontSize(zoomedFontSize.value ?? props.fontSize));

function editorFontSize() {
  return `${effectiveFontSize.value}px`;
}

function handleEditorWheel(event) {
  if (!isPrimaryModifier(event)) return;
  event.preventDefault();
  const next = clampFontSize(effectiveFontSize.value + (event.deltaY < 0 ? 1 : -1));
  if (next === effectiveFontSize.value) return;
  zoomedFontSize.value = next;
  emit("fontSizeChange", next);
}

function syncStatus(state) {
  const main = state.selection.main;
  const line = state.doc.lineAt(main.head);
  cursorLine.value = line.number;
  cursorCol.value = main.head - line.from + 1;
  selectionLength.value = state.selection.ranges.reduce(
    (sum, range) => sum + range.to - range.from,
    0,
  );
  lineCount.value = state.doc.lines;
}

function createEditorState() {
  return EditorState.create({
    doc: props.content,
    extensions: [
      basicSetup,
      closeBrackets(),
      autocompletion(),
      bracketMatching(),
      themeCompartment.of(editorThemeExtension(props.resolvedTheme)),
      editorChromeTheme,
      highlightSelectionMatches(),
      languageCompartment.of([]),
      phrasesCompartment.of(searchPhrasesExtension()),
      readonlyCompartment.of(readonlyExtension(props.readonly || props.loading)),
      featureCompartment.of(featureExtensions()),
      keymap.of([
        { key: "Mod-z", preventDefault: true, run: undo },
        { key: "Mod-Shift-z", preventDefault: true, run: redo },
        { key: "Mod-y", preventDefault: true, run: redo },
        ...searchKeymap,
        {
          key: "Mod-s",
          preventDefault: true,
          run() {
            emit("save");
            return true;
          },
        },
        {
          key: "Mod-Shift-f",
          preventDefault: true,
          run() {
            if (!props.formattingEnabled) return false;
            void formatContent();
            return true;
          },
        },
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          lastEmittedContent = update.state.doc.toString();
          emit("update:content", lastEmittedContent);
        }
        if (update.docChanged || update.selectionSet) {
          syncStatus(update.state);
        }
      }),
    ],
  });
}

function reconfigureEditorTheme(theme) {
  if (!view) return;
  const scrollDom = view.scrollDOM;
  const scroll = {
    left: scrollDom.scrollLeft,
    top: scrollDom.scrollTop,
  };
  view.dispatch({
    effects: themeCompartment.reconfigure(editorThemeExtension(theme)),
  });
  requestAnimationFrame(() => {
    scrollDom.scrollLeft = scroll.left;
    scrollDom.scrollTop = scroll.top;
  });
}

function replaceEditorContent(content) {
  if (!view) return;
  const current = view.state.doc.toString();
  if (content === current) return;
  view.dispatch({
    changes: { from: 0, to: current.length, insert: content },
  });
}

onMounted(() => {
  view = new EditorView({
    parent: editorRoot.value,
    state: createEditorState(),
  });
  syncStatus(view.state);
  configureLanguage(props.path);
});

watch(
  () => props.content,
  (content) => {
    if (content === lastEmittedContent) return;
    replaceEditorContent(content);
  },
);

watch(
  () => [props.readonly, props.loading],
  () => {
    if (!view) return;
    view.dispatch({
      effects: readonlyCompartment.reconfigure(readonlyExtension(props.readonly || props.loading)),
    });
  },
);

watch(
  () => [props.tabSize, props.lineWrapping, props.highlightCurrentLine],
  () => {
    if (!view) return;
    view.dispatch({
      effects: featureCompartment.reconfigure(featureExtensions()),
    });
  },
);

watch(
  () => props.resolvedTheme,
  (theme) => reconfigureEditorTheme(theme),
  { flush: "post" },
);

watch(
  () => locale.value,
  () => {
    if (!view) return;
    view.dispatch({
      effects: phrasesCompartment.reconfigure(searchPhrasesExtension()),
    });
  },
);

watch(
  () => props.path,
  (path) => {
    if (!view) return;
    // 切换文件必须整体重建状态：仅替换文档会让撤销历史跨文件串扰。
    view.setState(createEditorState());
    syncStatus(view.state);
    configureLanguage(path);
  },
);

onBeforeUnmount(() => {
  view?.destroy();
  view = null;
});
</script>

<template>
  <section
    class="sftp-editor"
    :style="{
      '--sftp-editor-font-family': cssFontFamily(fontFamily),
      '--sftp-editor-font-size': editorFontSize(),
    }"
  >
    <header class="sftp-editor-toolbar">
      <button
        type="button"
        class="sftp-editor-back"
        @click="$emit('back')"
      >
        <ArrowLeft
          :size="16"
          stroke-width="2"
        />
        <span>{{ backLabel }}</span>
      </button>
      <div class="sftp-editor-title">
        <span class="sftp-editor-title-text">{{ title }}</span>
        <span class="sftp-editor-path">{{ path }}</span>
      </div>
      <div class="sftp-editor-actions">
        <button
          v-if="formattingEnabled"
          type="button"
          class="sftp-button"
          :disabled="loading || saving || formatting || readonly"
          @click="formatContent"
        >
          {{ formatting ? `${formatLabel}...` : formatLabel }}
        </button>
        <button
          type="button"
          class="sftp-button"
          :disabled="loading || saving || readonly"
          @click="$emit('saveAndBack')"
        >
          {{ saving ? `${saveLabel}...` : saveLabel }}
        </button>
      </div>
    </header>
    <div
      v-if="error"
      class="sftp-editor-error"
    >
      {{ error }}
    </div>
    <div
      v-if="loading"
      class="sftp-editor-loading"
    >
      {{ loadingLabel }}
    </div>
    <div
      ref="editorRoot"
      class="sftp-editor-root"
      @wheel="handleEditorWheel"
    />
    <footer class="sftp-editor-statusbar">
      <div class="sftp-editor-status-group">
        <span
          class="sftp-editor-status-item"
          :class="{ 'is-dirty': dirty }"
        >
          <span
            v-if="dirty"
            class="sftp-editor-status-dot"
            aria-hidden="true"
          />
          {{ dirty ? t("sftp.editor.status.unsaved") : t("sftp.editor.status.saved") }}
        </span>
      </div>
      <div class="sftp-editor-status-group sftp-editor-status-meta">
        <span class="sftp-editor-status-item">
          {{ t("sftp.editor.status.position", { line: cursorLine, col: cursorCol }) }}
        </span>
        <span
          v-if="selectionLength"
          class="sftp-editor-status-item"
        >
          {{ t("sftp.editor.status.selected", { count: selectionLength }) }}
        </span>
        <span class="sftp-editor-status-item">
          {{ languageName || t("sftp.editor.status.plainText") }}
        </span>
        <span class="sftp-editor-status-item">
          {{ t("sftp.editor.status.indent", { size: tabSize }) }}
        </span>
        <span class="sftp-editor-status-item">
          {{ t("sftp.editor.status.lines", { count: lineCount }) }}
        </span>
      </div>
    </footer>
  </section>
</template>
