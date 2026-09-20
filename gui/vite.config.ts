/// <reference types="vitest/config" />
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import process from "node:process";
import path from "node:path";
import { fileURLToPath } from 'node:url';
import { storybookTest } from '@storybook/addon-vitest/vitest-plugin';
import { playwright } from '@vitest/browser-playwright';
const dirname = typeof __dirname !== 'undefined' ? __dirname : path.dirname(fileURLToPath(import.meta.url));

// More info at: https://storybook.js.org/docs/next/writing-tests/integrations/vitest-addon
const host = process.env.TAURI_DEV_HOST;

// monaco-editorが同梱するDOMPurifyの写し(esm/vs/base/browser/dompurify/dompurify.js)は、脆弱性の対象の版で、
// Monaco自身が更新されるまで変わらない。配布物には、同じ形で使える、npmのdompurifyを代わりに入れる
// (package.jsonのoverridesで、修正済みの版に固定している)。vite buildの配布ビルドにだけ効き、開発サーバーと
// Storybookの事前バンドルには効かない。配布物に入った版は、scripts/check-dist.tsが確かめる。
function useNpmDompurifyInMonaco(): Plugin {
  return {
    name: "use-npm-dompurify-in-monaco",
    enforce: "pre",
    resolveId: {
      // 全てのimportについてプラグインを呼ばないよう、対象の指定だけに絞る。
      filter: { id: /^\.\/dompurify\/dompurify\.js$/ },
      async handler(source, importer, options) {
        if (!importer) return null;
        if (!importer.replaceAll("\\", "/").includes("/node_modules/monaco-editor/esm/vs/base/browser/")) return null;
        return this.resolve("dompurify", importer, { ...options, skipSelf: true });
      },
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [
    useNpmDompurifyInMonaco(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src")
    }
  },
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? {
      protocol: "ws",
      host,
      port: 1421
    } : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"]
    }
  },
  test: {
    projects: [{
      extends: true,
      plugins: [
      // The plugin will run tests for the stories defined in your Storybook config
      // See options at: https://storybook.js.org/docs/next/writing-tests/integrations/vitest-addon#storybooktest
      storybookTest({
        configDir: path.join(dirname, '.storybook')
      })],
      test: {
        name: 'storybook',
        browser: {
          enabled: true,
          headless: true,
          provider: playwright({}),
          instances: [{
            browser: 'chromium'
          }],
          // ダイアログ下部のボタンの行は、幅640px未満で縦に積まれる。その幅を前提にするstoryがあるため、
          // ビューポートを明示する(既定値に頼らない)。
          viewport: { width: 414, height: 896 }
        }
      }
    }, {
      // 状態遷移などの純関数のテスト。DOMもブラウザも使わないためNodeで実行する
      // (storybookプロジェクトのようにChromiumを起動しないので、素早く回せる)。
      resolve: {
        alias: {
          "@": path.resolve(dirname, "./src")
        }
      },
      test: {
        name: 'unit',
        environment: 'node',
        include: ['src/**/*.test.ts']
      }
    }]
  }
}));