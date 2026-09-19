// エクスポート/インポートの「エクスポート」「インポート」ボタンは、押した瞬間にOSネイティブの
// ファイルダイアログ(plugin:dialog|save・plugin:dialog|open)を呼ぶ。ネイティブダイアログは
// WebDriverから操作できず、テスト側からinvokeを差し替えることもできない。そのため、E2Eビルド
// (VITE_E2E_TESTING)に限り、gui/src/lib/file-dialog.tsがwindow.__e2eFileDialogPathsに
// 指定されたパスをダイアログの代わりに返す。このファイルは、browser.tauri.executeでその
// パスを設定してから、エクスポート/インポートを実行する。
// バックエンドのexport/import本体のロジック(暗号化・復号・往復・エラー系)は
// gui/src-tauri/src/export_import.rsの実ファイル・実DBを使ったテストでも検証している。

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

async function completeInitialSetup() {
  const startButton = await $("button=始める");
  try {
    await startButton.waitForExist({ timeout: 5000 });
    await startButton.click();
  } catch {
    // 既に初期化済み。
  }
  const maskButton = await $("button*=マスク実行");
  await maskButton.waitForExist({ timeout: 10000 });
}

async function createProfileViaIpc(name: string) {
  await browser.tauri.execute(
    ({ core }, profileName) =>
      core.invoke("create_profile", {
        profile: { profile_name: profileName, description: null, rules: [] },
      }),
    name
  );
}

// 保存/開くダイアログの代わりに返すパスを、アプリ(E2Eビルド)側へ設定する。
async function setE2eFileDialogPaths(paths: { save?: string; open?: string }) {
  await browser.tauri.execute((_tauri, p) => {
    (window as unknown as { __e2eFileDialogPaths?: typeof p }).__e2eFileDialogPaths = p;
  }, paths);
}

// プロファイル管理画面を開き、指定した名前の行の「エクスポート」を押してモーダルを返す。
async function openExportDialogFor(profileName: string) {
  const listButton = await $("button=プロファイル一覧");
  await listButton.waitForExist({ timeout: 10000 });
  await listButton.click();
  await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });

  const row = await $(
    `//div[contains(@class,"rounded-lg")][.//*[contains(text(),"${profileName}")]]`
  );
  const exportButton = await row.$("button=エクスポート");
  await exportButton.waitForExist({ timeout: 10000 });
  await exportButton.click();

  const dialog = await $('[role="dialog"]');
  await dialog.waitForExist({ timeout: 10000 });
  return dialog;
}

// このwdio runプロセス内で後続に実行される他のit()がcompleteInitialSetup()経由で
// メイン画面前提のまま始められるよう、プロファイル管理画面からメイン画面へ戻す
// (spec単位でアプリが再起動されるわけではなく、状態が共有され続けるため)。
async function returnToMainScreen() {
  await (await $("button=閉じる(メイン画面へ)")).click();
  await (await $("button*=マスク実行")).waitForExist({ timeout: 10000 });
}

describe("エクスポートモーダル", () => {
  it("プロファイル一覧の「エクスポート」から実データで生成されたパスフレーズ入りのモーダルが開く", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eエクスポート表示確認");

    const dialog = await openExportDialogFor("E2Eエクスポート表示確認");
    const dialogText = await dialog.getText();
    expect(dialogText).toContain("E2Eエクスポート表示確認");

    const passphraseInput = await dialog.$("input[readonly]");
    await passphraseInput.waitForExist({ timeout: 10000 });
    const passphrase = await passphraseInput.getValue();
    expect(passphrase.length).toBeGreaterThan(0);

    await (await dialog.$("button=キャンセル")).click();
    await returnToMainScreen();
  });
});

describe("エクスポートの実行(ファイルダイアログの差し替え)", () => {
  let exportDir: string;

  before(() => {
    // アプリのデータフォルダ(SENSITIVEMASKER_DATA_DIR)の外に置く(保存先の検証で拒否されるため)。
    exportDir = fs.mkdtempSync(path.join(os.tmpdir(), "sensitivemasker-e2e-files-"));
  });

  after(() => {
    fs.rmSync(exportDir, { recursive: true, force: true });
  });

  it("保存先を指定すると、保存ダイアログを開かずにファイルが書き出される", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eエクスポート実行確認");
    const exportPath = path.join(exportDir, "export.smx");
    await setE2eFileDialogPaths({ save: exportPath });

    const dialog = await openExportDialogFor("E2Eエクスポート実行確認");
    await (await dialog.$("button=エクスポート")).click();

    await $("div*=エクスポートが完了しました").waitForExist({ timeout: 10000 });
    expect(fs.existsSync(exportPath)).toBe(true);
    expect(fs.statSync(exportPath).size).toBeGreaterThan(0);

    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });
});
