import { ref } from "vue";

// 脚本交互弹窗：脚本里的 input/confirm/alert 都变成 Promise，
// 由 AppShell 挂载的 ScriptPromptDialog 解决。多个脚本并发弹窗时按 FIFO 排队。
export const scriptPrompt = ref(null);

const pendingPrompts = [];
let activePrompt = null;
let nextPromptId = 0;

function showNextPrompt() {
  if (activePrompt || !pendingPrompts.length) return;
  activePrompt = pendingPrompts.shift();
  const { runId: _runId, ...publicRequest } = activePrompt.request;
  scriptPrompt.value = {
    ...publicRequest,
    requestId: activePrompt.requestId,
  };
}

export function requestScriptPrompt(request) {
  return new Promise((resolve) => {
    pendingPrompts.push({
      request: { ...request },
      requestId: ++nextPromptId,
      resolve,
    });
    showNextPrompt();
  });
}

export function resolveScriptPrompt(value, requestId = scriptPrompt.value?.requestId) {
  if (!activePrompt || activePrompt.requestId !== requestId) return false;
  const prompt = activePrompt;
  activePrompt = null;
  scriptPrompt.value = null;
  prompt?.resolve(value);
  showNextPrompt();
  return true;
}

export function cancelScriptPrompts(runId) {
  if (!runId) return 0;
  let cancelled = 0;
  const retained = [];
  for (const prompt of pendingPrompts) {
    if (prompt.request.runId === runId) {
      cancelled += 1;
      prompt.resolve(null);
    } else {
      retained.push(prompt);
    }
  }
  pendingPrompts.splice(0, pendingPrompts.length, ...retained);

  if (activePrompt?.request.runId === runId) {
    cancelled += 1;
    resolveScriptPrompt(null, activePrompt.requestId);
  }
  return cancelled;
}
