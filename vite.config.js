import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import UnoCSS from "unocss/vite";

export default defineConfig({
  plugins: [UnoCSS({ inspector: false }), vue()],
  clearScreen: false,
  build: {
    sourcemap: false,
    minify: "terser",
    cssMinify: "lightningcss",
    reportCompressedSize: false,
    terserOptions: {
      compress: {
        passes: 2,
        // Vue/Vue I18n depend on reactive getters for dependency tracking.
        // Treating property reads as pure can make production builds miss
        // locale updates even though the dev server behaves correctly.
        pure_getters: false,
        drop_console: true,
        drop_debugger: true,
      },
      format: {
        comments: false,
      },
      mangle: {
        safari10: true,
      },
      module: true,
    },
    rolldownOptions: {
      checks: {
        invalidAnnotation: false,
        pluginTimings: false,
      },
      output: {
        codeSplitting: true,
        entryFileNames: "assets/[hash].js",
        chunkFileNames: "assets/[hash].js",
        assetFileNames: "assets/[hash][extname]",
        manualChunks(id) {
          const nid = id.replaceAll("\\", "/");
          // Xterm renderer — large and optional (WebGL fallback)
          if (nid.includes("/node_modules/@xterm/xterm/")) return "xterm-core";
          // Split addons further so each optional capability can cache independently.
          if (nid.includes("/node_modules/@xterm/addon-webgl/")) return "xterm-addon-webgl";
          if (nid.includes("/node_modules/@xterm/addon-ligatures/")) return "xterm-addon-ligatures";
          if (nid.includes("/node_modules/@xterm/addon-search/")) return "xterm-addon-search";
          if (nid.includes("/node_modules/@xterm/addon-web-links/")) return "xterm-addon-web-links";
          if (nid.includes("/node_modules/@xterm/addon-unicode11/")) return "xterm-addon-unicode11";
          if (nid.includes("/node_modules/@xterm/addon-image/")) return "xterm-addon-image";
          if (nid.includes("/node_modules/@xterm/addon-progress/")) return "xterm-addon-progress";
          if (nid.includes("/node_modules/@xterm/")) return "xterm-misc";
          // Heavy editors / visualization — not needed on first paint
          // Split CodeMirror into smaller cacheable bundles (CodeMirror + Lezer can be very large).
          if (nid.includes("/node_modules/@codemirror/language-data/"))
            return "codemirror-language-data";
          const codemirrorLanguagePackage = nid.match(
            /\/node_modules\/@codemirror\/(lang-[^/]+)\//,
          );
          if (codemirrorLanguagePackage) return `codemirror-${codemirrorLanguagePackage[1]}`;
          if (nid.includes("/node_modules/@lezer/css/")) return "lezer-css";
          if (nid.includes("/node_modules/@lezer/html/")) return "lezer-html";
          if (nid.includes("/node_modules/@lezer/javascript/")) return "lezer-js";
          if (nid.includes("/node_modules/@lezer/json/")) return "lezer-json";
          if (nid.includes("/node_modules/@lezer/")) return "lezer";
          if (
            nid.includes("/node_modules/@codemirror/autocomplete/") ||
            nid.includes("/node_modules/@codemirror/commands/") ||
            nid.includes("/node_modules/@codemirror/search/")
          )
            return "codemirror-addons";
          if (
            nid.includes("/node_modules/@codemirror/state/") ||
            nid.includes("/node_modules/@codemirror/view/") ||
            nid.includes("/node_modules/@codemirror/language/")
          )
            return "codemirror-core";
          if (
            nid.includes("/node_modules/@codemirror/") ||
            nid.includes("/node_modules/codemirror/")
          )
            return "codemirror-misc";
          // Prettier is loaded only when the user formats a script. Keep its
          // runtime and parser plugins separate so the on-demand formatter
          // does not become a single chunk above Vite's warning threshold.
          if (nid.includes("/node_modules/prettier/plugins/babel")) return "prettier-babel";
          if (nid.includes("/node_modules/prettier/plugins/estree")) return "prettier-estree";
          if (nid.includes("/node_modules/prettier/")) return "prettier-core";
          if (nid.includes("/node_modules/chart.js/")) return "chart";
          // Icons — stable, cache-friendly
          if (nid.includes("/node_modules/@lucide/")) return "icons";
          // UI primitives
          if (nid.includes("/node_modules/reka-ui/")) return "ui";
          if (nid.includes("/node_modules/@vueuse/")) return "vueuse";
          if (nid.includes("/node_modules/cytoscape/")) return "cytoscape";
          if (nid.includes("/node_modules/gsap/")) return "gsap";
          if (nid.includes("/node_modules/@tauri-apps/")) return "tauri-api";
          // Core framework + i18n — rarely changes
          if (nid.includes("/node_modules/vue/") || nid.includes("/node_modules/vue-router/"))
            return "vue";
          if (nid.includes("/node_modules/pinia/")) return "pinia";
          if (nid.includes("/node_modules/vue-i18n/")) return "vue-i18n";
          // Utility libs
          if (nid.includes("/node_modules/pako/")) return "pako";
          if (nid.includes("/node_modules/sortablejs/")) return "sortable";
          return undefined;
        },
      },
    },
  },
  server: {
    strictPort: true,
    host: "127.0.0.1",
    port: 1420,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
