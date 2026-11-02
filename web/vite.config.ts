import { defineConfig } from "vite";

export default defineConfig({
  // Served from https://dabupr.github.io/regrex/ on GitHub Pages.
  base: process.env.REGREX_BASE ?? "/regrex/",
  worker: { format: "es" },
  build: { target: "es2022" },
});
