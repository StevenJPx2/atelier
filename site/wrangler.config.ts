import { defineWranglerConfig } from "wrangler/experimental-config"

// cf builds a static site with Wrangler, which reads the folder to upload here.
// The Worker serves only the redirect to the hub; the VitePress build is a check.
export default defineWranglerConfig({
  assetsDirectory: "redirect",
})
