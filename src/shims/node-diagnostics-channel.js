// node:diagnostics_channel 的浏览器 shim。@xterm/addon-ligatures 的依赖链
// （font-ligatures → lru-cache）静态导入该 Node 内建模块做指标采集；WebView2
// 中不存在它，打包器以空模块兜底，导致 addon 激活时 channel() 调用抛
// TypeError、连字在发布构建中整体失效。提供无订阅者的最小实现让指标路径短路。
const noopChannel = {
  hasSubscribers: false,
  publish() {},
  subscribe() {},
  unsubscribe() {},
};

export function channel(_name) {
  return noopChannel;
}

export function tracingChannel(_name) {
  return {
    ...noopChannel,
    start: noopChannel,
    end: noopChannel,
    asyncStart: noopChannel,
    asyncEnd: noopChannel,
    error: noopChannel,
    traceSync(fn, context, ...args) {
      return fn.apply(context, args);
    },
    tracePromise(fn, context, ...args) {
      return fn.apply(context, args);
    },
  };
}

export default { channel, tracingChannel };
