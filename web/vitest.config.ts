import { defineConfig } from "vitest/config";

// The web test runner. `root` is the repo root (this config site, web/, dispatches from there;
// `cpd test run web/<file>.test.ts::<title>` passes repo-root-relative file paths via
// `pnpm --filter cpd-web-client exec vitest`, so vitest has to see `web/`-prefixed paths).
// Environment is node by default — no view needs a DOM yet (the views harness may add jsdom).
export default defineConfig({
  test: {
    root: "..",
    environment: "node",
    globals: true,
    include: ["web/src/**/*.test.{ts,tsx}"],
  },
});
