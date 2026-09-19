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

const TARGET = {
  session: 1,
  sourcePath: "C:/dummy/export.smx",
  passphrase: "dummy-passphrase-0001",
};
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
    discardPending: () => {
      calls.push("discardPending");
      return Promise.resolve();
    },
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

  it("開いたままかは、OKを押した時の画面(セッション)で判定する。同じファイルを開き直した別の画面は、開いたままとは見なさない", async () => {
    const openSessions = new Set([2]);
    const asked: number[] = [];
    const { calls, decryption, handlers } = setup({
      isStillOpen: (session) => {
        asked.push(session);
        return openSessions.has(session);
      },
    });

    const confirming = handlers.onConfirm();
    decryption.resolve("dummy-preview");
    await confirming;
    expect(asked).toEqual([TARGET.session]);
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "discardPending", "busy:false"]);
  });

  it("破棄が終わるまで、待ちを解かない(次の復号が、破棄と入れ違いにならない)", async () => {
    const discard = deferred<void>();
    const { calls, decryption, handlers } = setup({
      isStillOpen: () => false,
      discardPending: () => {
        calls.push("discardPending");
        return discard.promise;
      },
    });

    const confirming = handlers.onConfirm();
    decryption.resolve("dummy-preview");
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "discardPending"]);

    discard.resolve();
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "discardPending", "busy:false"]);
  });

  it("破棄に失敗しても、待ちは解ける(次の復号を始められなくならない)", async () => {
    const { calls, decryption, handlers } = setup({
      isStillOpen: () => false,
      discardPending: () => {
        calls.push("discardPending");
        return Promise.reject(new Error("dummy discard failure"));
      },
    });

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
