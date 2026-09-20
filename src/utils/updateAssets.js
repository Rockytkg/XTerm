// 按当前平台/架构从 GitHub Release 资产中挑选安装包下载地址。
// 命名约定见 .github/workflows/build-release.yml：Windows 产出 x64/arm64 的
// .msi 与 .exe（默认下载 NSIS .exe 安装包），macOS 产出 .dmg 与
// darwin-<arch>.zip，Linux 产出 .AppImage/.deb/.rpm。匹配不到时回退发布页地址。
// dev 快照的 prerelease 资产命名与正式版相同；debug 包仅作 workflow
// artifacts、不进 Release，因此不会被更新下载选中。

const ARCH_TOKENS = {
  x86_64: ["x64", "x86_64", "amd64"],
  aarch64: ["arm64", "aarch64"],
};

const PLATFORM_EXTENSIONS = {
  windows: [".exe", ".msi"],
  macos: [".dmg", ".zip"],
  linux: [".appimage", ".deb", ".rpm"],
};

export function pickUpdateAssetUrl(status) {
  const releaseUrl = status?.releaseUrl || "";
  const assets = Array.isArray(status?.assets) ? status.assets : [];
  if (!assets.length) return releaseUrl;

  const archTokens = ARCH_TOKENS[status?.arch] || [];
  const archMatched = assets.filter((asset) => {
    const name = (asset.name || "").toLowerCase();
    return archTokens.some((token) => name.includes(token));
  });
  const pool = archMatched.length ? archMatched : assets;

  const extensions = PLATFORM_EXTENSIONS[status?.platform] || [];
  for (const ext of extensions) {
    const hit = pool.find((asset) => (asset.name || "").toLowerCase().endsWith(ext));
    if (hit?.downloadUrl) return hit.downloadUrl;
  }
  return releaseUrl;
}
