import { defineConfig, build } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    {
      name: "build-content-script",
      async closeBundle() {
        await build({
          configFile: false,
          build: {
            outDir: "dist",
            emptyOutDir: false,
            lib: {
              entry: resolve(__dirname, "src/content-script.ts"),
              name: "LumenPodContentScript",
              fileName: () => "content-script.js",
              formats: ["iife"],
            },
            rollupOptions: {
              output: {
                inlineDynamicImports: true,
              },
            },
          },
        });
      },
    },
  ],
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        "side-panel": resolve(__dirname, "src/side-panel/index.html"),
        "service-worker": resolve(__dirname, "src/service-worker.ts"),
      },
      output: {
        entryFileNames: (chunkInfo) => {
          if (chunkInfo.name === "service-worker") {
            return "[name].js";
          }
          return "assets/[name]-[hash].js";
        },
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
});
