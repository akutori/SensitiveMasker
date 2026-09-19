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

// 保留の識別子は、Rust側で0から払い出される(アプリ起動後の最初の復号は0)。0は、所有する保留が無い(null)ことではなく、
// 有効な識別子として扱う。0を無い扱いにする真偽値の判定を見逃さないよう、全てのテストを、0と、0以外(7)の両方で実行する。
describe.each([0, 7])("createImportConfirmHandlers(所有する保留の識別子: %i)", (owned) => {
  // 確定や破棄の後に、次の復号の結果として所有する、別の識別子。
  const next = owned + 1;
  const afterNext = owned + 2;

  // 呼び出された操作を、順に記録する依存を作る。ownedPendingIdは、この画面が所有する保留の識別子(初期値を指定できる)。
  function setup(overrides: Partial<ImportConfirmDeps> = {}, initialOwnedPendingId: number | null = owned) {
    const calls: string[] = [];
    const ownedPendingId: { current: number | null } = { current: initialOwnedPendingId };
    const commit = deferred();
    const deps: ImportConfirmDeps = {
      isOpen: () => true,
      commit: (pendingId) => {
        calls.push(`commit:${pendingId}`);
        return commit.promise;
      },
      discardPending: (pendingId) => calls.push(`discardPending:${pendingId}`),
      close: () => calls.push("close"),
      closeIfStillOpen: () => calls.push("closeIfStillOpen"),
      ...overrides,
    };
    return {
      calls,
      ownedPendingId,
      commit,
      handlers: createImportConfirmHandlers(deps, ownedPendingId),
    };
  }

  it("開く操作では、何もしない", () => {
    const { calls, handlers } = setup();
    handlers.onOpenChange(true);
    expect(calls).toEqual([]);
  });

  it("キャンセルなど、確定しない閉じ方では、画面を閉じ、この画面が所有する保留を、その識別子で破棄する", () => {
    const { calls, ownedPendingId, handlers } = setup();
    handlers.onOpenChange(false);
    expect(calls).toEqual(["close", `discardPending:${owned}`]);
    // 破棄した保留は、もう所有しない(後で離れたときに、二重に破棄しない)。
    expect(ownedPendingId.current).toBeNull();
  });

  it("所有する保留が無ければ、閉じても、何も破棄しない(他の画面の保留を、識別子を指定せずに消さない)", () => {
    const { calls, handlers } = setup({}, null);
    handlers.onOpenChange(false);
    expect(calls).toEqual(["close"]);
  });

  it("確定を始めた後の閉じる操作では、保留中の内容を破棄しない(確定と続けて発行すると、実行順が保証されず、破棄が先だと確定が失敗する)", async () => {
    const { calls, commit, handlers } = setup();

    // 「インポート実行」は、確定を先に呼び、続けて画面を閉じる操作を呼ぶ。
    const confirming = handlers.onConfirm();
    handlers.onOpenChange(false);
    expect(calls).toEqual([`commit:${owned}`, "close"]);

    commit.resolve();
    await confirming;
    expect(calls).toEqual([`commit:${owned}`, "close", "closeIfStillOpen"]);
  });

  it("確定は、この画面が所有する保留の識別子で行い、確定を始めた時点で、その保留を所有しなくなる(確定は、成否に関わらず保留を消費する)", async () => {
    const { calls, ownedPendingId, commit, handlers } = setup();

    const confirming = handlers.onConfirm();
    expect(calls).toEqual([`commit:${owned}`]);
    expect(ownedPendingId.current).toBeNull();

    commit.resolve();
    await confirming;
  });

  it("確定が終わったら、次の(新しい保留の)閉じる操作では、再び、その保留を破棄する", async () => {
    const { calls, ownedPendingId, commit, handlers } = setup();
    const confirming = handlers.onConfirm();
    commit.resolve();
    await confirming;

    // 次の復号の結果が届き、新しい保留を所有する。
    ownedPendingId.current = next;
    calls.length = 0;
    handlers.onOpenChange(false);
    expect(calls).toEqual(["close", `discardPending:${next}`]);
  });

  it("確定が終わった後は、所有する保留が無いため、閉じる操作でも、破棄を発行しない(消費済みの保留を、二重に破棄しない)", async () => {
    const { calls, commit, handlers } = setup();
    const confirming = handlers.onConfirm();
    commit.resolve();
    await confirming;

    calls.length = 0;
    handlers.onOpenChange(false);
    expect(calls).toEqual(["close"]);
  });

  it("確定が失敗しても、例外は伝えず、後始末をして、次の(新しい保留の)確定を続けられる", async () => {
    const { calls, ownedPendingId, commit, handlers } = setup();
    const confirming = handlers.onConfirm();
    commit.reject(new Error("dummy failure"));
    await expect(confirming).resolves.toBeUndefined();
    expect(calls).toEqual([`commit:${owned}`, "closeIfStillOpen"]);

    // 次の復号の結果が届き、新しい保留を所有すると、その保留を確定できる。
    ownedPendingId.current = next;
    calls.length = 0;
    await handlers.onConfirm();
    expect(calls).toEqual([`commit:${next}`, "closeIfStillOpen"]);
  });

  it("確定している間に、もう一度押されても、確定は1回だけ実行される", async () => {
    const { calls, commit, handlers } = setup();
    const first = handlers.onConfirm();
    const second = handlers.onConfirm();
    commit.resolve();
    await Promise.all([first, second]);
    expect(calls.filter((call) => call.startsWith("commit:"))).toEqual([`commit:${owned}`]);
  });

  it("確認画面が既に閉じている(閉じる途中に届いた押下)なら、確定を実行せず、所有する保留も手放さない", async () => {
    const { calls, ownedPendingId, handlers } = setup({ isOpen: () => false });
    await handlers.onConfirm();
    expect(calls).toEqual([]);
    expect(ownedPendingId.current).toBe(owned);
  });

  it("所有する保留が無ければ(破棄済み)、確認画面が開いて見えても、確定を実行しない(識別子の無い確定を、発行しない)", async () => {
    const { calls, handlers } = setup({}, null);
    await handlers.onConfirm();
    expect(calls).toEqual([]);
  });

  it("画面を離れると、確認画面が開いているかに関わらず、この画面が所有する保留を、その識別子で破棄する(復号の結果が届いてから確認画面が描画されるまでの間に離れても、保留を残さない)", () => {
    for (const isOpen of [true, false]) {
      const { calls, ownedPendingId, handlers } = setup({ isOpen: () => isOpen });
      handlers.onLeave();
      expect(calls).toEqual([`discardPending:${owned}`]);
      expect(ownedPendingId.current).toBeNull();
    }
  });

  it("画面を離れても、所有する保留が無ければ、何も破棄しない(他の画面の保留を、消さない)", () => {
    const { calls, handlers } = setup({}, null);
    handlers.onLeave();
    expect(calls).toEqual([]);
  });

  it("確定している間に画面を離れても、保留中の内容を破棄しない(確定が、それを使う)", async () => {
    const { calls, commit, handlers } = setup();
    const confirming = handlers.onConfirm();
    handlers.onLeave();
    expect(calls).toEqual([`commit:${owned}`]);

    commit.resolve();
    await confirming;
  });

  it("確定が終わった後に画面を離れたら、その後に所有した保留を、再び破棄する", async () => {
    const { calls, ownedPendingId, commit, handlers } = setup();
    const confirming = handlers.onConfirm();
    commit.resolve();
    await confirming;

    ownedPendingId.current = afterNext;
    calls.length = 0;
    handlers.onLeave();
    expect(calls).toEqual([`discardPending:${afterNext}`]);
  });
});
