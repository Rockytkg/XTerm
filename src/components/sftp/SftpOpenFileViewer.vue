<script setup>
// 预览库（含 pdfjs）体积较大：本组件由 SftpOpenFileView 经 defineAsyncComponent 懒加载，
// 插件、样式与 pdf worker 资源随该 chunk 分包，不进入主 bundle。
import { onBeforeUnmount, onMounted } from "vue";
import { OpenFileViewer } from "@open-file-viewer/vue";
import {
  archivePlugin,
  audioPlugin,
  cadPlugin,
  drawingPlugin,
  emailPlugin,
  fallbackPlugin,
  imagePlugin,
  model3dPlugin,
  officePlugin,
  pdfPlugin,
  textPlugin,
  videoPlugin,
  xmindPlugin,
} from "@open-file-viewer/core";
import "@open-file-viewer/core/style.css";
import pdfWorkerSrc from "pdfjs-dist/build/pdf.worker.mjs?url";
import { bytesToBase64 } from "../../utils/filePreview";
import { createLogger } from "../../utils/logger";

const logger = createLogger("frontend.sftp.preview");

const props = defineProps({
  file: { type: Blob, required: true },
  fileName: { type: String, required: true },
  locale: { type: String, default: "zh-CN" },
  // 后端嗅探 mime 提示（魔数命中的具体类型优先于扩展名映射）；为空时不传，交给预览库按文件名匹配
  mimeType: { type: String, default: undefined },
  // 库内"下载当前文件"链接（各插件失败回退、文本下载按钮等）的回调
  onDownload: { type: Function, default: null },
  // 库内子资源下载（如邮件附件）的回调：{ name, contentBase64 }
  onDownloadResource: { type: Function, default: null },
  theme: { type: String, default: "light" },
});

// 插件按顺序匹配，fallbackPlugin 必须最后；cad/model3d 未装高保真引擎时退化为元信息预览。
// 不含 gisPlugin：它的样式来自 jsdelivr、瓦片来自 OpenStreetMap，CSP 与离线环境下必然不可用。
const plugins = [
  imagePlugin(),
  videoPlugin(),
  audioPlugin(),
  textPlugin(),
  // cMap/标准字体用本地打包资源（vite.config.js 的 syncPdfjsAssets）；默认的 CDN 地址被 CSP 拦截
  pdfPlugin({
    workerSrc: pdfWorkerSrc,
    cMapUrl: "/pdfjs/cmaps/",
    cMapPacked: true,
    standardFontDataUrl: "/pdfjs/standard_fonts/",
  }),
  officePlugin(),
  archivePlugin(),
  emailPlugin(),
  drawingPlugin(),
  xmindPlugin(),
  cadPlugin(),
  model3dPlugin(),
  fallbackPlugin(),
];

// Tauri webview 中浏览器 blob 下载与打印不可靠，下载走已有的 SFTP 传输按钮
const toolbar = {
  zoom: true,
  rotate: true,
  fullscreen: true,
  search: true,
  download: false,
  print: false,
};

// 预览库所有下载入口（各插件失败回退的 <a href="blob:..." download>、文本插件"下载"按钮
// 的程序化 link.click()——该 link 挂在 document.body 上、不经过本组件 DOM）在 Tauri
// webview 中都会无提示写入系统下载目录。组件存活期间在 document 捕获阶段统一拦截：
// download 名与当前文件一致的走 onDownload（后端 SFTP 传输），其余是预览内部子资源
// （如邮件附件），取 blob 字节经 onDownloadResource 交后端保存对话框落盘。
async function routeDownload(anchor) {
  const name = anchor.getAttribute("download");
  if (!name || name === props.fileName) {
    props.onDownload?.();
    return;
  }
  if (!props.onDownloadResource) return;
  try {
    const bytes = new Uint8Array(await (await fetch(anchor.href)).arrayBuffer());
    props.onDownloadResource({ name, contentBase64: bytesToBase64(bytes) });
  } catch (error) {
    logger.warn("preview.resource.read_failed", error);
  }
}

function handleDownloadClick(event) {
  if (!(event.target instanceof Element)) return;
  const anchor = event.target.closest('a[download][href^="blob:"]');
  if (!anchor) return;
  event.preventDefault();
  event.stopPropagation();
  void routeDownload(anchor);
}

onMounted(() => {
  document.addEventListener("click", handleDownloadClick, true);
});

onBeforeUnmount(() => {
  document.removeEventListener("click", handleDownloadClick, true);
});
</script>

<template>
  <div class="sftp-preview-viewer">
    <OpenFileViewer
      :file="file"
      :file-name="fileName"
      width="100%"
      height="100%"
      fit="contain"
      :toolbar="toolbar"
      :theme="theme"
      :locale="locale"
      :mime-type="mimeType"
      :plugins="plugins"
    />
  </div>
</template>
