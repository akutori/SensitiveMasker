// エクスポート/インポートの「エクスポート」「インポート」ボタンは、押した瞬間にOSネイティブの
// ファイルダイアログ(plugin:dialog|save・plugin:dialog|open)を呼ぶ。ネイティブダイアログは
// WebDriverから操作できず、テスト側からinvokeを差し替えることもできない。そのため、E2Eビルド
// (VITE_E2E_TESTING)に限り、gui/src/lib/file-dialog.tsがwindow.__e2eFileDialogPathsに
// 指定された値をダイアログの代わりに返す。このファイルは、browser.tauri.executeでその
// 値を設定してから、エクスポート/インポートを実行する。保存・開くのどちらにも、決着を
// テストが握るPromiseを指定でき、選択を待つ間の画面や、取り消し・失敗の後の画面を検証するために使う。
// インポートの保留(Rust側の、復号済みの内容)は、識別子を指定して確定・破棄する。画面の外から確定を直接呼んで
// 保留が残っていないことを確かめられるよう、E2Eビルドは、アプリが受け取った識別子を、受け取った順に、
// window.__e2ePendingImportIdsへ残す(gui/src/lib/e2e-pending-import.ts)。
// バックエンドのexport/import本体のロジック(暗号化・復号・往復・エラー系)は
// gui/src-tauri/src/export_import.rsの実ファイル・実DBを使ったテストでも検証している。

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

async function completeInitialSetup() {
  const maskButton = await $("button*=マスク実行");
  const startButton = await $("button=始める");
  // 初回は初期設定画面、初期化済みならメイン画面が出る。どちらかが出るまで待ってから分岐する
  // (初期化済みのときに、出ない「始める」を待たないため)。
  await browser.waitUntil(
    async () => (await maskButton.isExisting()) || (await startButton.isExisting()),
    { timeout: 15000, timeoutMsg: "初期設定画面もメイン画面も表示されなかった" }
  );
  if (await startButton.isExisting()) {
    await startButton.click();
    await maskButton.waitForExist({ timeout: 10000 });
  }
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

// 開くダイアログの結果を、settleOpenDialogが呼ばれるまで保留する。ダイアログを開こうとした回数(差し替え値が
// 参照された回数)も数え、ボタンの押下が、ダイアログを開く処理まで届いたことの確認(openDialogCalls)に使う。
async function holdOpenDialog() {
  await browser.tauri.execute(() => {
    const w = window as unknown as {
      __e2eFileDialogPaths?: object;
      __e2eOpenControl?: { resolve: (path: string | null) => void };
      __e2eOpenCalls?: number;
    };
    w.__e2eOpenCalls = 0;
    const pending = new Promise<string | null>((resolve) => {
      w.__e2eOpenControl = { resolve };
    });
    const paths = {};
    Object.defineProperty(paths, "open", {
      enumerable: true,
      get() {
        w.__e2eOpenCalls = (w.__e2eOpenCalls ?? 0) + 1;
        return pending;
      },
    });
    w.__e2eFileDialogPaths = paths;
  });
}

async function openDialogCalls(): Promise<number> {
  return browser.tauri.execute(
    () => (window as unknown as { __e2eOpenCalls?: number }).__e2eOpenCalls ?? 0
  );
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

// 保留中のインポートが無いときの、確定のエラー文言(Rust側のcommit_pending_importと同じ)。
const NO_PENDING_IMPORT_MESSAGE = "確認待ちのインポートがありません";

// 保留の識別子を指定して、確定を直接呼ぶ。成功なら(実際に取り込まれる)null、失敗ならそのエラー文言を返す。
async function commitPendingImportError(pendingId: number): Promise<string | null> {
  return browser.tauri.execute(
    ({ core }, id) =>
      core.invoke("commit_pending_import", { pendingId: id }).then(
        () => null,
        (error) => String(error)
      ),
    pendingId
  );
}

// 保留の破棄を、直接呼ぶ。識別子を省略すると、全ての保留を破棄する。
async function clearPendingImportDirectly(pendingId?: number): Promise<void> {
  await browser.tauri.execute(
    ({ core }, id) => core.invoke("clear_pending_import", id == null ? {} : { pendingId: id }),
    pendingId
  );
}

// アプリ(E2Eビルド)が、復号の結果として受け取った保留の識別子を、受け取った順に返す。
async function receivedPendingImportIds(): Promise<number[]> {
  return browser.tauri.execute(
    () => (window as unknown as { __e2ePendingImportIds?: number[] }).__e2ePendingImportIds ?? []
  );
}

// 直近に受け取った保留の確定を、直接呼ぶ。保留が残っていなければ、NO_PENDING_IMPORT_MESSAGEで失敗する
// (成功すると、実際に取り込まれる)。復号の結果を受け取っていないと、「保留が残っていない」の確認が、何も
// 確かめずに通ってしまうため、その場で落とす。
async function commitLastReceivedPendingImportError(): Promise<string | null> {
  const ids = await receivedPendingImportIds();
  if (ids.length === 0) throw new Error("復号の結果を受け取っていない(保留の識別子が無い)");
  return commitPendingImportError(ids[ids.length - 1]);
}

// 1回の復号(scrypt)にかかる時間(ms)を、画面を介さず、IPCで直接測る。復号した保留は、その場で破棄する。
async function measureDecryptMs(sourcePath: string, passphrase: string): Promise<number> {
  return browser.tauri.execute(
    async ({ core }, p, s) => {
      const startedAt = performance.now();
      const result = (await core.invoke("preview_import", { sourcePath: p, passphrase: s })) as {
        pending_id: number;
      };
      const elapsedMs = performance.now() - startedAt;
      await core.invoke("clear_pending_import", { pendingId: result.pending_id });
      return elapsedMs;
    },
    sourcePath,
    passphrase
  );
}

// 画面を介さず、IPCで復号し、保留の識別子を返す(保留は、画面の状態に結び付かず、残る)。
async function previewPendingImportViaIpc(sourcePath: string, passphrase: string): Promise<number> {
  return browser.tauri.execute(
    async ({ core }, p, s) => {
      const result = (await core.invoke("preview_import", { sourcePath: p, passphrase: s })) as {
        pending_id: number;
      };
      return result.pending_id;
    },
    sourcePath,
    passphrase
  );
}

// 画面を介さず、IPCで復号を始め、結果を待たずに戻る(復号している最中に、ページを読み込み直すため)。
async function startPreviewImportViaIpc(sourcePath: string, passphrase: string): Promise<void> {
  await browser.tauri.execute(
    ({ core }, p, s) => {
      void core.invoke("preview_import", { sourcePath: p, passphrase: s }).catch(() => {});
    },
    sourcePath,
    passphrase
  );
}

// アサーションが落ちたとき、その失敗のメッセージの先頭に、原因の手がかりを付けて投げ直す
// (expectは、matcherごとのメッセージを引数に取れないため)。
function withHint(hint: string, assertion: () => unknown): void {
  try {
    assertion();
  } catch (error) {
    if (error instanceof Error) error.message = `${hint}\n${error.message}`;
    throw error;
  }
}

async function listProfileNames(): Promise<string[]> {
  return browser.tauri.execute(async ({ core }) => {
    const summaries = (await core.invoke("list_profiles")) as Array<{ name: string }>;
    return summaries.map((s) => s.name);
  });
}

async function currentUrl(): Promise<string> {
  return browser.tauri.execute(() => location.href);
}

async function focusIsInsideDialog(): Promise<boolean> {
  return browser.tauri.execute(() => !!document.activeElement?.closest('[role="dialog"]'));
}

// エクスポート完了の通知(読み上げの領域)。領域は書き出す前から存在して中身が空で、完了すると、
// 同じ領域に通知が入る(領域ごと後から現れると、スクリーンリーダーに読み上げられないことがある)。
const EXPORT_NOTICE_TEXT = "二度と表示できません";

async function exportNoticeIsEmpty(dialog: WebdriverIO.Element): Promise<boolean> {
  const notice = await dialog.$('[role="status"]');
  return (await notice.isExisting()) && (await notice.getProperty("textContent")) === "";
}

async function waitForExportNotice(dialog: WebdriverIO.Element) {
  const notice = await dialog.$('[role="status"]');
  await browser.waitUntil(async () => (await notice.getText()).includes(EXPORT_NOTICE_TEXT), {
    timeout: 10000,
    timeoutMsg: "エクスポート完了の通知が表示されなかった",
  });
  return notice;
}

// 片付けの1手順を実行する。片付けそのものの失敗は、テストの失敗にしない(元のテストの結果を覆い隠さない)。
async function bestEffort(step: () => Promise<unknown>) {
  try {
    await step();
  } catch {
    // 片付けは最善を尽くすだけ。
  }
}

// 失敗したテストが残した状態(保留中のダイアログ、開いたままの画面)を片付け、後続のテストへ
// 連鎖させない。成功したテストでは何も起きない。手順は互いに独立に行い、1つの失敗で残りを飛ばさない
// (メイン画面へ戻れないと、次のテストは「初期設定画面もメイン画面も表示されなかった」という別の症状で落ちる)。
afterEach(async () => {
  await bestEffort(() =>
    browser.tauri.execute(() => {
      const w = window as unknown as {
        __e2eFileDialogPaths?: unknown;
        __e2eSaveControl?: { resolve: (path: string | null) => void };
        __e2eOpenControl?: { resolve: (path: string | null) => void };
        __e2ePendingImportIds?: number[];
      };
      w.__e2eSaveControl?.resolve(null);
      w.__e2eOpenControl?.resolve(null);
      // 次のテストが、保留していないのにsettleしたとき、前のテストの参照に当たらず失敗するようにする。
      w.__e2eSaveControl = undefined;
      w.__e2eOpenControl = undefined;
      w.__e2eFileDialogPaths = undefined;
      // 次のテストの「直近に受け取った保留の識別子」が、前のテストのものにならないようにする。
      w.__e2ePendingImportIds = undefined;
    })
  );
  await bestEffort(async () => {
    for (let attempt = 0; attempt < 3; attempt++) {
      const openDialog = await $('[role="dialog"], [role="alertdialog"]');
      if (!(await openDialog.isExisting())) break;
      const closeButton = await openDialog.$("button=閉じる");
      if (await closeButton.isExisting()) await closeButton.click();
      else await browser.keys("Escape");
      await browser.pause(300);
    }
  });
  await bestEffort(async () => {
    const backButton = await $("button=閉じる(メイン画面へ)");
    if (await backButton.isExisting()) await backButton.click();
  });
  // 保留中の復号済みの内容(Rust側)を、次のテストへ持ち越さない。識別子を指定しない破棄は、全ての保留を消す。
  // 画面を離れる後始末が、既に破棄していれば、何も起きない。
  await bestEffort(() => browser.tauri.execute(({ core }) => core.invoke("clear_pending_import")));
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

let keeperSequence = 0;

// プロファイルを書き出し(書き出したファイルのパスフレーズを返す)、そのプロファイルを削除する
// (取り込み直せる状態にするため)。アクティブなプロファイルは削除できないため、別のプロファイルを
// アクティブにしておく。書き出しの後は、プロファイル管理画面が開いている。
async function exportAndDeleteProfile(profileName: string, filePath: string): Promise<string> {
  // 行の検索は、名前の一部の一致で行うため、書き出すプロファイルの名前を含まない名前にする。
  const keeperName = `E2E保持用${++keeperSequence}`;
  await createProfileViaIpc(keeperName);
  await setActiveProfileViaIpc(keeperName);
  await createProfileViaIpc(profileName);
  await setE2eFileDialogPaths({ save: filePath, open: filePath });

  const dialog = await openExportDialogFor(profileName);
  const passphrase = await (await dialog.$("input[readonly]")).getValue();
  await (await dialog.$("button=エクスポート")).click();
  await waitForExportNotice(dialog);
  await (await dialog.$("button=閉じる")).click();
  await dialog.waitForExist({ reverse: true, timeout: 10000 });
  await deleteProfileViaIpc(profileName);
  return passphrase;
}

// 復号して、内容の確認画面を開くところまで進める(取り込みは実行しない)。インポートのボタンがある画面
// (プロファイル管理画面・メイン画面)で呼ぶ。
async function openImportConfirmDialog(passphrase: string) {
  await (await $("button=インポート")).click();
  const importDialog = await $('[role="dialog"]');
  await importDialog.waitForExist({ timeout: 10000 });
  await (await importDialog.$("input")).setValue(passphrase);
  await (await importDialog.$("button=OK")).click();
  await (await $('[role="alertdialog"]')).waitForExist({ timeout: 15000 });
}

// 読み込んだファイルに、UTF-8として読めないバイト列があったときの警告(トースト)。
const INVALID_BYTES_WARNING_TEXT = "不正なバイト列があったため";

// 前面に確認画面などがあっても、背景にあるボタンを、JSのclick()で1回押す。押せない状態(無い・無効)は、
// 押したつもりで何も起きないことを避けるため、その場で失敗させる。
async function pressBackgroundButton(label: string) {
  await browser.tauri.execute((_tauri, buttonLabel) => {
    const button = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
      (b) => b.textContent?.trim() === buttonLabel
    );
    if (!button) throw new Error(`${buttonLabel}のボタンが見つからない`);
    if (button.disabled) throw new Error(`${buttonLabel}のボタンが無効になっている`);
    button.click();
  }, label);
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
    // 読み上げの領域は、書き出す前から存在し、中身は空である。
    const notice = await dialog.$('[role="status"]');
    expect(await exportNoticeIsEmpty(dialog)).toBe(true);

    // 「表示」にしてから実行しても、成功した時点で伏せ字へ戻る。
    await (await dialog.$('button[aria-label="パスフレーズを表示"]')).click();
    expect(await passphraseInput.getAttribute("type")).toBe("text");

    await (await dialog.$("button=エクスポート")).click();

    // 完了すると、書き出す前と同じ領域(作り直されたものではない)に、通知が入る。
    await browser.waitUntil(async () => (await notice.getText()).includes(EXPORT_NOTICE_TEXT), {
      timeout: 10000,
      timeoutMsg: "同じ領域にエクスポート完了の通知が入らなかった",
    });
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
    await waitForExportNotice(dialog);
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
    await waitForExportNotice(dialog);
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
    expect(await exportNoticeIsEmpty(dialog)).toBe(true);

    const exportPath = path.join(exportDir, "after-cancel.smx");
    await setE2eFileDialogPaths({ save: exportPath });
    await exportButton.click();
    await waitForExportNotice(dialog);
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
    expect(await exportNoticeIsEmpty(dialog)).toBe(true);
    expect(fs.existsSync(invalidPath)).toBe(false);

    const exportPath = path.join(exportDir, "after-failure.smx");
    await setE2eFileDialogPaths({ save: exportPath });
    await exportButton.click();
    await waitForExportNotice(dialog);
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
    expect(await exportNoticeIsEmpty(dialog)).toBe(true);

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
    await waitForExportNotice(dialog);
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
    await waitForExportNotice(dialog);
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
    await waitForExportNotice(dialog);

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
    await waitForExportNotice(dialog);
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
    await waitForExportNotice(exportDialog);
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

  it("インポートを実行すると、書き出して削除したプロファイルが、同じ名前で復元される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eインポート実行確認";
    // 削除するプロファイル以外をアクティブにしておく(アクティブなプロファイルは削除できない)。
    const activeProfileName = "E2E取込実行の保持プロファイル";
    await createProfileViaIpc(activeProfileName);
    await setActiveProfileViaIpc(activeProfileName);
    await createProfileViaIpc(profileName);
    const filePath = path.join(exportDir, "import-commit.smx");
    await setE2eFileDialogPaths({ save: filePath, open: filePath });

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    await (await dialog.$("button=エクスポート")).click();
    await waitForExportNotice(dialog);
    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await deleteProfileViaIpc(profileName);

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(passphrase);
    await (await importDialog.$("button=OK")).click();
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    await (await confirmDialog.$("button=インポート実行")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });

    // 取り込みが確定し、削除したプロファイルが一覧に戻る。戻らないときは、確定に失敗したのか、
    // 確定したのに画面へ反映されなかったのかを見分けられるよう、失敗の通知と、実際に登録されて
    // いるプロファイルの名前を添える(通知は数秒で消えるため、確定の直後に採取しておく)。
    const notices = await browser.tauri.execute(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1000));
      return Array.from(document.querySelectorAll("[data-sonner-toast]")).map((t) => t.textContent);
    });
    try {
      await $(
        `//div[contains(@class,"rounded-lg")][.//*[contains(text(),"${profileName}")]]`
      ).waitForExist({ timeout: 15000 });
    } catch (error) {
      const registered = await browser.tauri.execute(async ({ core }) => {
        const summaries = (await core.invoke("list_profiles")) as Array<{ name: string }>;
        return summaries.map((s) => s.name);
      });
      throw new Error(
        `インポートを実行しても、プロファイルが一覧に戻らなかった。通知: ${JSON.stringify(notices)}、` +
          `登録されているプロファイル: ${JSON.stringify(registered)}`,
        { cause: error }
      );
    }
    await returnToMainScreen();
  });

  it("再生成した後に書き出したファイルは、再生成後のパスフレーズでだけインポートできる", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2E再生成往復確認");
    const filePath = path.join(exportDir, "regenerated.smx");
    await setE2eFileDialogPaths({ save: filePath, open: filePath });

    await openProfileManagement();
    await (await $("button=全体エクスポート")).click();
    const exportDialog = await $('[role="dialog"]');
    await exportDialog.waitForExist({ timeout: 10000 });
    const passphraseInput = await exportDialog.$("input[readonly]");
    const initial = await passphraseInput.getValue();
    await (await exportDialog.$("button=再生成")).click();
    await browser.waitUntil(async () => (await passphraseInput.getValue()) !== initial, {
      timeout: 10000,
      timeoutMsg: "再生成してもパスフレーズが変わらなかった",
    });
    const regenerated = await passphraseInput.getValue();
    await (await exportDialog.$("button=エクスポート")).click();
    await waitForExportNotice(exportDialog);
    await (await exportDialog.$("button=閉じる")).click();
    await exportDialog.waitForExist({ reverse: true, timeout: 10000 });

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    const field = await importDialog.$("input");

    // 再生成する前のパスフレーズでは、復号に失敗する。
    await field.setValue(initial);
    await (await importDialog.$("button=OK")).click();
    const error = await importDialog.$('[role="alert"]');
    await error.waitForExist({ timeout: 15000 });
    expect(await error.getText()).toContain("復号に失敗しました");

    // 再生成した後のパスフレーズなら、内容の確認画面へ進む(取り込みは実行せず取り消す)。
    await field.setValue(regenerated);
    await (await importDialog.$("button=OK")).click();
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    expect(await confirmDialog.getText()).toContain("インポート内容の確認");
    await (await confirmDialog.$("button=キャンセル")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("メイン画面のインポートでも、インポートを実行すると、書き出して削除したプロファイルが復元される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eメイン画面取込確認";
    // 削除するプロファイル以外をアクティブにしておく(アクティブなプロファイルは削除できない)。
    const activeProfileName = "E2Eメイン画面取込の保持プロファイル";
    await createProfileViaIpc(activeProfileName);
    await setActiveProfileViaIpc(activeProfileName);
    await createProfileViaIpc(profileName);
    const filePath = path.join(exportDir, "main-screen-import.smx");
    await setE2eFileDialogPaths({ save: filePath, open: filePath });

    const dialog = await openExportDialogFor(profileName);
    const passphrase = await (await dialog.$("input[readonly]")).getValue();
    await (await dialog.$("button=エクスポート")).click();
    await waitForExportNotice(dialog);
    await (await dialog.$("button=閉じる")).click();
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    await deleteProfileViaIpc(profileName);
    await returnToMainScreen();

    // メイン画面の「インポート」から、同じ流れで確定する。
    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(passphrase);
    await (await importDialog.$("button=OK")).click();
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    await (await confirmDialog.$("button=インポート実行")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });

    await browser.waitUntil(async () => (await listProfileNames()).includes(profileName), {
      timeout: 15000,
      timeoutMsg: "メイン画面のインポートを実行しても、プロファイルが復元されなかった",
    });
  });

  it("画面を閉じる操作と同じ瞬間にエクスポートを押しても、書き出しは実行されない", async () => {
    await completeInitialSetup();
    const profileName = "E2Eエクスポート閉じる直後実行確認";
    await createProfileViaIpc(profileName);
    const dialog = await openExportDialogFor(profileName);
    await holdSaveDialogCounting();

    // 1回のJSタスクの中で、キャンセルとエクスポートを続けて押す(再描画される前なので、2つ目の押下は、
    // 閉じる前の画面を見る)。閉じた画面のパスフレーズを、誰も見ないまま書き出してはならない。
    await browser.tauri.execute(() => {
      const buttons = Array.from(document.querySelectorAll('[role="dialog"] button'));
      const find = (label: string) =>
        buttons.find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
      const cancel = find("キャンセル");
      const exportButton = find("エクスポート");
      if (!cancel || !exportButton) throw new Error("ボタンが見つからない");
      cancel.click();
      exportButton.click();
    });
    await dialog.waitForExist({ reverse: true, timeout: 10000 });
    expect(await saveDialogCalls()).toBe(0);
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
    await waitForExportNotice(dialog);

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
    await waitForExportNotice(exportDialog);
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
    // キャンセルすると、保留中の(復号済みの)内容は破棄され、その後に確定を呼んでも成立しない。
    expect(await commitLastReceivedPendingImportError()).toBe(NO_PENDING_IMPORT_MESSAGE);
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
    await waitForExportNotice(exportDialog);
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

  it("復号している間は、OKを押せず、入力も変えられず、その理由が示され、結果が届くと確認画面へ進む", async () => {
    await completeInitialSetup();
    const profileName = "E2Eインポート復号中確認";
    const passphrase = await exportAndDeleteProfile(
      profileName,
      path.join(exportDir, "decrypting.smx")
    );

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(passphrase);

    // OKを押した直後(復号の結果が届く前)の状態を、同じJSタスクの中で確かめる(復号にかかる時間に頼らない)。
    const whileDecrypting = await browser.tauri.execute(async () => {
      const dialog = document.querySelector('[role="dialog"]');
      const ok = Array.from(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? []).find(
        (b) => b.textContent?.trim() === "OK"
      );
      const input = dialog?.querySelector<HTMLInputElement>("input");
      if (!ok || !input) throw new Error("OKまたは入力欄が見つからない");
      // 実際のクリックと同じく、押したボタンにフォーカスがある状態から始める(JSのclick()は、フォーカスを移さない)。
      ok.focus();
      ok.click();
      await new Promise((resolve) => setTimeout(resolve, 0));
      return {
        disabled: ok.disabled,
        busy: ok.getAttribute("aria-busy"),
        readOnly: input.readOnly,
        // 押したボタンが無効になっても、フォーカスは画面の内側(入力欄)に留まる(Tabで背景へ出られないように)。
        inputFocused: document.activeElement === input,
        status: dialog?.querySelector('[role="status"]')?.textContent ?? null,
      };
    });
    expect(whileDecrypting).toEqual({
      disabled: true,
      busy: "true",
      readOnly: true,
      inputFocused: true,
      status: "復号しています…",
    });

    // 結果が届くと、内容の確認画面へ進む(取り込みは実行せず取り消す)。
    const confirmDialog = await $('[role="alertdialog"]');
    await confirmDialog.waitForExist({ timeout: 15000 });
    expect(await confirmDialog.getText()).toContain(profileName);
    await (await confirmDialog.$("button=キャンセル")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("復号している間にインポートの画面を閉じると、結果が届いても確認画面は出ず、復号済みの内容は破棄される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eインポート復号中閉じる確認";
    const passphrase = await exportAndDeleteProfile(
      profileName,
      path.join(exportDir, "decrypting-close.smx")
    );

    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(passphrase);

    // OKを押すのと同じJSタスクの流れの中で閉じ、続けて、同じファイルを開き直す(復号の結果が届くより前に、必ず行う。
    // WebDriverの往復を挟むと、復号が先に終わりうる)。開き直した時点の状態も、その場で読む。
    const reopenedState = await browser.tauri.execute(async () => {
      const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
      const openDialogs = () =>
        Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"][data-state="open"]'));
      const buttonNamed = (root: ParentNode, label: string) =>
        Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find(
          (b) => b.textContent?.trim() === label
        );
      const waitUntil = async (condition: () => boolean, what: string) => {
        for (let i = 0; i < 300 && !condition(); i++) await sleep(10);
        if (!condition()) throw new Error(`${what}が起きなかった`);
      };

      const first = openDialogs()[0];
      const ok = first && buttonNamed(first, "OK");
      const cancel = first && buttonNamed(first, "キャンセル");
      if (!ok || !cancel) throw new Error("ボタンが見つからない");
      ok.click();
      cancel.click();
      await waitUntil(() => openDialogs().length === 0, "画面が閉じること");

      const importButton = buttonNamed(document.body, "インポート");
      if (!importButton) throw new Error("インポートのボタンが見つからない");
      importButton.click();
      await waitUntil(() => openDialogs().length === 1, "画面が開き直されること");
      const input = openDialogs()[0].querySelector<HTMLInputElement>("input");
      if (!input) throw new Error("入力欄が見つからない");
      return { readOnly: input.readOnly };
    });
    // 開き直した時点で、古い復号がまだ終わっていない(この検証の前提。終わっていたら、ここで落とす)。
    expect(reopenedState).toEqual({ readOnly: true });

    // 復号が終わるまでは、開き直した画面も、入力できない状態のまま(復号の終了を待てる)。
    const reopened = await $('[role="dialog"]');
    const reopenedInput = await reopened.$("input");
    await browser.waitUntil(async () => !(await reopenedInput.getProperty("readOnly")), {
      timeout: 15000,
      timeoutMsg: "復号が終わらなかった(入力できる状態へ戻らなかった)",
    });

    // 結果が届いても、開き直した画面は(別の画面の結果なので)確認画面へ進まず、エラーも出ない。復号済みの内容も、
    // Rust側に残っていない(残っていると、確定を呼ぶだけで取り込まれてしまう)。
    expect(await $('[role="alertdialog"]').isExisting()).toBe(false);
    expect(await reopened.$('[role="alert"]').isExisting()).toBe(false);
    expect(await commitLastReceivedPendingImportError()).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(profileName);

    await (await reopened.$("button=キャンセル")).click();
    await reopened.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("インポートのファイルの選択を待つ間に開かれたエクスポート画面を、置き換えない", async () => {
    await completeInitialSetup();
    const profileName = "E2Eインポート選択中確認";
    await createProfileViaIpc(profileName);
    // パスフレーズ入力画面を開くだけなので、指すファイルは実在しなくてよい。
    const filePath = path.join(exportDir, "picking.smx");
    await openProfileManagement();

    // 対照: エクスポート画面を開かなければ、選択が終わった時点で、パスフレーズの入力画面が出る。
    await holdOpenDialog();
    await (await $("button=インポート")).click();
    await settleOpenDialog(filePath);
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    expect(await importDialog.getText()).toContain("パスフレーズを入力");
    await (await importDialog.$("button=キャンセル")).click();
    await importDialog.waitForExist({ reverse: true, timeout: 10000 });

    // ファイルの選択を待つ間にエクスポート画面を開くと、選択が終わっても、その画面のまま残る。
    await holdOpenDialog();
    await (await $("button=インポート")).click();
    const exportDialog = await openExportDialogFromRow(profileName);
    await settleOpenDialog(filePath);
    await browser.pause(1500);
    expect(await exportDialog.isDisplayed()).toBe(true);
    expect(await exportDialog.getText()).toContain("エクスポート: " + profileName);

    await (await exportDialog.$("button=キャンセル")).click();
    await exportDialog.waitForExist({ reverse: true, timeout: 10000 });
    await returnToMainScreen();
  });

  it("プロファイル管理画面で確認画面を開いたまま画面を離れると、復号済みの内容(Rust側の保留)は破棄される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eインポート確認中離脱確認";
    const passphrase = await exportAndDeleteProfile(
      profileName,
      path.join(exportDir, "leaving.smx")
    );

    // 開いたまま、履歴で戻る(メイン画面へ移る)。
    await openImportConfirmDialog(passphrase);
    await browser.back();
    await (await $("button*=マスク実行")).waitForExist({ timeout: 10000 });
    await browser.pause(1000);
    expect(await commitLastReceivedPendingImportError()).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(profileName);
  });

  it("メイン画面で確認画面を開いたまま画面を離れると、復号済みの内容(Rust側の保留)は破棄される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eメイン画面確認中離脱確認";
    const passphrase = await exportAndDeleteProfile(
      profileName,
      path.join(exportDir, "leaving-main.smx")
    );
    await returnToMainScreen();

    // 開いたまま、履歴で戻る(プロファイル管理画面へ移る)。
    await openImportConfirmDialog(passphrase);
    await browser.back();
    await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });
    await browser.pause(1000);
    expect(await commitLastReceivedPendingImportError()).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(profileName);
    await returnToMainScreen();
  });

  it("復号している間に画面を離れても、結果が届いた時に、復号済みの内容(Rust側の保留)は破棄される", async () => {
    await completeInitialSetup();
    const profileName = "E2Eインポート復号中離脱確認";
    const passphrase = await exportAndDeleteProfile(
      profileName,
      path.join(exportDir, "decrypting-leave.smx")
    );

    // この環境で、復号にかかる時間を測る(結果が届くのを待つ時間を、固定値に頼らず決めるため)。
    await (await $("button=インポート")).click();
    const measuring = await $('[role="dialog"]');
    await measuring.waitForExist({ timeout: 10000 });
    await (await measuring.$("input")).setValue(passphrase);
    const startedAt = Date.now();
    await (await measuring.$("button=OK")).click();
    const measured = await $('[role="alertdialog"]');
    await measured.waitForExist({ timeout: 15000 });
    const decryptMs = Date.now() - startedAt;
    await (await measured.$("button=キャンセル")).click();
    await measured.waitForExist({ reverse: true, timeout: 10000 });

    // OKを押すのと同じJSタスクの中で、履歴を戻す(復号の結果が届くより前に、この画面が破棄される)。
    await (await $("button=インポート")).click();
    const importDialog = await $('[role="dialog"]');
    await importDialog.waitForExist({ timeout: 10000 });
    await (await importDialog.$("input")).setValue(passphrase);
    await browser.tauri.execute(() => {
      const ok = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')).find(
        (b) => b.textContent?.trim() === "OK"
      );
      if (!ok) throw new Error("OKのボタンが見つからない");
      ok.click();
      window.history.back();
    });
    await (await $("button*=マスク実行")).waitForExist({ timeout: 10000 });

    // 復号が終わるまで待っても、確認画面は出ず、復号済みの内容も残っていない。この画面を離れた後の復号の結果
    // (2件目)が、アプリに届いていること(届いていないと、1件目の識別子で確かめてしまう)を、先に確かめる。
    await browser.pause(Math.max(3000, decryptMs * 3));
    expect(await receivedPendingImportIds()).toHaveLength(2);
    expect(await $('[role="alertdialog"]').isExisting()).toBe(false);
    expect(await commitLastReceivedPendingImportError()).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(profileName);
  });

  it("保留は、識別子ごとに独立している(IPCを直接呼ぶ): 別の識別子の確定・破棄は、他の保留に作用せず、識別子を指定しない破棄は、全ての保留を消す", async () => {
    await completeInitialSetup();
    const nameX = "E2E識別子確認X";
    const nameY = "E2E識別子確認Y";
    const fileX = path.join(exportDir, "pending-id-x.smx");
    const fileY = path.join(exportDir, "pending-id-y.smx");
    const passphraseX = await exportAndDeleteProfile(nameX, fileX);
    await returnToMainScreen();
    const passphraseY = await exportAndDeleteProfile(nameY, fileY);

    // 復号は、画面を介さず、IPCで直接行う(戻り値の形と、引数の名前を、実アプリで確かめる)。
    const previewDirectly = (sourcePath: string, passphrase: string) =>
      browser.tauri.execute(
        async ({ core }, p, s) =>
          (await core.invoke("preview_import", { sourcePath: p, passphrase: s })) as {
            pending_id: number;
            preview: { kind: string; name: string };
          },
        sourcePath,
        passphrase
      );
    const previewX = await previewDirectly(fileX, passphraseX);
    const previewY = await previewDirectly(fileY, passphraseY);
    expect(previewX.preview.kind).toBe("single");
    expect(previewX.preview.name).toBe(nameX);
    expect(previewY.preview.name).toBe(nameY);
    expect(previewX.pending_id).not.toBe(previewY.pending_id);

    // 払い出されていない識別子の確定は失敗し、どちらの保留も消費しない。
    const unusedId = Math.max(previewX.pending_id, previewY.pending_id) + 1000;
    expect(await commitPendingImportError(unusedId)).toBe(NO_PENDING_IMPORT_MESSAGE);
    // Xの識別子だけを破棄すると、Yの保留は残り、Yの識別子で確定すると、Yの内容だけが取り込まれる。
    await clearPendingImportDirectly(previewX.pending_id);
    expect(await commitPendingImportError(previewX.pending_id)).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await commitPendingImportError(previewY.pending_id)).toBeNull();
    const namesAfterCommit = await listProfileNames();
    expect(namesAfterCommit).toContain(nameY);
    expect(namesAfterCommit).not.toContain(nameX);

    // 識別子を指定しない破棄は、全ての保留を消す(Xは、まだ取り込まれていないため、もう一度、復号できる)。
    const againFirst = await previewDirectly(fileX, passphraseX);
    const againSecond = await previewDirectly(fileX, passphraseX);
    await clearPendingImportDirectly();
    expect(await commitPendingImportError(againFirst.pending_id)).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await commitPendingImportError(againSecond.pending_id)).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(nameX);
  });

  it("別の画面で復号が重なり、古い復号の結果が後から届いても、新しい画面の確認画面と確定は、影響を受けない", async () => {
    await completeInitialSetup();
    // 書き出す2つのファイル。どちらを古い画面(プロファイル管理画面)が、どちらを新しい画面(メイン画面)が復号するかは、
    // 復号の所要時間を測ってから決める。
    const nameA = "E2E復号重複確認甲";
    const nameB = "E2E復号重複確認乙";
    const fileA = path.join(exportDir, "overlap-a.smx");
    const fileB = path.join(exportDir, "overlap-b.smx");
    const passphraseA = await exportAndDeleteProfile(nameA, fileA);
    await returnToMainScreen();
    const passphraseB = await exportAndDeleteProfile(nameB, fileB);

    // 1回の復号にかかる時間は、ファイルごとに違う(ageは、書き出すたびに、scryptの作業係数を較正するため、2つのファイルで
    // 1段(約2倍)ずれることがある)。古い画面は、前後に空白が混ざったパスフレーズを入力し、入力どおりの復号に失敗してから、
    // 空白を除いて再試行するため、復号に、その約2倍(2×S_old)かかる。新しい画面の復号は、古い復号を始めた後、少し遅れて始まり、
    // 1回(S_new)で終わる。1回の復号が遅い方のファイルを古い画面に割り当てると、S_old ≥ S_new のため、
    // 2×S_old ≥ 2×S_new > S_new + (新しい復号の開始の遅れ) となり(S_newが、その遅れより長ければ)、作業係数のずれに関わらず、
    // 新しい画面の確認画面が出た後に、古い復号の結果が届く。
    // 測定は、画面を介さず、IPCで直接行い、ファイルごとに2回(交互)測って小さい方を採る(他の処理と重なって遅くなった1回に、引きずられないため)。
    const measuredA: number[] = [];
    const measuredB: number[] = [];
    for (let round = 0; round < 2; round++) {
      measuredA.push(await measureDecryptMs(fileA, passphraseA));
      measuredB.push(await measureDecryptMs(fileB, passphraseB));
    }
    const sideA = { name: nameA, file: fileA, passphrase: passphraseA, singleMs: Math.min(...measuredA) };
    const sideB = { name: nameB, file: fileB, passphrase: passphraseB, singleMs: Math.min(...measuredB) };
    const [oldSide, newSide] = sideA.singleMs >= sideB.singleMs ? [sideA, sideB] : [sideB, sideA];
    const { name: oldName, file: oldFile, passphrase: oldPassphrase } = oldSide;
    const { name: newName, file: newFile, passphrase: newPassphrase } = newSide;

    // 古い画面: 前後に空白が混ざったパスフレーズを入力する(復号に、1回の約2倍かかる)。
    await setE2eFileDialogPaths({ open: oldFile });
    await (await $("button=インポート")).click();
    const oldDialog = await $('[role="dialog"]');
    await oldDialog.waitForExist({ timeout: 10000 });
    await (await oldDialog.$("input")).setValue(`  ${oldPassphrase}  `);
    // 新しい画面が開くファイルを、差し替える(古い画面は、既に開いている)。
    await setE2eFileDialogPaths({ open: newFile });

    // 古い復号を始めた同じJSタスクの中で、古い画面を離れ、新しい画面で復号を始め、その確認画面が開くまで進める
    // (WebDriverの往復を挟むと、新しい復号を始めるのが遅れ、古い復号の方が先に終わりうる)。
    const newScreen = await browser.tauri.execute(async (_tauri, passphrase) => {
      const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
      const openDialogs = () =>
        Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"][data-state="open"]'));
      const buttonNamed = (root: ParentNode, label: string, exact = true) =>
        Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find((b) =>
          exact ? b.textContent?.trim() === label : b.textContent?.includes(label)
        );
      const waitUntil = async (condition: () => boolean, what: string, timeoutMs = 3000) => {
        for (let waited = 0; waited < timeoutMs && !condition(); waited += 10) await sleep(10);
        if (!condition()) throw new Error(`${what}が起きなかった`);
      };

      const oldOk = openDialogs()[0] && buttonNamed(openDialogs()[0], "OK");
      if (!oldOk) throw new Error("古い画面のOKのボタンが見つからない");
      const oldStartedAt = performance.now();
      oldOk.click();
      window.history.back();
      await waitUntil(
        () => openDialogs().length === 0 && !!buttonNamed(document.body, "マスク実行", false),
        "古い画面を離れて、メイン画面へ移ること"
      );

      const importButton = buttonNamed(document.body, "インポート");
      if (!importButton) throw new Error("インポートのボタンが見つからない");
      importButton.click();
      await waitUntil(() => openDialogs().length === 1, "新しい画面のパスフレーズ入力画面が開くこと");
      const input = openDialogs()[0].querySelector<HTMLInputElement>("input");
      if (!input) throw new Error("入力欄が見つからない");
      // Reactの制御された入力欄へ、値を入れる。
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, passphrase);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await waitUntil(() => {
        const ok = buttonNamed(openDialogs()[0], "OK");
        return !!ok && !ok.disabled;
      }, "OKが押せる状態になること");
      buttonNamed(openDialogs()[0], "OK")?.click();
      const newStartedMs = performance.now() - oldStartedAt;

      const confirmSelector = '[role="alertdialog"][data-state="open"]';
      await waitUntil(() => document.querySelector(confirmSelector) !== null, "確認画面が開くこと", 15000);
      return {
        confirmText: document.querySelector(confirmSelector)?.textContent ?? "",
        receivedIds:
          (window as unknown as { __e2ePendingImportIds?: number[] }).__e2ePendingImportIds ?? [],
        newStartedMs,
        confirmShownMs: performance.now() - oldStartedAt,
      };
    }, newPassphrase);
    // この検証の前提: 新しい画面の確認画面が出た時点で、古い復号の結果は、まだ届いていない(受け取った識別子は、
    // 新しい画面の1件だけ)。届いていたら、ここで落とす(古い結果が、後から届く場面を、再現できていない)。
    withHint(
      [
        "前提が崩れた: 新しい画面の確認画面が出た時点で、古い復号の結果が、既に届いていた。",
        `1回の復号にかかった時間(測定の小さい方): 古い画面のファイル ${Math.round(oldSide.singleMs)}ms、` +
          `新しい画面のファイル ${Math.round(newSide.singleMs)}ms(古い画面は、空白の再試行で、その約2倍かかる)。`,
        `古い復号の開始から、新しい復号の開始まで ${Math.round(newScreen.newStartedMs)}ms、` +
          `新しい確認画面が出るまで ${Math.round(newScreen.confirmShownMs)}ms。`,
        "新しい復号の開始や確認画面の表示が、古い復号の所要時間(1回の測定値の約2倍)より遅れていないか、",
        "古い画面のファイルの方が、1回の復号が遅くなる割り当てになっているか(測定が他の処理の影響を受けていないか)を確かめる。",
      ].join("\n"),
      () => expect(newScreen.receivedIds).toHaveLength(1)
    );
    expect(newScreen.confirmText).toContain(newName);

    // 古い復号の結果が、離れた画面へ、後から届く。
    await browser.waitUntil(async () => (await receivedPendingImportIds()).length === 2, {
      timeout: 20000,
      timeoutMsg: "古い復号の結果が届かなかった",
    });
    const [newId, oldId] = await receivedPendingImportIds();
    expect(newId).not.toBe(oldId);
    // 古い画面の後始末(届いた結果の保留だけの破棄)が終わるまで待つ。
    await browser.pause(1500);

    // 新しい画面の確認画面は、影響を受けず、開いたまま、新しい画面のファイルの内容を示している。
    const confirmDialog = await $('[role="alertdialog"]');
    expect(await confirmDialog.isDisplayed()).toBe(true);
    const confirmText = await confirmDialog.getText();
    expect(confirmText).toContain(newName);
    expect(confirmText).not.toContain(oldName);

    // 確定すると、確認画面に出ていた、新しい画面のファイルの内容だけが取り込まれる。
    await (await confirmDialog.$("button=インポート実行")).click();
    await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
    await browser.waitUntil(async () => (await listProfileNames()).includes(newName), {
      timeout: 10000,
      timeoutMsg: "新しい画面のファイルの内容が取り込まれなかった",
    });
    // 古い復号の結果の保留は、破棄されている(確定を呼んでも、取り込まれない)。
    expect(await commitPendingImportError(oldId)).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(oldName);
  });

  // 確認画面を開いたまま、背景にあるボタンを押し(ファイルの選択・読み込みを保留し)、決着させても、確認画面の
  // まま残る(選択・読み込みを待つ間に、画面が変わる場合を再現する)。入口ごとに独立したテストにする。
  // 読み込みを伴う入口(envインポート・ファイルから)は、読み込んだ内容を使わないので、不正なバイト列の警告も出さない。
  // 「置き換えられなかった」が、押下が入口の処理まで届かなかったせいで成り立たないよう、確認画面が無い状態の
  // 同じ操作(対照)で、その入口の画面が開くことを先に確かめる。
  const confirmDialogGuardCases = [
    { route: "profiles", label: "インポート", opens: "パスフレーズを入力", readsFile: false },
    { route: "profiles", label: "envインポート", opens: "取り込み対象を選択", readsFile: true },
    { route: "main", label: "インポート", opens: "パスフレーズを入力", readsFile: false },
    { route: "main", label: "ファイルから", opens: "ファイル取り込み方法の選択", readsFile: true },
  ] as const;
  confirmDialogGuardCases.forEach(({ route, label, opens, readsFile }, index) => {
    const screenName = route === "profiles" ? "プロファイル管理画面" : "メイン画面";
    it(`確認画面が開かれている間は、ファイルの選択・読み込みが終わっても、「${label}」は、その画面を置き換えない(${screenName})`, async () => {
      await completeInitialSetup();
      const profileName = `E2E確認中選択待ち確認${index}`;
      const smxPath = path.join(exportDir, `guard-confirm-${index}.smx`);
      const passphrase = await exportAndDeleteProfile(profileName, smxPath);
      // 内容は架空の値だけにする。末尾の2バイト(0xFF 0xFE)は、UTF-8として読めない(読み込みの警告を出させるため)。
      const textPath = path.join(exportDir, `guard-confirm-${index}.env`);
      fs.writeFileSync(
        textPath,
        Buffer.concat([
          Buffer.from("API_TOKEN=dummy-token-value-0002\n"),
          Buffer.from([0xff, 0xfe]),
          Buffer.from("\n"),
        ])
      );
      if (route === "main") await returnToMainScreen();
      const invalidBytesWarning = await $(`div*=${INVALID_BYTES_WARNING_TEXT}`);

      // 対照: 確認画面が開かれていなければ、選択・読み込みが終わった時点で、その入口の画面が開く。読み込みを
      // 伴う入口では、警告も出る。開くまでの時間を測り、後で「置き換えられない」ことを確かめるまでの待ちの目安にする。
      await holdOpenDialog();
      await pressBackgroundButton(label);
      expect(await openDialogCalls()).toBe(1);
      const settledAt = Date.now();
      await settleOpenDialog(textPath);
      const opened = await $('[role="dialog"]');
      await opened.waitForExist({ timeout: 10000 });
      const openedAfterMs = Date.now() - settledAt;
      expect(await opened.getText()).toContain(opens);
      if (readsFile) await invalidBytesWarning.waitForExist({ timeout: 5000 });
      await (await opened.$("button=キャンセル")).click();
      await opened.waitForExist({ reverse: true, timeout: 10000 });
      // 対照の警告(数秒表示される)が消えてから進む(次の「警告が出ていない」の確認と混ざらないように)。
      if (readsFile) await invalidBytesWarning.waitForExist({ reverse: true, timeout: 15000 });

      // 確認画面を開いたまま、同じ操作をする。
      await setE2eFileDialogPaths({ save: smxPath, open: smxPath });
      await openImportConfirmDialog(passphrase);
      await holdOpenDialog();
      await pressBackgroundButton(label);
      expect(await openDialogCalls()).toBe(1);
      await settleOpenDialog(textPath);
      await browser.pause(Math.max(1500, openedAfterMs * 3));
      expect(await $('[role="alertdialog"]').isDisplayed()).toBe(true);
      expect(await $('[role="dialog"]').isExisting()).toBe(false);
      expect(await invalidBytesWarning.isExisting()).toBe(false);

      // 確認画面を取り消すと、保留も破棄される。
      const confirmDialog = await $('[role="alertdialog"]');
      await (await confirmDialog.$("button=キャンセル")).click();
      await confirmDialog.waitForExist({ reverse: true, timeout: 10000 });
      expect(await commitLastReceivedPendingImportError()).toBe(NO_PENDING_IMPORT_MESSAGE);
      if (route === "profiles") await returnToMainScreen();
    });
  });

  // Rust側の保留は、メインウィンドウのページの読み込みが始まったときに、全て破棄される(ページを読み込み直すと、
  // 保留の識別子を持つ画面が無くなるため)。ページ内の画面遷移(履歴による切り替え)は、ページの読み込みを起こさず、
  // 破棄されない。
  it("ページ内の画面遷移では、ページの読み込みに伴う保留の破棄は起きない(所有する画面が居ない保留は、残る)", async () => {
    await completeInitialSetup();
    const profileName = "E2E画面遷移保留確認";
    const smxPath = path.join(exportDir, "route-keeps-pending.smx");
    const passphrase = await exportAndDeleteProfile(profileName, smxPath);

    const pendingId = await previewPendingImportViaIpc(smxPath, passphrase);
    // 画面を行き来する(プロファイル管理画面 → メイン画面 → プロファイル管理画面)。
    await returnToMainScreen();
    await openProfileManagement();

    // 画面の遷移の後も、保留は残っていて、確定できる(取り込まれる)。
    expect(await commitPendingImportError(pendingId)).toBeNull();
    expect(await listProfileNames()).toContain(profileName);

    await deleteProfileViaIpc(profileName);
    await returnToMainScreen();
  });

  it("ページを読み込み直すと、復号済みの内容(Rust側の保留)は破棄される", async () => {
    await completeInitialSetup();
    const profileName = "E2E再読み込み保留確認";
    const smxPath = path.join(exportDir, "reload-discards-pending.smx");
    const passphrase = await exportAndDeleteProfile(profileName, smxPath);
    await returnToMainScreen();

    const pendingId = await previewPendingImportViaIpc(smxPath, passphrase);
    await browser.refresh();
    await completeInitialSetup();

    expect(await commitPendingImportError(pendingId)).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(profileName);
  });

  it("確認画面を開いたままページを読み込み直すと、確認画面で得た保留は、読み込み直しの後に確定できない", async () => {
    await completeInitialSetup();
    const profileName = "E2E確認中再読み込み確認";
    const passphrase = await exportAndDeleteProfile(profileName, path.join(exportDir, "reload-with-confirm.smx"));
    await returnToMainScreen();

    await openImportConfirmDialog(passphrase);
    const ids = await receivedPendingImportIds();
    expect(ids).toHaveLength(1);
    await browser.refresh();
    await completeInitialSetup();

    // 読み込み直した画面は、確認画面を持たない。確認画面が持っていた保留は、確定できない。
    expect(await $('[role="alertdialog"]').isExisting()).toBe(false);
    expect(await commitPendingImportError(ids[0])).toBe(NO_PENDING_IMPORT_MESSAGE);
    expect(await listProfileNames()).not.toContain(profileName);
  });

  it("復号している最中にページを読み込み直しても、その復号の結果は、保留として残らない", async () => {
    await completeInitialSetup();
    const profileName = "E2E復号中再読み込み確認";
    const smxPath = path.join(exportDir, "reload-while-decrypting.smx");
    const passphrase = await exportAndDeleteProfile(profileName, smxPath);
    await returnToMainScreen();

    // 前後に空白のあるパスフレーズは、入力どおりで復号に失敗してから再試行するため、復号が約2倍かかる。この復号が
    // 成功することと、かかる時間を、先に測る(下の、読み込み直しを要求する時点で、復号が終わっていないことの確認に使う)。
    const paddedPassphrase = ` ${passphrase} `;
    const decryptMs = await measureDecryptMs(smxPath, paddedPassphrase);
    // 次に払い出される識別子を知る(払い出して、破棄する)。
    const idBefore = await previewPendingImportViaIpc(smxPath, passphrase);
    await clearPendingImportDirectly(idBefore);

    const startedAt = Date.now();
    await startPreviewImportViaIpc(smxPath, paddedPassphrase);
    await browser.refresh();
    const reloadRequestedMs = Date.now() - startedAt;
    await completeInitialSetup();
    // 前提: 読み込み直しの要求は、復号が終わるよりずっと前に出ている(復号が終わった後に読み込みが始まると、結果は、
    // ページの世代の保護とは無関係に、読み込みの開始で消えるため、この確認は何も確かめない)。
    withHint(
      `復号(${Math.round(decryptMs)}ms)が終わる前に、読み込み直しを要求できなかった(${reloadRequestedMs}ms)`,
      () => expect(reloadRequestedMs).toBeLessThan(decryptMs / 2)
    );
    // 読み込み直す前に始めた復号が、終わるまで待つ。
    await browser.pause(Math.max(3000, decryptMs * 2));

    // 復号の結果を捨てていれば、その復号は識別子を払い出さないので、次に払い出される識別子は idBefore + 1 になる。
    // 結果を保留していれば(読み込みの開始の前後どちらでも)、その復号が識別子 idBefore + 1 を払い出し、次の識別子は
    // idBefore + 2 になる。
    const nextId = await previewPendingImportViaIpc(smxPath, passphrase);
    withHint(`次に払い出された識別子は ${nextId}(復号中の結果を捨てていれば ${idBefore + 1})`, () =>
      expect(nextId).toBe(idBefore + 1)
    );
    expect(await listProfileNames()).not.toContain(profileName);
    await clearPendingImportDirectly(nextId);
  });
});
