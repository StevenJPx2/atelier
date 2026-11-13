import { defineConfig } from "cf/config"

export default defineConfig({
  accountId: "7bb7dff27822397d46162181bfedf38d",
  worker: {
    name: "atelier-docs",
    compatibilityDate: "2026-11-12",
    // Paths no redirect covers land on 404.html, which links to the hub.
    assets: { notFoundHandling: "404-page" },
  },
})
