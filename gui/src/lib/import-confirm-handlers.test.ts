import { describe, expect, it } from "vitest";
import { createImportConfirmHandlers, type ImportConfirmDeps } from "./import-confirm-handlers";

// 決着を、テストから握れるPromise。
function deferred() {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

// 呼び出された操作を、順に記録する依存を作る。
function setup(overrides: Partial<ImportConfirmDeps> = {}) {
  const calls: string[] = [];
  const started = { current: false };
  const commit = deferred();
  const deps: ImportConfirmDeps = {
    isOpen: () => true,
    commit: () => {
      calls.push("commit");
      return commit.promise;
    },
    discardPending: () => calls.push("discardPending"),
    close: () => calls.push("close"),
    closeIfStillOpen: () => calls.push("closeIfStillOpen"),
    ...overrides,
  };
  return { calls, started, commit, handlers: createImportConfirmHandlers(deps, started) };
}

describe("createImportConfirmHandlers", () => {
  it("開く操作では、何もしない", () => {
    const { calls, handlers } = setup();
    handlers.onOpenChange(true);
    expect(calls).toEqual([]);
  });

  it("キャンセルなど、確定しない閉じ方では、画面を閉じ、保留中の内容を破棄する", () => {
    const { calls, handlers } = setup();
    handlers.onOpenChange(false);
    expect(calls).toEqual(["close", "discardPending"]);
  });

  it("確定を始めた後の閉じる操作では、保留中の内容を破棄しない(確定と続けて発行すると、実行順が保証されず、破棄が先だと確定が失敗する)", async () => {
    const { calls, commit, handlers } = setup();

    // 「インポート実行」は、確定を先に呼び、続けて画面を閉じる操作を呼ぶ。
    const confirming = handlers.onConfirm();
    handlers.onOpenChange(false);
    expect(calls).toEqual(["commit", "close"]);

    commit.resolve();
    await confirming;
    expect(calls).toEqual(["commit", "close", "closeIfStillOpen"]);
  });

  it("確定が終わったら、次の閉じる操作では、再び破棄する", async () => {
    const { calls, commit, handlers } = setup();
    const confirming = handlers.onConfirm();
    commit.resolve();
    await confirming;

    calls.length = 0;
    handlers.onOpenChange(false);
    expect(calls).toEqual(["close", "discardPending"]);
  });

  it("確定が失敗しても、例外は伝えず、後始末をして、次の閉じる操作では、再び破棄する", async () => {
    const { calls, commit, handlers, started } = setup();
    const confirming = handlers.onConfirm();
    commit.reject(new Error("dummy failure"));
    await expect(confirming).resolves.toBeUndefined();
    expect(calls).toEqual(["commit", "closeIfStillOpen"]);
    expect(started.current).toBe(false);
  });

  it("確定している間に、もう一度押されても、確定は1回だけ実行される", async () => {
    const { calls, commit, handlers } = setup();
    const first = handlers.onConfirm();
    const second = handlers.onConfirm();
    commit.resolve();
    await Promise.all([first, second]);
    expect(calls.filter((call) => call === "commit")).toHaveLength(1);
  });

  it("確認画面が既に閉じている(閉じる途中に届いた押下)なら、確定を実行しない", async () => {
    const { calls, handlers } = setup({ isOpen: () => false });
    await handlers.onConfirm();
    expect(calls).toEqual([]);
  });
});
