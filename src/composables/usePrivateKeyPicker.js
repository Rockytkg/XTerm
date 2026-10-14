import { getCurrentInstance, onBeforeUnmount, ref } from "vue";
import { choosePrivateKey } from "../services/credentials";

// The native file picker steals focus, which reka-ui treats as an outside
// interaction that would close the surrounding dialog. While a pick is in
// flight (and briefly after, until focus settles) callers suppress dialog
// dismissal via keepDialogOpen.
export function usePrivateKeyPicker({ cooldownMs = 150 } = {}) {
  const pickerOpen = ref(false);
  let resetTimer = null;
  let disposed = false;

  function clearResetTimer() {
    if (resetTimer) {
      window.clearTimeout(resetTimer);
      resetTimer = null;
    }
  }

  function dispose() {
    disposed = true;
    clearResetTimer();
    pickerOpen.value = false;
  }

  // Some callers use the state machine in plain tests or utility code. Only
  // register the lifecycle hook when Vue has an owner instance; the disposed
  // flag still closes the async completion race for real components.
  if (getCurrentInstance()) onBeforeUnmount(dispose);

  // Returns the key content, or null when cancelled/already picking.
  async function pickPrivateKey(title) {
    if (pickerOpen.value) return null;
    clearResetTimer();
    pickerOpen.value = true;
    try {
      return await choosePrivateKey(title);
    } finally {
      if (!disposed) {
        resetTimer = window.setTimeout(() => {
          pickerOpen.value = false;
          resetTimer = null;
        }, cooldownMs);
      }
    }
  }

  function keepDialogOpen(event) {
    if (pickerOpen.value) {
      event.preventDefault();
    }
  }

  return { pickerOpen, pickPrivateKey, keepDialogOpen };
}
