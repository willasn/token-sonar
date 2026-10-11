import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

// Vite 以 ui/ 为根：ui/index.html 是入口，产物落到 ui/dist（Tauri 的 frontendDist）。
// 开发端口固定 5173，与 src-tauri/tauri.conf.json 的 devUrl 保持一致。
export default defineConfig({
  root: "ui",
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./ui/src", import.meta.url)),
    },
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
