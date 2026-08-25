import { onBeforeUnmount, ref } from "vue";
import { choosePrivateKey } from "../services/credentials";

// The native file picker steals focus, which reka-ui treats as an outside
// interaction that would close the surrounding dialog. While a pick is in
// flight (and briefly after, until focus settles) callers suppress dialog
// dismissal via keepDialogOpen.
export function usePrivateKeyPicker() {
  const pickerOpen = ref(false);
  let resetTimer = null;

  onBeforeUnmount(() => {
    if (resetTimer) {
      window.clearTimeout(resetTimer);
      resetTimer = null;
    }
  });

  // Returns the key content, or null when cancelled/already picking.
  async function pickPrivateKey(title) {
    if (pickerOpen.value) return null;
    if (resetTimer) {
      window.clearTimeout(resetTimer);
      resetTimer = null;
    }
    pickerOpen.value = true;
    try {
      return await choosePrivateKey(title);
    } finally {
      resetTimer = window.setTimeout(() => {
        pickerOpen.value = false;
        resetTimer = null;
      }, 150);
    }
  }

  function keepDialogOpen(event) {
    if (pickerOpen.value) {
      event.preventDefault();
    }
  }

  return { pickerOpen, pickPrivateKey, keepDialogOpen };
}
