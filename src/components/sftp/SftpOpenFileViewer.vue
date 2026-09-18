<script setup>
// 预览库（含 pdfjs）体积较大：本组件由 SftpOpenFileView 经 defineAsyncComponent 懒加载，
// 插件、样式与 pdf worker 资源随该 chunk 分包，不进入主 bundle。
import { OpenFileViewer } from "@open-file-viewer/vue";
import {
  archivePlugin,
  audioPlugin,
  cadPlugin,
  drawingPlugin,
  emailPlugin,
  fallbackPlugin,
  gisPlugin,
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

defineProps({
  file: { type: Blob, required: true },
  fileName: { type: String, required: true },
  locale: { type: String, default: "zh-CN" },
  // 无扩展名文件的后端嗅探 mime 提示；为空时不传，交给预览库按文件名匹配
  mimeType: { type: String, default: undefined },
  theme: { type: String, default: "light" },
});

// 插件按顺序匹配，fallbackPlugin 必须最后；cad/model3d/gis 未装高保真引擎时退化为元信息预览
const plugins = [
  imagePlugin(),
  videoPlugin(),
  audioPlugin(),
  textPlugin(),
  pdfPlugin({ workerSrc: pdfWorkerSrc }),
  officePlugin(),
  archivePlugin(),
  emailPlugin(),
  drawingPlugin(),
  xmindPlugin(),
  cadPlugin(),
  model3dPlugin(),
  gisPlugin(),
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
</script>

<template>
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
    class="sftp-preview-viewer"
  />
</template>
