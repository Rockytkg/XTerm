export async function resolve(specifier, context, nextResolve) {
  if (
    specifier.endsWith("/ipc/serviceRunner") ||
    specifier.endsWith("/services/ipc/serviceRunner")
  ) {
    return {
      url: new globalThis.URL("./mockServiceRunner.mjs", import.meta.url).href,
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
