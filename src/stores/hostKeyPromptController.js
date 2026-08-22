import { ref } from "vue";

export function createHostKeyPromptController() {
  const hostKeyPrompt = ref(null);

  function setPrompt(prompt) {
    if (!prompt?.connectionId || !prompt?.sessionId) return false;
    hostKeyPrompt.value = { ...prompt };
    return true;
  }

  function takePrompt() {
    const prompt = hostKeyPrompt.value;
    hostKeyPrompt.value = null;
    return prompt;
  }

  function answerPrompt(mode) {
    const prompt = hostKeyPrompt.value;
    if (!prompt) return null;
    return { prompt: takePrompt(), mode: ["save", "once"].includes(mode) ? mode : "cancel" };
  }

  function cancelPrompt() {
    hostKeyPrompt.value = null;
  }

  function cancelPromptForConnection(connectionId, sessionId = "") {
    if (hostKeyPrompt.value?.connectionId !== connectionId) return false;
    if (sessionId && hostKeyPrompt.value?.sessionId !== sessionId) return false;
    cancelPrompt();
    return true;
  }

  return {
    answerPrompt,
    cancelPromptForConnection,
    hostKeyPrompt,
    setPrompt,
  };
}
