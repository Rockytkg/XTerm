import { i18n } from "../../i18n/index.js";
import { cancelScriptPrompts, requestScriptPrompt } from "./scriptPromptController.js";
import { getRecordingBridge, getScriptBridge } from "./bridges.js";
import { readDataFile, writeDataFile } from "./scriptFileLoader.js";

const MAX_OUTPUT_BUFFER_CHARS = 128 * 1024;
// searchBuffer 最多带回的匹配行数（count 仍为总数），避免巨型缓冲区撑爆运行记录。
const MAX_BUFFER_SEARCH_MATCHES = 100;
const DEFAULT_WAIT_TIMEOUT_MS = 10000;

const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);
const RE_CSI = new RegExp(`${ESC}\\[[0-?]*[ -/]*[@-~]`, "g");
const RE_OSC = new RegExp(`${ESC}\\][^${BEL}]*(?:${BEL}|${ESC}\\\\)`, "g");
const RE_ESC = new RegExp(`${ESC}[@-Z\\\\-_]`, "g");

// 脚本匹配的是“可读文本”：剥离 CSI/OSC/ESC 控制序列，避免 \x1b[K 之类序列
// 把用户等待的提示符拆碎（例如 "#" 被渲染成 "\x1b[1m#\x1b[0m"）。
export function stripAnsi(data) {
  return String(data || "")
    .replace(RE_CSI, "")
    .replace(RE_OSC, "")
    .replace(RE_ESC, "");
}

export class ScriptStoppedError extends Error {
  constructor() {
    super("script stopped");
    this.name = "ScriptStoppedError";
  }
}

function normalizeRegExp(pattern) {
  // 去掉 g/y，避免 lastIndex 在多次检查间漂移导致漏匹配。
  return new RegExp(pattern.source, pattern.flags.replaceAll(/[gy]/g, ""));
}

function escapeRegExp(text) {
  return String(text).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function matchPattern(buffer, pattern, apiName = "waitFor") {
  if (typeof pattern === "string") {
    const index = buffer.indexOf(pattern);
    return index < 0 ? null : { index, text: pattern, groups: [] };
  }
  if (pattern instanceof RegExp) {
    const match = normalizeRegExp(pattern).exec(buffer);
    return match ? { index: match.index, text: match[0], groups: match.slice(1) } : null;
  }
  throw new TypeError(`${apiName} pattern must be a string or RegExp`);
}

// 多模式竞争：命中位置最早者胜出；同位置时排在前面的模式优先。
function matchEarliest(buffer, patterns, apiName) {
  let best = null;
  patterns.forEach((pattern, patternIndex) => {
    const match = matchPattern(buffer, pattern, apiName);
    if (match && (!best || match.index < best.index)) {
      best = { ...match, patternIndex };
    }
  });
  return best;
}

// 命名按键 → 终端控制序列：菜单型设备与中断场景（Ctrl+C）常用；
// 未知名称直接抛错，避免静默发送错误序列。
const KEY_SEQUENCES = {
  enter: "\r",
  tab: "\t",
  esc: ESC,
  escape: ESC,
  backspace: "\x7f",
  space: " ",
  up: `${ESC}[A`,
  down: `${ESC}[B`,
  right: `${ESC}[C`,
  left: `${ESC}[D`,
  home: `${ESC}[H`,
  end: `${ESC}[F`,
  pageup: `${ESC}[5~`,
  pagedown: `${ESC}[6~`,
  insert: `${ESC}[2~`,
  delete: `${ESC}[3~`,
  f1: `${ESC}OP`,
  f2: `${ESC}OQ`,
  f3: `${ESC}OR`,
  f4: `${ESC}OS`,
  f5: `${ESC}[15~`,
  f6: `${ESC}[17~`,
  f7: `${ESC}[18~`,
  f8: `${ESC}[19~`,
  f9: `${ESC}[20~`,
  f10: `${ESC}[21~`,
  f11: `${ESC}[23~`,
  f12: `${ESC}[24~`,
};

function keySequence(key) {
  const normalized = String(key ?? "")
    .trim()
    .toLowerCase();
  const ctrl = /^ctrl[-+]([a-z])$/.exec(normalized);
  if (ctrl) return String.fromCharCode(ctrl[1].charCodeAt(0) - 96);
  if (Object.hasOwn(KEY_SEQUENCES, normalized)) return KEY_SEQUENCES[normalized];
  throw new TypeError(`press key must be a named key or "ctrl+<letter>", got: ${String(key)}`);
}

function normalizePromptArgs(messageOrOptions, options) {
  if (typeof messageOrOptions === "object" && messageOrOptions !== null) {
    return { ...messageOrOptions };
  }
  return { ...(options || {}), message: String(messageOrOptions ?? "") };
}

// 数据文件对话框文案由前端按当前语言提供；options.title 可覆盖默认标题。
function dataFileLabels(titleKey, titleOverride) {
  return {
    title: titleOverride || i18n.global.t(titleKey),
    textFilesLabel: i18n.global.t("scripts.fileDialog.textFiles"),
    allFilesLabel: i18n.global.t("scripts.fileDialog.allFiles"),
  };
}

/**
 * 构建脚本可见的 xterm.* API。只依赖注入的 run/context/lifecycle/trackTask/log，
 * 不感知运行记录的存储方式（由 scriptRunner 持有）。
 */
export function createScriptApi({ run, context, lifecycle, trackTask, log }) {
  let outputBuffer = "";
  // pending 里的每一项：{ kind: "wait"|"sleep"|"read", resolve, reject, timer, ... }
  const pending = new Set();

  function failPending(error) {
    for (const entry of [...pending]) {
      clearTimeout(entry.timer);
      entry.reject(error);
    }
    pending.clear();
  }

  lifecycle.failPending = () => failPending(new ScriptStoppedError());
  lifecycle.cleanups.push(lifecycle.failPending);
  lifecycle.cleanups.push(() => cancelScriptPrompts(run.runId));

  function throwIfStopped() {
    if (run.aborted) throw new ScriptStoppedError();
  }

  // 统一注册挂起项：到期先摘除再回调；timeoutMs 传 null（或非有限负数）表示永不超时。
  function addPending(entry, timeoutMs, onTimeout) {
    const timeout = timeoutMs === null ? Number.NaN : Number(timeoutMs);
    if (Number.isFinite(timeout) && timeout >= 0) {
      entry.timer = setTimeout(() => {
        pending.delete(entry);
        onTimeout();
      }, timeout);
    }
    pending.add(entry);
  }

  function appendOutput(rawData) {
    const text = stripAnsi(rawData);
    if (!text) return;
    outputBuffer += text;
    if (outputBuffer.length > MAX_OUTPUT_BUFFER_CHARS) {
      outputBuffer = outputBuffer.slice(-MAX_OUTPUT_BUFFER_CHARS / 2);
    }
    for (const entry of [...pending]) {
      if (entry.kind === "wait") checkWaiter(entry);
      if (entry.kind === "read") entry.chunks.push(text);
    }
  }

  function handleOutput(rawData) {
    try {
      appendOutput(rawData);
    } catch (error) {
      lifecycle.failRuntime?.(error);
    }
  }

  const bridge = getScriptBridge(context.targetSessionId);
  lifecycle.cleanups.push(bridge.onOutput(handleOutput));

  function checkWaiter(entry) {
    const match = entry.patterns
      ? matchEarliest(outputBuffer, entry.patterns, entry.apiName)
      : matchPattern(outputBuffer, entry.pattern, entry.apiName);
    if (!match) return;
    clearTimeout(entry.timer);
    pending.delete(entry);
    // before：自上次消费点到本次匹配起点之间收到的输出，expect 系列把它带回给脚本；
    // 随后连同匹配文本一起消费，后续等待不会重复命中同一段输出。
    const before = outputBuffer.slice(0, match.index);
    outputBuffer = outputBuffer.slice(match.index + match.text.length);
    entry.resolve(entry.mapMatch(match, before));
  }

  // 统一的“等待输出匹配”实现：挂起/超时/消费语义只有一套，
  // waitFor/expect/expectAny 只是结果映射（mapMatch）不同。
  function waitOnOutput(entryBase, timeoutMs = DEFAULT_WAIT_TIMEOUT_MS, message = "") {
    throwIfStopped();
    return new Promise((resolve, reject) => {
      const entry = { kind: "wait", resolve, reject, timer: null, ...entryBase };
      const timeout = Number(timeoutMs);
      // timeout<=0 表示永不超时；自定义错误信息优先，缺省走 i18n。
      addPending(entry, timeout > 0 ? timeout : null, () =>
        reject(
          new Error(
            message ||
              i18n.global.t("scripts.errors.waitTimeout", {
                timeout,
                pattern: entryBase.label,
              }),
          ),
        ),
      );
      try {
        checkWaiter(entry);
      } catch (error) {
        clearTimeout(entry.timer);
        pending.delete(entry);
        reject(error);
      }
    });
  }

  function waitFor(pattern, timeoutMs, message) {
    throwIfStopped();
    // 先校验类型，避免把 TypeError 延迟到异步路径。
    if (typeof pattern !== "string" && !(pattern instanceof RegExp)) {
      return Promise.reject(new TypeError("waitFor pattern must be a string or RegExp"));
    }
    return waitOnOutput(
      { apiName: "waitFor", pattern, label: String(pattern), mapMatch: (match) => match.text },
      timeoutMs,
      message,
    );
  }

  // expect：与 waitFor 相同的等待/超时/消费语义，但返回结构化结果
  // { text, groups, before }——groups 为正则捕获组，before 为匹配前收到的输出，
  // 让“发命令 → 等到提示符 → 解析中间输出”一条链路完成，无需事后翻缓冲区。
  function expect(pattern, timeoutMs, message) {
    throwIfStopped();
    if (typeof pattern !== "string" && !(pattern instanceof RegExp)) {
      return Promise.reject(new TypeError("expect pattern must be a string or RegExp"));
    }
    return waitOnOutput(
      {
        apiName: "expect",
        pattern,
        label: String(pattern),
        mapMatch: (match, before) => ({ text: match.text, groups: match.groups, before }),
      },
      timeoutMs,
      message,
    );
  }

  // expectAny：等待多个模式中的任意一个，返回 { index, text, groups, before }；
  // index 指明命中第几个模式，用于区分错误输出与正常提示符等分支。
  function expectAny(patterns, timeoutMs, message) {
    throwIfStopped();
    const list = Array.isArray(patterns) ? patterns : [patterns];
    if (!list.length || list.some((p) => typeof p !== "string" && !(p instanceof RegExp))) {
      return Promise.reject(new TypeError("expectAny patterns must be strings or RegExps"));
    }
    return waitOnOutput(
      {
        apiName: "expectAny",
        patterns: list,
        label: list.map((pattern) => String(pattern)).join(" | "),
        mapMatch: (match, before) => ({
          index: match.patternIndex,
          text: match.text,
          groups: match.groups,
          before,
        }),
      },
      timeoutMs,
      message,
    );
  }

  function sleep(ms) {
    throwIfStopped();
    return new Promise((resolve, reject) => {
      const entry = { kind: "sleep", resolve, reject, timer: null };
      addPending(entry, Number(ms) || 0, resolve);
    });
  }

  function read(timeoutMs = 1000) {
    throwIfStopped();
    return new Promise((resolve, reject) => {
      const entry = { kind: "read", chunks: [], resolve, reject, timer: null };
      addPending(entry, Number(timeoutMs) || 0, () => resolve(entry.chunks.join("")));
    });
  }

  async function send(data) {
    throwIfStopped();
    // 经 ScriptBridgeAddon 走 xterm input，等同人工输入：前端回显 + 正常链路发往后端。
    if (!bridge.send(String(data ?? ""))) throw new Error("target session is not available");
  }

  function getScreen() {
    throwIfStopped();
    return bridge.getScreenText();
  }

  // 读取整个终端缓冲区（滚动回退 + 当前屏幕）的既有文本，只读、无 UI 副作用。
  function getBuffer() {
    throwIfStopped();
    return bridge.getBufferText();
  }

  // 在缓冲区既有内容中逐行检索；字符串按子串匹配，RegExp 按行 test。
  // 返回 { count, matches: [{ line, text }] }，line 为 1 起始的全缓冲区行号，
  // matches 截断到 MAX_BUFFER_SEARCH_MATCHES 条，count 始终是命中总行数。
  function searchBuffer(pattern) {
    throwIfStopped();
    if (typeof pattern !== "string" && !(pattern instanceof RegExp)) {
      throw new TypeError("searchBuffer pattern must be a string or RegExp");
    }
    const regex = pattern instanceof RegExp ? normalizeRegExp(pattern) : null;
    const matches = [];
    let count = 0;
    bridge
      .getBufferText()
      .split("\n")
      .forEach((text, index) => {
        const hit = regex ? regex.test(text) : text.includes(pattern);
        if (!hit) return;
        count += 1;
        if (matches.length < MAX_BUFFER_SEARCH_MATCHES) {
          matches.push({ line: index + 1, text });
        }
      });
    return { count, matches };
  }

  function requireRecordingBridge() {
    const recording = getRecordingBridge();
    if (!recording) throw new Error("session recording is not available");
    return recording;
  }

  // 开启会话记录：写入路径由用户在原生保存对话框中选定，脚本无法指定。
  async function startRecording() {
    throwIfStopped();
    const recording = requireRecordingBridge();
    const path = await recording.start(context.targetSessionId);
    // 对话框悬停期间脚本被停止：回滚刚开始的记录，避免遗留孤儿记录。
    if (run.aborted) {
      if (path) await recording.stop(context.targetSessionId).catch(() => {});
      throw new ScriptStoppedError();
    }
    // 与 input/form/文件对话框一致：用户取消保存对话框 = 取消脚本执行。
    if (!path) throw new ScriptStoppedError();
    return path;
  }

  // 停止会话记录：冲刷并收尾文件，返回写入路径；本就没有记录时返回空串。
  async function stopRecording() {
    throwIfStopped();
    const path = await requireRecordingBridge().stop(context.targetSessionId);
    throwIfStopped();
    return path;
  }

  async function prompt(kind, messageOrOptions, options) {
    throwIfStopped();
    const value = await requestScriptPrompt({
      kind,
      runId: run.runId,
      scriptName: run.scriptName,
      ...normalizePromptArgs(messageOrOptions, options),
    });
    // 取消输入/表单视为取消脚本执行；confirm 的取消是否定回答（false），不终止。
    if (value === null && kind !== "confirm") throw new ScriptStoppedError();
    throwIfStopped();
    return value;
  }

  // 需要用户在原生对话框中授权的操作统一入口：前后检查停止状态；
  // 与 input/form 弹窗一致——用户取消对话框 = 取消脚本执行。
  async function withUserConsent(operation) {
    throwIfStopped();
    const result = await operation();
    throwIfStopped();
    if (!result) throw new ScriptStoppedError();
    return result;
  }

  // 读取本地数据文件：路径由用户在原生对话框中选定，脚本只拿到文本内容。
  async function readData(options = {}) {
    const result = await withUserConsent(() =>
      readDataFile(dataFileLabels("scripts.fileDialog.readDataTitle", options?.title)),
    );
    return result.content;
  }

  // 保存数据到本地文件：脚本只提供建议文件名与文本内容，写入路径由用户决定。
  async function saveData(data, options = {}) {
    const fileName = String(options?.fileName || "data.txt").replace(/[\\/:*?"<>|]/g, "_");
    return withUserConsent(() =>
      writeDataFile(
        fileName,
        String(data ?? ""),
        dataFileLabels("scripts.fileDialog.saveDataTitle", options?.title),
      ),
    );
  }

  return {
    send: (data) => trackTask(send(data)),
    sendLine: (text = "") => trackTask(send(`${text}\r`)),
    waitFor: (...args) => trackTask(waitFor(...args)),
    expect: (...args) => trackTask(expect(...args)),
    expectAny: (...args) => trackTask(expectAny(...args)),
    // 命名按键（ctrl+c / enter / 方向键 / f1-f12 等）：等同人工按键发送控制序列。
    press: (key) => trackTask(send(keySequence(key))),
    waitForAny: (patterns, timeoutMs, message) =>
      trackTask(
        waitFor(
          new RegExp(
            (Array.isArray(patterns) ? patterns : [patterns])
              .map((pattern) =>
                pattern instanceof RegExp
                  ? `(?:${pattern.source})`
                  : `(?:${escapeRegExp(pattern)})`,
              )
              .join("|"),
          ),
          timeoutMs,
          message,
        ),
      ),
    read: (...args) => trackTask(read(...args)),
    getScreen,
    getBuffer,
    searchBuffer,
    // 会话记录控制：复用工作区记录管线（输入+输出、ANSI 归一化、异步冲刷落盘），
    // 记录文件路径始终经原生对话框由用户授权。
    startRecording: () => trackTask(startRecording()),
    stopRecording: () => trackTask(stopRecording()),
    isRecording: () => getRecordingBridge()?.isActive(context.targetSessionId) === true,
    sleep: (...args) => trackTask(sleep(...args)),
    input: (messageOrOptions, options) => trackTask(prompt("input", messageOrOptions, options)),
    confirm: (messageOrOptions, options) =>
      trackTask(prompt("confirm", messageOrOptions, options).then((value) => value === true)),
    alert: (messageOrOptions, options) => trackTask(prompt("alert", messageOrOptions, options)),
    // 自定义表单弹窗：一次收集多个字段；字段支持 required/type:"url" 校验，
    // 取消视为取消脚本执行（抛出停止），提交返回 { key: value }。
    form: (options) => trackTask(prompt("form", options)),
    // 本地数据文件读写：仅文本数据，路径经原生对话框由用户授权，脚本无法自由访问文件系统。
    readFile: (options) => trackTask(readData(options)),
    saveFile: (data, options) => trackTask(saveData(data, options)),
    log: (...args) => log(...args),
    // 会话元数据：仅暴露非敏感连接信息（协议/地址/端口/用户名），便于脚本
    // 按设备类型分支；密码、私钥等凭证绝不进入脚本作用域。
    session: Object.freeze({
      id: context.targetSessionId,
      label: context.targetLabel,
      protocol: String(context.sessionInfo?.protocol || ""),
      host: String(context.sessionInfo?.host || ""),
      port: context.sessionInfo?.port ?? "",
      username: String(context.sessionInfo?.username || ""),
    }),
  };
}
