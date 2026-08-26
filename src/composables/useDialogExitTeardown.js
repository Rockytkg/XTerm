import { getCurrentInstance, onBeforeUnmount } from "vue";

// reka-ui 对话框的退出动画由 --motion-duration-quick(70ms) 驱动，Presence 会等
// 动画结束才卸载内容。若关闭瞬间就清空驱动渲染的数据，弹壳会在退出途中被
// 重渲染（表单切换、高度与文案突变）。调用方应立即关闭弹窗、但把数据清理
// 延迟到退出动画之后。
export const DIALOG_EXIT_MS = 120;

// 管理一次"关闭后清理"的调度：schedule 后若在动画期间重新打开，cancel 掉
// 挂起的清理即可让内容保持稳定；组件卸载时自动丢弃未执行的清理。
export function useDialogExitTeardown() {
  let timer = 0;

  function cancelExitTeardown() {
    if (!timer) return;
    window.clearTimeout(timer);
    timer = 0;
  }

  function scheduleExitTeardown(teardown) {
    cancelExitTeardown();
    timer = window.setTimeout(() => {
      timer = 0;
      teardown();
    }, DIALOG_EXIT_MS);
  }

  if (getCurrentInstance()) onBeforeUnmount(cancelExitTeardown);
  return { scheduleExitTeardown, cancelExitTeardown };
}
