import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

function readJson<T>(relativePath: string): T {
  return JSON.parse(readFileSync(new URL(relativePath, import.meta.url), "utf8")) as T;
}

// tauri.conf.jsonの設定のうち、画面の安全性に関わり、外れても画面から気づけないものを固定する。
describe("メインウィンドウの設定(tauri.conf.json)", () => {
  const config = readJson<{ app: { windows: Array<Record<string, unknown>> } }>("../../src-tauri/tauri.conf.json");

  // WebView2の自動補完(Suggestions)は、入力欄のautocomplete="off"を守らない場合がある。
  // パスフレーズや、ルールのパターンなどの機微な入力が候補として残らないよう、窓ごと無効にする。
  it("WebView2の自動補完(Suggestions)を無効にしている", () => {
    expect(config.app.windows[0].generalAutofillEnabled).toBe(false);
  });
});

// E2Eだけが使う口は、scripts/generate-e2e-config.tsが、E2Eビルドのときだけ、上書きの設定(tauri.e2e.conf.json)へ足す。
// 配布用の設定(tauri.conf.jsonと、capabilities/default.json)へ入ると、画面(JavaScript)が、ウィンドウを操作でき、
// グローバルなTauriのAPIを呼べてしまうため、入り込んでいないことを固定する。
describe("配布用の設定に、E2E専用の口が入っていない", () => {
  const config = readJson<{
    app: { withGlobalTauri?: boolean; security?: { capabilities?: unknown[] } };
  }>("../../src-tauri/tauri.conf.json");
  const capability = readJson<{ permissions: string[] }>("../../src-tauri/capabilities/default.json");

  it("グローバルなTauriのAPI(window.__TAURI__)を公開しない", () => {
    expect(config.app.withGlobalTauri ?? false).toBe(false);
  });

  it("ウィンドウを閉じる・表示する権限を、配布用のcapabilitiesへ足さない", () => {
    expect(capability.permissions).not.toContain("core:window:allow-close");
    expect(capability.permissions).not.toContain("core:window:allow-show");
  });

  // 許可する権限の種類は、core:default・ダイアログ2つ・アプリ自身のコマンド(allow-…)だけに限る。ウィンドウ・ウェブビューの操作や、
  // プラグインの権限を足すと、画面(JavaScript)が、その操作を行えるため、足すときは、このテストを、意図して更新する。
  it("許可する権限の種類を、core:default・ダイアログ・アプリ自身のコマンドだけに限る", () => {
    const allowed = /^(core:default|dialog:allow-(open|save)|allow-[a-z0-9-]+)$/;
    expect(capability.permissions.filter((permission) => !allowed.test(permission))).toEqual([]);
  });

  it("E2E用のプラグイン(wdio)の権限を、配布用のcapabilitiesへ足さない", () => {
    expect(capability.permissions.filter((permission) => permission.startsWith("wdio"))).toEqual([]);
  });

  it("E2E専用のcapabilityを、tauri.conf.jsonのcapabilitiesへ足さない", () => {
    const capabilities = config.app.security?.capabilities ?? [];
    const identifiers = capabilities.map((entry) =>
      typeof entry === "string" ? entry : (entry as { identifier?: string }).identifier
    );
    expect(identifiers).not.toContain("e2e");
  });
});
