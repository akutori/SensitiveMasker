// エクスポート/インポートの「エクスポート」「インポート」ボタンは、押した瞬間にOSネイティブの
// ファイルダイアログ(plugin:dialog|save・plugin:dialog|open)を呼ぶ。ネイティブダイアログは
// WebDriverから操作できず、テスト側からinvokeを差し替えることもできない。そのため、E2Eビルド
// (VITE_E2E_TESTING)に限り、gui/src/lib/file-dialog.tsがwindow.__e2eFileDialogPathsに
// 指定された値をダイアログの代わりに返す。このファイルは、browser.tauri.executeでその
// 値を設定してから、エクスポート/インポートを実行する。保存ダイアログには、決着を
// テストが握るPromiseも指定でき、書き出し中の画面や、取り消し・失敗を検証するために使う。
// バックエンドのexport/import本体のロジック(暗号化・復号・往復・エラー系)は
// gui/src-tauri/src/export_import.rsの実ファイル・実DBを使ったテストでも検証している。

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

async function completeInitialSetup() {
  const maskButton = await $("button*=マスク実行");
  // 既に初期化済みでメイン画面が出ていれば、何も待たない。
  if (await maskButton.isExisting()) return;
  const startButton = await $("button=始める");
  await startButton.waitForExist({ timeout: 10000 });
  await startButton.click();
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

async function deleteProfileViaIpc(name: string) {
  await browser.tauri.execute(
    ({ core }, profileName) => core.invoke("delete_profile", { name: profileName }),
    name
  );
}

// アクティブなプロファイルは削除できないため、削除するプロファイル以外をアクティブにしておく用途。
async function setActiveProfileViaIpc(name: string) {
  await browser.tauri.execute(
    ({ core }, profileName) => core.invoke("set_active_profile", { name: profileName }),
    name
  );
}

// 保存/開くダイアログの代わりに返すパスを、アプリ(E2Eビルド)側へ設定する。
async function setE2eFileDialogPaths(paths: { save?: string; open?: string }) {
  await browser.tauri.execute((_tauri, p) => {
    (window as unknown as { __e2eFileDialogPaths?: typeof p }).__e2eFileDialogPaths = p;
  }, paths);
}

// 保存ダイアログの結果を、settleSaveDialogが呼ばれるまで保留する。
async function holdSaveDialog() {
  await browser.tauri.execute(() => {
    const w = window as unknown as {
      __e2eFileDialogPaths?: { save?: Promise<string | null> };
      __e2eSaveControl?: { resolve: (path: string | null) => void; reject: (reason: Error) => void };
    };
    w.__e2eFileDialogPaths = {
      save: new Promise<string | null>((resolve, reject) => {
        w.__e2eSaveControl = { resolve, reject };
      }),
    };
  });
}

// 保留した保存ダイアログを、保存先の決定(path)・取り消し(null)・失敗(error)のいずれかで決着させる。
async function settleSaveDialog(outcome: { path: string | null } | { error: string }) {
  await browser.tauri.execute((_tauri, o) => {
    const control = (
      window as unknown as {
        __e2eSaveControl?: { resolve: (path: string | null) => void; reject: (reason: Error) => void };
      }
    ).__e2eSaveControl;
    if (!control) throw new Error("保存ダイアログが保留されていない(holdSaveDialogを先に呼ぶ)");
    if ("error" in o) control.reject(new Error(o.error));
    else control.resolve(o.path);
  }, outcome);
}

// holdSaveDialogに加えて、保存ダイアログを開こうとした回数(差し替え値が参照された回数)を数える。
async function holdSaveDialogCounting() {
  await browser.tauri.execute(() => {
    const w = window as unknown as {
      __e2eFileDialogPaths?: object;
      __e2eSaveControl?: { resolve: (path: string | null) => void; reject: (reason: Error) => void };
      __e2eSaveCalls?: number;
    };
    w.__e2eSaveCalls = 0;
    const pending = new Promise<string | null>((resolve, reject) => {
      w.__e2eSaveControl = { resolve, reject };
    });
    const paths = {};
    Object.defineProperty(paths, "save", {
      enumerable: true,
      get() {
        w.__e2eSaveCalls = (w.__e2eSaveCalls ?? 0) + 1;
        return pending;
      },
    });
    w.__e2eFileDialogPaths = paths;
  });
}

async function saveDialogCalls(): Promise<number> {
  return browser.tauri.execute(
    () => (window as unknown as { __e2eSaveCalls?: number }).__e2eSaveCalls ?? 0
  );
}

// 開くダイアログの結果を、settleOpenDialogが呼ばれるまで保留する。
async function holdOpenDialog() {
  await browser.tauri.execute(() => {
    const w = window as unknown as {
      __e2eFileDialogPaths?: { open?: Promise<string | null> };
      __e2eOpenControl?: { resolve: (path: string | null) => void };
    };
    w.__e2eFileDialogPaths = {
      open: new Promise<string | null>((resolve) => {
        w.__e2eOpenControl = { resolve };
      }),
    };
  });
}

// 保留した開くダイアログを、選んだパス(path)または取り消し(null)で決着させる。
async function settleOpenDialog(path: string | null) {
  await browser.tauri.execute((_tauri, p) => {
    const control = (
      window as unknown as { __e2eOpenControl?: { resolve: (path: string | null) => void } }
    ).__e2eOpenControl;
    if (!control) throw new Error("開くダイアログが保留されていない(holdOpenDialogを先に呼ぶ)");
    control.resolve(p);
  }, path);
}

// クリップボードが期待した値と一致していれば、クリアして真を返す。WebViewからクリップボードを
// 読めないため、比較とクリアをRust側で行うコマンドの結果で確かめる。
async function clipboardHolds(expected: string): Promise<boolean> {
  const result = await browser.tauri.execute(
    ({ core }, text) => core.invoke("clear_clipboard_if_matches", { expected: text }),
    expected
  );
  return (result as { outcome: string }).outcome === "cleared";
}

async function currentUrl(): Promise<string> {
  return browser.tauri.execute(() => location.href);
}

async function focusIsInsideDialog(): Promise<boolean> {
  return browser.tauri.execute(() => !!document.activeElement?.closest('[role="dialog"]'));
}

// 失敗したテストが残した状態(保留中のダイアログ、開いたままの画面)を片付け、後続のテストへ
// 連鎖させない。成功したテストでは何も起きない。片付けそのものの失敗は、テストの失敗にしない。
afterEach(async () => {
  try {
    await browser.tauri.execute(() => {
      const w = window as unknown as {
        __e2eFileDialogPaths?: unknown;
        __e2eSaveControl?: { resolve: (path: string | null) => void };
        __e2eOpenControl?: { resolve: (path: string | null) => void };
      };
      w.__e2eSaveControl?.resolve(null);
      w.__e2eOpenControl?.resolve(null);
      // 次のテストが、保留していないのにsettleしたとき、前のテストの参照に当たらず失敗するようにする。
      w.__e2eSaveControl = undefined;
      w.__e2eOpenControl = undefined;
      w.__e2eFileDialogPaths = undefined;
    });
    for (let attempt = 0; attempt < 3; attempt++) {
      const openDialog = await $('[role="dialog"], [role="alertdialog"]');
      if (!(await openDialog.isExisting())) break;
      const closeButton = await openDialog.$("button=閉じる");
      if (await closeButton.isExisting()) await closeButton.click();
      else await browser.keys("Escape");
      await browser.pause(300);
    }
    const backButton = await $("button=閉じる(メイン画面へ)");
    if (await backButton.isExisting()) await backButton.click();
  } catch {
    // 片付けは最善を尽くすだけで、失敗しても元のテストの結果を覆い隠さない。
  }
});

async function openProfileManagement() {
  const listButton = await $("button=プロファイル一覧");
  await listButton.waitForExist({ timeout: 10000 });
  await listButton.click();
  await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });
}

// プロファイル管理画面で、指定した名前の行の「エクスポート」を押してモーダルを返す。
async function openExportDialogFromRow(profileName: string) {
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

// メイン画面からプロファイル管理画面を開き、指定した名前の行のエクスポートモーダルを返す。
async function openExportDialogFor(profileName: string) {
  await openProfileManagement();
  return openExportDialogFromRow(profileName);
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

  it("コピー中に無効になった再生成が、応答後に有効へ戻る(無効のまま固まらない)", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eエクスポートコピー確認");

    const dialog = await openExportDialogFor("E2Eエクスポートコピー確認");
    const passphraseInput = await dialog.$("input[readonly]");
    const passphrase = await passphraseInput.getValue();

    await (await dialog.$("button=コピー")).click();
    await $("div*=パスフレーズをコピーしました").waitForExist({ timeout: 10000 });
    const regenerateButton = await dialog.$("button=再生成");
    await regenerateButton.waitForEnabled({ timeout: 10000 });
    await regenerateButton.click();
    await browser.waitUntil(async () => (await passphraseInput.getValue()) !== passphrase, {
      timeout: 10000,
      timeoutMsg: "再生成してもパスフレーズが変わらなかった",
    });

    await (await dialog.$("button=キャンセル")).click();
    await returnToMainScreen();
  });

  it("コピーすると、自動クリアまでの秒数が、通知と注意文に表示される", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eエクスポート秒数表示確認");

    const dialog = await openExportDialogFor("E2Eエクスポート秒数表示確認");
    expect(await dialog.getText()).toContain("E2Eエクスポート秒数表示確認");
    expect(await dialog.getText()).toContain("コピーの30秒後に、自動クリアを試みます");

    await (await dialog.$("button=コピー")).click();
    const copiedToast = await $("div*=パスフレーズをコピーしました");
    await copiedToast.waitForExist({ timeout: 10000 });
    expect(await copiedToast.getText()).toContain("30秒後に自動クリアを試みます");

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

  it("成功しても画面は開いたままで、同じパスフレーズが伏せ字で残り、閉じて開き直すと別のパスフレーズになる", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eエクスポート実行確認");
    const exportPath = path.join(exportDir, "export.smx");
    await setE2eFileDialogPaths({ save: exportPath });

    const dialog = await openExportDialogFor("E2Eエクスポート実行確認");
    const passphraseInput = await dialog.$("input[readonly]");
    const passphrase = await passphraseInput.getValue();

    // 「表示」にしてから実行しても、成功した時点で伏せ字へ戻る。
    await (await dialog.$('button[aria-label="パスフレーズを表示"]')).click();
    expect(await passphraseInput.getAttribute("type")).toBe("text");

    await (await dialog.$("button=エクスポート")).click();

    const notice = await dialog.$('[role="status"]');
    await notice.waitForExist({ timeout: 10000 });
    expect(await notice.getText()).toContain("二度と表示できません");
    expect(fs.statSync(exportPath).size).toBeGreaterThan(0);

    // 画面は開いたままで、同じパスフレーズが伏せ字で残る。再生成・エクスポートは出ない。
    expect(await passphraseInput.getValue()).toBe(passphrase);
    expect(await passphraseInput.getAttribute("type")).toBe("password");
    for (const label of ["再生成", "エクスポート", "キャンセル"]) {
      expect(await dialog.$(`button=${label}`).isExisting()).toBe(false);
    }
    await (await dialog.$("button=コピー")).waitForEnabled({ timeout: 10000 });

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });

    // 開き直すと、別のパスフレーズが生成される。
    const reopened = await openExportDialogFromRow("E2Eエクスポート実行確認");
    expect(await (await reopened.$("input[readonly]")).getValue()).not.toBe(passphrase);
    await (await reopened.$("button=キャンセル")).click();
    await reopened.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("単一プロファイルを書き出したファイルは、画面に表示されていたパスフレーズでインポートできる", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート単一往復確認";
    // このテストだけで実行しても、書き出したプロファイルをあとで削除できるようにする
    // (最初に作ったプロファイルは自動でアクティブになり、削除できないため)。
    const activeProfileName = "E2E単一往復確認用の保持プロファイル";
    await createProfileViaIpc(activeProfileName);
    await setActiveProfileViaIpc(activeProfileName);
    await createProfileViaIpc(profileName);
    const exportPath = path.join(exportDir, "single-roundtrip.smx");
    await setE2eFileDialogPaths({ save: exportPath, open: exportPath });

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    await (await dialog.$("button=エクスポート")).click();
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });

    // 単一プロファイルのインポートは、同名のプロファイルが既にあると拒否されるため、先に削除する。
    await deleteProfileViaIpc(profileName);

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(passphrase);
    await (await importDialog.$("button=OK")).click();

    // 表示されていたパスフレーズなら、内容の確認画面へ進む(取り込みは実行せず取り消す)。
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    const confirmText = await confirmDialog.getText();
    expect(confirmText).toContain("インポート内容の確認");
    expect(confirmText).toContain(profileName);
    await (await confirmDialog.$("button=キャンセル")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("コピーしたパスフレーズは、編集中も、再生成の後も、書き出しの後も、画面に表示されているものと一致する", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポートコピー値確認";
    await createProfileViaIpc(profileName);
    await setE2eFileDialogPaths({ save: path.join(exportDir, "copy-value.smx") });

    const dialog = await openExportDialogFor(profileName);
    const passphraseInput = await dialog.$("input[readonly]");
    const copyButton = await dialog.$("button=コピー");

    const initial = await passphraseInput.getValue();
    await copyButton.click();
    await browser.waitUntil(() => clipboardHolds(initial), {
      timeout: 10000,
      timeoutMsg: "編集中にコピーした値が、画面のパスフレーズと一致しない",
    });

    // 再生成すると、新しいパスフレーズがコピーされる(古い値ではない)。
    const regenerateButton = await dialog.$("button=再生成");
    await regenerateButton.waitForEnabled({ timeout: 10000 });
    await regenerateButton.click();
    await browser.waitUntil(async () => (await passphraseInput.getValue()) !== initial, {
      timeout: 10000,
      timeoutMsg: "再生成してもパスフレーズが変わらなかった",
    });
    const regenerated = await passphraseInput.getValue();
    await copyButton.waitForEnabled({ timeout: 10000 });
    await copyButton.click();
    await browser.waitUntil(() => clipboardHolds(regenerated), {
      timeout: 10000,
      timeoutMsg: "再生成の後にコピーした値が、画面のパスフレーズと一致しない",
    });

    // 書き出しの後も、同じパスフレーズがコピーされる(相手へ渡すための、この機能の主目的)。
    await (await dialog.$("button=エクスポート")).click();
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    await copyButton.waitForEnabled({ timeout: 10000 });
    await copyButton.click();
    await browser.waitUntil(() => clipboardHolds(regenerated), {
      timeout: 10000,
      timeoutMsg: "書き出しの後にコピーした値が、画面のパスフレーズと一致しない",
    });

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("保存先の選択を取り消すと、編集中に戻り、続けて書き出せる", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート取消確認";
    await createProfileViaIpc(profileName);

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    const exportButton = await dialog.$("button=エクスポート");

    await holdSaveDialog();
    await exportButton.click();
    await exportButton.waitForEnabled({ reverse: true, timeout: 10000 });
    await settleSaveDialog({ path: null });
    await exportButton.waitForEnabled({ timeout: 10000 });
    expect(await dialog.$('[role="status"]').isExisting()).toBe(false);

    const exportPath = path.join(exportDir, "after-cancel.smx");
    await setE2eFileDialogPaths({ save: exportPath });
    await exportButton.click();
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    expect(fs.statSync(exportPath).size).toBeGreaterThan(0);
    expect(await (await dialog.$("input[readonly]")).getValue()).toBe(passphrase);

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("書き出しに失敗すると、通知され、編集中に戻り、別の保存先で書き出せる", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート失敗確認";
    await createProfileViaIpc(profileName);

    const dialog = await openExportDialogFor(profileName);
    const exportButton = await dialog.$("button=エクスポート");

    // 拡張子が.smxでない保存先は、書き出しの事前検証で拒否される。
    const invalidPath = path.join(exportDir, "invalid.txt");
    await setE2eFileDialogPaths({ save: invalidPath });
    await exportButton.click();
    await $("div*=保存先には拡張子.smxを指定してください").waitForExist({ timeout: 10000 });
    await exportButton.waitForEnabled({ timeout: 10000 });
    expect(await dialog.$('[role="status"]').isExisting()).toBe(false);
    expect(fs.existsSync(invalidPath)).toBe(false);

    const exportPath = path.join(exportDir, "after-failure.smx");
    await setE2eFileDialogPaths({ save: exportPath });
    await exportButton.click();
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    expect(fs.statSync(exportPath).size).toBeGreaterThan(0);

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("保存先の選択そのものが失敗すると、通知され、編集中に戻る", async () => {
    await completeInitialSetup();
    const profileName = "E2E保存先ダイアログ失敗確認";
    await createProfileViaIpc(profileName);

    const dialog = await openExportDialogFor(profileName);
    const exportButton = await dialog.$("button=エクスポート");

    await holdSaveDialog();
    await exportButton.click();
    await exportButton.waitForEnabled({ reverse: true, timeout: 10000 });
    await settleSaveDialog({ error: "e2e: 保存ダイアログの失敗" });
    await $("div*=保存先を選択できませんでした").waitForExist({ timeout: 10000 });
    await exportButton.waitForEnabled({ timeout: 10000 });
    expect(await dialog.$('[role="status"]').isExisting()).toBe(false);

    await (await dialog.$("button=キャンセル")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("書き出し中は、Escapeを押しても閉じず、フォーカスは画面の内側に留まる", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート実行中確認";
    await createProfileViaIpc(profileName);

    // 編集中はEscapeで閉じる。以降の「閉じない」の確認が、キーが届いていることを前提にできる。
    const editingDialog = await openExportDialogFor(profileName);
    await browser.keys("Escape");
    await editingDialog.waitForExist({ reverse: true, timeout: 10000 });

    const dialog = await openExportDialogFromRow(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    const exportButton = await dialog.$("button=エクスポート");
    await holdSaveDialog();
    await exportButton.click();
    await exportButton.waitForEnabled({ reverse: true, timeout: 10000 });

    // 押したボタンが無効になっても、フォーカスは画面の内側にある(外へ落ちると、Tabで背景の画面へ
    // 出られ、書き出し中に背景の操作をして、パスフレーズを失いかねない)。WebDriverのTabはキー
    // イベントの送出だけでブラウザ既定のフォーカス移動は起きないため、Tabの後の確認は、
    // フォーカストラップのキー処理が画面の内側に留めることの確認になる。
    expect(await focusIsInsideDialog()).toBe(true);
    await browser.keys(["Tab", "Tab", "Tab"]);
    expect(await focusIsInsideDialog()).toBe(true);

    await browser.keys("Escape");
    await browser.pause(500);
    expect(await dialog.isDisplayed()).toBe(true);

    // 保存先が決まると書き出しに成功し、同じパスフレーズが残っている。
    await settleSaveDialog({ path: path.join(exportDir, "during-export.smx") });
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    expect(await (await dialog.$("input[readonly]")).getValue()).toBe(passphrase);

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("envインポートのファイルの選択・読み込みを待つ間に開かれたエクスポート画面を、置き換えない", async () => {
    await completeInitialSetup();
    const profileName = "E2E環境変数取込中確認";
    await createProfileViaIpc(profileName);
    // 内容は架空の値だけにする。
    const envPath = path.join(exportDir, "sample.env");
    fs.writeFileSync(envPath, "API_TOKEN=dummy-token-value-0001\n");
    await openProfileManagement();

    // 対照: エクスポート画面を開かなければ、読み込みが終わった時点で、取り込み対象の選択画面が出る。
    await holdOpenDialog();
    await (await $("button=envインポート")).click();
    await settleOpenDialog(envPath);
    const selectDialog = await $('[role="dialog"]');
    await selectDialog.waitForExist({ timeout: 10000 });
    expect(await selectDialog.getText()).toContain("取り込み対象を選択");
    await (await selectDialog.$("button=キャンセル")).click();
    await selectDialog.waitForExist({ reverse: true, timeout: 10000 });

    // ファイルの選択を待つ間にエクスポート画面を開くと、読み込みが終わっても、その画面のまま残る。
    await holdOpenDialog();
    await (await $("button=envインポート")).click();
    const exportDialog = await openExportDialogFromRow(profileName);
    await settleOpenDialog(envPath);
    await browser.pause(1500);
    expect(await exportDialog.isDisplayed()).toBe(true);
    expect(await exportDialog.getText()).toContain("エクスポート: " + profileName);

    await (await exportDialog.$("button=キャンセル")).click();
    await exportDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("エクスポートを同じ瞬間に2回押しても、保存先の選択と書き出しは1回だけ実行される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート二重実行確認";
    await createProfileViaIpc(profileName);
    const dialog = await openExportDialogFor(profileName);
    await holdSaveDialogCounting();

    // 1回のJSタスクの中で2回押す(描画される前なので、ボタンはまだ無効になっていない)。
    await browser.tauri.execute(() => {
      const button = Array.from(document.querySelectorAll('[role="dialog"] button')).find(
        (b) => b.textContent?.trim() === "エクスポート"
      ) as HTMLButtonElement | undefined;
      if (!button) throw new Error("エクスポートのボタンが見つからない");
      button.click();
      button.click();
    });
    await (await dialog.$("button=エクスポート")).waitForEnabled({ reverse: true, timeout: 10000 });
    expect(await saveDialogCalls()).toBe(1);

    await settleSaveDialog({ path: path.join(exportDir, "double-click.smx") });
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    expect(await saveDialogCalls()).toBe(1);

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("画面を閉じないまま履歴で戻る操作をしても、書き出し後のエクスポート画面は残る", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート履歴戻り確認";
    await createProfileViaIpc(profileName);
    await setE2eFileDialogPaths({ save: path.join(exportDir, "history-back.smx") });

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    await (await dialog.$("button=エクスポート")).click();
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });

    // マウスの戻るボタンなどと同じ、履歴の移動(popstate)を起こす操作。
    const urlBefore = await currentUrl();
    await browser.back();
    await browser.pause(500);
    expect(await dialog.isDisplayed()).toBe(true);
    expect(await (await dialog.$("input[readonly]")).getValue()).toBe(passphrase);
    expect(await currentUrl()).toBe(urlBefore);

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("書き出し中に履歴で戻る操作をしても、エクスポート画面は残り、履歴の位置も変わらない", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート実行中履歴戻り確認";
    await createProfileViaIpc(profileName);

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    const exportButton = await dialog.$("button=エクスポート");
    await holdSaveDialog();
    await exportButton.click();
    await exportButton.waitForEnabled({ reverse: true, timeout: 10000 });

    const urlBefore = await currentUrl();
    await browser.back();
    await browser.pause(500);
    expect(await dialog.isDisplayed()).toBe(true);
    expect(await (await dialog.$("input[readonly]")).getValue()).toBe(passphrase);
    expect(await currentUrl()).toBe(urlBefore);

    await settleSaveDialog({ path: path.join(exportDir, "history-back-exporting.smx") });
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("再生成の直後に、同じ瞬間にエクスポートを押しても、画面に表示されるパスフレーズで書き出される", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2E再生成直後実行確認");
    const filePath = path.join(exportDir, "regenerate-then-export.smx");
    await setE2eFileDialogPaths({ save: filePath, open: filePath });

    await openProfileManagement();
    await (await $("button=全体エクスポート")).click();
    const exportDialog = await $('[role="dialog"]');
    await exportDialog.waitForExist({ timeout: 10000 });
    const passphraseInput = await exportDialog.$("input[readonly]");
    const initial = await passphraseInput.getValue();

    // 1回のJSタスクの中で、再生成とエクスポートを続けて押す(再描画される前なので、2つ目の押下は、
    // 再生成する前の画面を見る)。
    await browser.tauri.execute(() => {
      const buttons = Array.from(document.querySelectorAll('[role="dialog"] button'));
      const find = (label: string) =>
        buttons.find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
      const regenerate = find("再生成");
      const exportButton = find("エクスポート");
      if (!regenerate || !exportButton) throw new Error("ボタンが見つからない");
      regenerate.click();
      exportButton.click();
    });
    await (await exportDialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    const displayed = await passphraseInput.getValue();
    expect(displayed).not.toBe(initial);
    await (await exportDialog.$("button=閉じる")).click();
    await exportDialog.waitForExist({ reverse: true, timeout: 10000 });

    // 書き出したファイルは、画面に表示されていたパスフレーズでインポートできる。
    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(displayed);
    await (await importDialog.$("button=OK")).click();
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    expect(await confirmDialog.getText()).toContain("インポート内容の確認");
    await (await confirmDialog.$("button=キャンセル")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("書き出し後は、Escapeを押しても閉じず、「閉じる」で閉じる", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート成功後確認";
    await createProfileViaIpc(profileName);
    await setE2eFileDialogPaths({ save: path.join(exportDir, "after-export.smx") });

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    await (await dialog.$("button=エクスポート")).click();
    await (await dialog.$('[role="status"]')).waitForExist({ timeout: 10000 });

    await browser.keys("Escape");
    await browser.pause(500);
    expect(await dialog.isDisplayed()).toBe(true);
    expect(await (await dialog.$("input[readonly]")).getValue()).toBe(passphrase);

    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("全体エクスポートしたファイルを、画面に表示されていたパスフレーズでインポートできる(誤ったパスフレーズはエラーになる)", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eインポート往復確認");
    const roundTripPath = path.join(exportDir, "roundtrip.smx");
    await setE2eFileDialogPaths({ save: roundTripPath, open: roundTripPath });

    // 単一プロファイルのインポートは、同名のプロファイルが既にあると拒否される。同じDBでの
    // 往復は、重複した名前が自動でリネームされる全体エクスポートで行う。
    await openProfileManagement();
    await (await $("button=全体エクスポート")).click();
    const exportDialog = await $('[role="dialog"]');
    await exportDialog.waitForExist({ timeout: 10000 });
    const passphrase = await (await exportDialog.$("input[readonly]")).getValue();
    await (await exportDialog.$("button=エクスポート")).click();
    await (await exportDialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    await (await exportDialog.$("button=閉じる")).click();
    await exportDialog.waitForExist({ reverse: true, timeout: 10000 });

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    expect(await importDialog.getText()).toContain("パスフレーズを入力");

    // 誤ったパスフレーズはエラーになり、ダイアログは開いたままになる。
    const input = await importDialog.$("input");
    await input.setValue("wrong-passphrase");
    await (await importDialog.$("button=OK")).click();
    const error = await importDialog.$('[role="alert"]');
    await error.waitForExist({ timeout: 15000 });
    expect(await error.getText()).toContain("復号に失敗しました");

    // 表示されていたパスフレーズなら、内容の確認画面へ進む(取り込みは実行せず取り消す)。
    // 同名のプロファイルは、リネームされて取り込まれる内容として表示される。
    await input.setValue(passphrase);
    await (await importDialog.$("button=OK")).click();
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    const confirmText = await confirmDialog.getText();
    expect(confirmText).toContain("インポート内容の確認");
    expect(confirmText).toContain("E2Eインポート往復確認 (インポート)");
    await (await confirmDialog.$("button=キャンセル")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("インポートのパスフレーズ欄は伏せ字から始まり、「表示」で入力内容を確認でき、開き直すと伏せ字へ戻る", async () => {
    await completeInitialSetup();
    // パスフレーズ入力画面を開くだけなので、指すファイルは実在しなくてよい。
    await setE2eFileDialogPaths({ open: path.join(exportDir, "dummy.smx") });
    await openProfileManagement();

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    const input = await importDialog.$("input");
    expect(await input.getAttribute("type")).toBe("password");

    await input.setValue("dummy-passphrase-0001");
    await (await importDialog.$('button[aria-label="パスフレーズを表示"]')).click();
    expect(await input.getAttribute("type")).toBe("text");
    expect(await input.getValue()).toBe("dummy-passphrase-0001");
    await (await importDialog.$('button[aria-label="パスフレーズを隠す"]')).click();
    expect(await input.getAttribute("type")).toBe("password");

    // 表示にしたまま閉じて開き直すと、伏せ字へ戻っている。
    await (await importDialog.$('button[aria-label="パスフレーズを表示"]')).click();
    await (await importDialog.$("button=キャンセル")).click();
    await importDialog.waitForExist({ reverse: true, timeout: 10000 });
    await (await $("button=インポート")).click();
    const reopened = await $('[role="dialog"]');
    await reopened.waitForExist({ timeout: 10000 });
    expect(await (await reopened.$("input")).getAttribute("type")).toBe("password");

    await (await reopened.$("button=キャンセル")).click();
    await reopened.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("貼り付けで前後に空白が混ざったパスフレーズでも、インポートの確認画面へ進める", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2E空白許容確認");
    const roundTripPath = path.join(exportDir, "whitespace.smx");
    await setE2eFileDialogPaths({ save: roundTripPath, open: roundTripPath });

    await openProfileManagement();
    await (await $("button=全体エクスポート")).click();
    const exportDialog = await $('[role="dialog"]');
    await exportDialog.waitForExist({ timeout: 10000 });
    const passphrase = await (await exportDialog.$("input[readonly]")).getValue();
    await (await exportDialog.$("button=エクスポート")).click();
    await (await exportDialog.$('[role="status"]')).waitForExist({ timeout: 10000 });
    await (await exportDialog.$("button=閉じる")).click();
    await exportDialog.waitForExist({ reverse: true, timeout: 10000 });

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    const input = await importDialog.$("input");
    const padded = `  ${passphrase}  `;
    await input.setValue(padded);
    expect(await input.getValue()).toBe(padded);
    await (await importDialog.$("button=OK")).click();

    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    expect(await confirmDialog.getText()).toContain("インポート内容の確認");
    await (await confirmDialog.$("button=キャンセル")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });
});
