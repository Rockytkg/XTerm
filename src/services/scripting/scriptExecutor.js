import { compileScript, createSandboxBindings } from "./scriptSyntax.js";
import { createScriptRuntime } from "./scriptRuntime.js";

// 脚本执行内核：编译用户代码、注入运行时作用域与沙盒绑定，
// 驱动到"主体返回 + 后台工作清空"为止。Worker 内执行与主线程直连执行
// （node 单测 / Worker 不可用时的回退）共用此模块，保证两条路径语义一致。
export function executeScript({ code, createApi, log, abortedState, formatBlockedMessage }) {
  const runtime = createScriptRuntime(abortedState, log);
  const api = createApi(runtime);
  const scopeNames = Object.keys(runtime.scope);
  const execute = compileScript(code, scopeNames);
  // 沙盒绑定值的位置必须与 SANDBOX_BLOCKED_GLOBALS 一一对应（compileScript 已同名追加）。
  const sandboxBindings = createSandboxBindings(formatBlockedMessage);
  const done = (async () => {
    await execute(api, ...Object.values(runtime.scope), ...sandboxBindings);
    await runtime.waitForBackgroundWork();
  })();
  return {
    done,
    failure: runtime.failure,
    dispose: runtime.dispose,
  };
}
