export function createEventBridge({ eventName, logName, logger, subscribe }) {
  const subscriptions = new Set();
  let startPromise = null;
  let stopBridge = null;

  function dispatch(payload) {
    for (const subscription of [...subscriptions]) {
      try {
        subscription.handler(payload);
      } catch (error) {
        logger.error(`${logName}.listener.failed`, error);
      }
    }
  }

  function start() {
    if (startPromise) return startPromise;
    startPromise = Promise.resolve(subscribe(eventName, (event) => dispatch(event?.payload)))
      .then((unlisten) => {
        stopBridge = typeof unlisten === "function" ? unlisten : null;
      })
      .catch((error) => {
        startPromise = null;
        throw error;
      });
    return startPromise;
  }

  function stop() {
    if (subscriptions.size > 0 || !startPromise) return;
    try {
      stopBridge?.();
    } catch (error) {
      logger.error(`${logName}.bridge.stop.failed`, error);
    }
    stopBridge = null;
    startPromise = null;
  }

  return async function observe(handler) {
    if (typeof handler !== "function") {
      throw new TypeError(`${logName} observer must be a function`);
    }

    // Register before starting the native bridge so an event emitted while the
    // subscription promise resolves cannot fall into an observation gap.
    const subscription = { handler };
    subscriptions.add(subscription);
    try {
      await start();
    } catch (error) {
      subscriptions.delete(subscription);
      stop();
      throw error;
    }

    let active = true;
    return () => {
      if (!active) return;
      active = false;
      subscriptions.delete(subscription);
      stop();
    };
  };
}
