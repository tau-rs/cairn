import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { homedir } from "node:os";

// Vault whose daemon we talk to. Override with CAIRN_VAULT; daemon addr with CAIRN_DAEMON.
const VAULT = process.env.CAIRN_VAULT ?? resolve(homedir(), "notes");
const DAEMON = process.env.CAIRN_DAEMON ?? "http://127.0.0.1:7777";

function readToken(): string | null {
  try {
    return readFileSync(resolve(VAULT, ".cairn/token"), "utf8").trim();
  } catch {
    return null; // daemon not started / no token yet
  }
}

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/query": {
        target: DAEMON,
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on("proxyReq", (proxyReq) => {
            const token = readToken(); // re-read per request: survives daemon restart
            if (token) proxyReq.setHeader("authorization", `Bearer ${token}`);
          });
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./vitest.setup.ts"],
  },
});
