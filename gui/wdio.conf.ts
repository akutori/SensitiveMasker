import path from "node:path";
import os from "node:os";
import fs from "node:fs";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// テスト実行のたびに空の一時ディレクトリを使う(実ユーザーの鍵/DBに触れない)。
// gui/src-tauri/src/profiles.rsのresolve_paths()がdebug build時のみ参照する。
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "sensitivemasker-e2e-"));

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./e2e/**/*.spec.ts"],
  maxInstances: 1,
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: path.resolve(__dirname, "../target/debug/gui.exe"),
      },
    },
  ],
  services: [
    [
      "@wdio/tauri-service",
      {
        driverProvider: "embedded",
        env: { SENSITIVEMASKER_DATA_DIR: dataDir },
      },
    ],
  ],
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 60000,
  },
  logLevel: "info",
};
