// 测试用 loader：把 src/services/sftp 重定向到 mockSftpService 桩，
// 并为 Vite 风格的无扩展名相对导入补 .js（node ESM 需要完整扩展名）。
export async function resolve(specifier, context, nextResolve) {
  if (specifier.endsWith("/services/sftp") || specifier.endsWith("/services/sftp.js")) {
    return {
      url: new globalThis.URL("./mockSftpService.mjs", import.meta.url).href,
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
