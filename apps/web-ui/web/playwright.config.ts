import { defineConfig } from "@playwright/test";

// Cairn runs on 5273 — port 5173 is tau-web-ui's default and would collide
// (Playwright's reuseExistingServer would serve the wrong app). strictPort
// makes a collision fail loudly instead of silently picking another port.
const PORT = 5273;

export default defineConfig({
  testDir: "./e2e",
  use: { baseURL: `http://localhost:${PORT}` },
  webServer: {
    command: `pnpm dev --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !process.env.CI,
  },
});
