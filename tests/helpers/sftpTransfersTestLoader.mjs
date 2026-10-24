const DEDICATED_MOCK = "./sftpTransfersTestMocks.mjs";

function mockUrl(specifier) {
  if (specifier.endsWith("/services/sftp") || specifier.endsWith("/services/sftp.js")) {
    return "./mockSftpService.mjs";
  }
  if (
    specifier.endsWith("/events/terminalEventBus") ||
    specifier.endsWith("/events/terminalEventBus.js") ||
    specifier.endsWith("/utils/schedulers") ||
    specifier.endsWith("/utils/schedulers.js") ||
    specifier.endsWith("/useToasts") ||
    specifier.endsWith("/useToasts.js")
  ) {
    return DEDICATED_MOCK;
  }
  return null;
}

export async function resolve(specifier, context, nextResolve) {
  const replacement = mockUrl(specifier);
  if (replacement) {
    return {
      url: new globalThis.URL(replacement, import.meta.url).href,
      shortCircuit: true,
    };
  }
  try {
    return await nextResolve(specifier, context);
  } catch (error) {
    if (error?.code === "ERR_MODULE_NOT_FOUND" && !/\.[a-z0-9]+$/i.test(specifier)) {
      return nextResolve(`${specifier}.js`, context);
    }
    throw error;
  }
}
