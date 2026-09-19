import { describe, expect, it } from "vitest";
import {
  createImportPassphraseHandlers,
  type ImportPassphraseDeps,
} from "./import-passphrase-handlers";

// 決着を、テストから握れるPromise。
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const TARGET = { sourcePath: "C:/dummy/export.smx", passphrase: "dummy-passphrase-0001" };
const PREVIEW_CALL = `preview:${TARGET.sourcePath}:${TARGET.passphrase}`;

// 呼び出された操作を、順に記録する依存を作る。
function setup(overrides: Partial<ImportPassphraseDeps<string>> = {}) {
  const calls: string[] = [];
  const decrypting = { current: false };
  const decryption = deferred<string>();
  const deps: ImportPassphraseDeps<string> = {
    target: () => TARGET,
    preview: (sourcePath, passphrase) => {
      calls.push(`preview:${sourcePath}:${passphrase}`);
      return decryption.promise;
    },
    isStillOpen: () => true,
    showConfirm: (preview) => calls.push(`showConfirm:${preview}`),
    showError: (error) => calls.push(`showError:${(error as Error).message}`),
    discardPending: () => calls.push("discardPending"),
    onBusyChange: (busy) => calls.push(`busy:${busy}`),
    ...overrides,
  };
  return { calls, decrypting, decryption, handlers: createImportPassphraseHandlers(deps, decrypting) };
}

describe("createImportPassphraseHandlers", () => {
  it("復号できたら、確認画面へ進み、復号している間だけ待ちになる", async () => {
    const { calls, decryption, handlers } = setup();

    const confirming = handlers.onConfirm();
    expect(calls).toEqual(["busy:true", PREVIEW_CALL]);

    decryption.resolve("dummy-preview");
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "showConfirm:dummy-preview", "busy:false"]);
  });

  it("復号に失敗したら、画面が開いたままなら、エラーを表示する", async () => {
    const { calls, decryption, handlers } = setup();

    const confirming = handlers.onConfirm();
    decryption.reject(new Error("dummy failure"));
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "showError:dummy failure", "busy:false"]);
  });

  it("復号に失敗しても、その間に画面が閉じられていたら、エラーは表示しない", async () => {
    const { calls, decryption, handlers } = setup({ isStillOpen: () => false });

    const confirming = handlers.onConfirm();
    decryption.reject(new Error("dummy failure"));
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "busy:false"]);
  });

  it("復号している間に画面が閉じられたら、復号済みの内容が保留されたまま残らないよう、破棄する", async () => {
    const { calls, decryption, handlers } = setup({ isStillOpen: () => false });

    const confirming = handlers.onConfirm();
    decryption.resolve("dummy-preview");
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "discardPending", "busy:false"]);
  });

  it("復号している間の再入は、復号を重ねて実行しない", async () => {
    const { calls, decryption, handlers } = setup();

    const first = handlers.onConfirm();
    const second = handlers.onConfirm();
    decryption.resolve("dummy-preview");
    await Promise.all([first, second]);
    expect(calls.filter((call) => call.startsWith("preview:"))).toHaveLength(1);
  });

  it("復号が終わったら、次のOKでは、再び復号できる", async () => {
    const { calls, decryption, handlers } = setup();

    const first = handlers.onConfirm();
    decryption.resolve("dummy-preview");
    await first;

    calls.length = 0;
    const again = handlers.onConfirm();
    await again;
    expect(calls.filter((call) => call.startsWith("preview:"))).toHaveLength(1);
  });

  it("パスフレーズ入力画面が開いていなければ(閉じる途中に届いた押下)、何もしない", async () => {
    const { calls, handlers } = setup({ target: () => null });

    await handlers.onConfirm();
    expect(calls).toEqual([]);
  });
});
