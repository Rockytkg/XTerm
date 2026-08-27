import { onBeforeUnmount, ref } from "vue";
import { streamSystemFonts } from "../services/systemFonts";

// 系统字体按块流式到达：挂载即开始加载并去重排序，重新加载先中止旧流，
// 组件卸载时中止流，避免已销毁组件继续写入字体列表。
export function useSystemFonts(currentFontFamily) {
  const systemFonts = ref([]);
  let fontAbortController;

  function appendFonts(fonts) {
    systemFonts.value = Array.from(new Set([...systemFonts.value, ...fonts].filter(Boolean))).sort(
      (a, b) => a.localeCompare(b),
    );
  }

  async function loadFonts() {
    fontAbortController?.abort();
    const controller = new AbortController();
    fontAbortController = controller;
    const current = currentFontFamily()?.trim();
    appendFonts([current]);
    for await (const chunk of streamSystemFonts({ signal: controller.signal })) {
      if (controller.signal.aborted) return;
      appendFonts(chunk.fonts);
      if (chunk.done) return;
    }
  }

  loadFonts();
  onBeforeUnmount(() => fontAbortController?.abort());

  return { systemFonts };
}
