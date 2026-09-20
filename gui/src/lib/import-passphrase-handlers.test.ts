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

// 復号の結果。pendingIdは、Rust側に保留された、その復号済みの内容の識別子。
interface DummyPreview {
  pendingId: number;
  label: string;
}

const TARGET = {
  session: 1,
  sourcePath: "C:/dummy/export.smx",
  passphrase: "dummy-passphrase-0001",
};
const PREVIEW_CALL = `preview:${TARGET.sourcePath}:${TARGET.passphrase}`;

// 保留の識別子は、Rust側で0から払い出される(アプリ起動後の最初の復号は0)。0は、所有する保留が無い(null)ことではなく、
// 有効な識別子として扱う。0を無い扱いにする真偽値の判定を見逃さないよう、全てのテストを、0と、0以外(7)の両方で実行する。
describe.each([0, 7])("createImportPassphraseHandlers(届いた結果の識別子: %i)", (pendingId) => {
  const PREVIEW: DummyPreview = { pendingId, label: "dummy-preview" };
  // 画面が既に所有している、届いた結果とは別の識別子。
  const other = pendingId + 1;

  // 呼び出された操作を、順に記録する依存を作る。ownedPendingIdは、この画面が所有する保留の識別子(初期値を指定できる)。
  function setup(
    overrides: Partial<ImportPassphraseDeps<DummyPreview>> = {},
    initialOwnedPendingId: number | null = null
  ) {
    const calls: string[] = [];
    const decrypting = { current: false };
    const ownedPendingId: { current: number | null } = { current: initialOwnedPendingId };
    const decryption = deferred<DummyPreview>();
    const deps: ImportPassphraseDeps<DummyPreview> = {
      target: () => TARGET,
      preview: (sourcePath, passphrase) => {
        calls.push(`preview:${sourcePath}:${passphrase}`);
        return decryption.promise;
      },
      isStillOpen: () => true,
      // 確認画面を出す時点で、所有する識別子が既に記録されているかを、その場で読んで残す。
      showConfirm: (preview) =>
        calls.push(`showConfirm:${preview.label}:owned=${ownedPendingId.current}`),
      showError: (error) => calls.push(`showError:${(error as Error).message}`),
      discardPending: (discardedId) => {
        calls.push(`discardPending:${discardedId}`);
        return Promise.resolve();
      },
      onBusyChange: (busy) => calls.push(`busy:${busy}`),
      ...overrides,
    };
    return {
      calls,
      decrypting,
      ownedPendingId,
      decryption,
      handlers: createImportPassphraseHandlers(deps, decrypting, ownedPendingId),
    };
  }

  it("復号できたら、確認画面へ進み、復号している間だけ待ちになる", async () => {
    const { calls, decryption, handlers } = setup();

    const confirming = handlers.onConfirm();
    expect(calls).toEqual(["busy:true", PREVIEW_CALL]);

    decryption.resolve(PREVIEW);
    await confirming;
    expect(calls).toEqual([
      "busy:true",
      PREVIEW_CALL,
      `showConfirm:dummy-preview:owned=${pendingId}`,
      "busy:false",
    ]);
  });

  it("確認画面へ進む時点で、その保留の識別子を、この画面が所有するものとして、同期的に記録済みにする(確認画面の描画を待たない)", async () => {
    const { calls, ownedPendingId, decryption, handlers } = setup();

    const confirming = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await confirming;

    expect(ownedPendingId.current).toBe(pendingId);
    // showConfirmが呼ばれた時点で、既に記録されている(記録が、確認画面の描画より後だと、その間に離れたとき、保留を破棄できない)。
    expect(calls).toContain(`showConfirm:dummy-preview:owned=${pendingId}`);
  });

  it("復号に失敗したら、画面が開いたままなら、エラーを表示し、所有する識別子は記録しない", async () => {
    const { calls, ownedPendingId, decryption, handlers } = setup();

    const confirming = handlers.onConfirm();
    decryption.reject(new Error("dummy failure"));
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "showError:dummy failure", "busy:false"]);
    expect(ownedPendingId.current).toBeNull();
  });

  it("復号に失敗しても、その間に画面が閉じられていたら、エラーは表示しない", async () => {
    const { calls, decryption, handlers } = setup({ isStillOpen: () => false });

    const confirming = handlers.onConfirm();
    decryption.reject(new Error("dummy failure"));
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, "busy:false"]);
  });

  it("復号している間に画面が閉じられたら、復号済みの内容が保留されたまま残らないよう、届いた結果の保留を、その識別子で破棄する", async () => {
    const { calls, decryption, handlers } = setup({ isStillOpen: () => false });

    const confirming = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, `discardPending:${pendingId}`, "busy:false"]);
  });

  it("遅れて届いた結果は、その結果の識別子だけを破棄し、この画面が既に所有する識別子(別の保留)は変えない", async () => {
    // 画面は、別の保留(識別子other)を所有している。この結果が、閉じられた画面へ遅れて届く。
    const { calls, ownedPendingId, decryption, handlers } = setup(
      { isStillOpen: () => false },
      other
    );

    const confirming = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await confirming;

    expect(calls.filter((call) => call.startsWith("discardPending:"))).toEqual([
      `discardPending:${pendingId}`,
    ]);
    expect(ownedPendingId.current).toBe(other);
  });

  // 確認画面の描画より前に画面が閉じられると、所有する保留(記録済みの識別子)を確認する人が居ないまま、画面は
  // 離れられずに残る。次のインポートの結果が届いたとき、記録を上書きすると、その保留は誰にも破棄されなくなる。
  describe("確認画面の描画より前に画面が閉じられ、確認されないまま所有する保留が残っているとき", () => {
    const SECOND_PREVIEW: DummyPreview = { pendingId: other, label: "dummy-preview-2" };

    // 1回目の復号の結果が届き(所有する識別子を記録し)、確認画面が描画される前に画面が閉じられた状態を作る
    // (画面を離れる操作は起きないため、記録は残る)。2回目の復号は、呼び出し側が決着させる。
    async function afterFirstResultLeftUnconfirmed(
      overrides: Partial<ImportPassphraseDeps<DummyPreview>> = {}
    ) {
      const results = [deferred<DummyPreview>(), deferred<DummyPreview>()];
      let started = 0;
      const setupResult = setup({ preview: () => results[started++].promise, ...overrides });
      const first = setupResult.handlers.onConfirm();
      results[0].resolve(PREVIEW);
      await first;
      expect(setupResult.ownedPendingId.current).toBe(pendingId);
      setupResult.calls.length = 0;
      return { ...setupResult, secondResult: results[1] };
    }

    it("次の結果が届いたら、確認画面へ進む前に、前の保留をその識別子で破棄し、新しい保留の識別子を記録する", async () => {
      const { calls, ownedPendingId, secondResult, handlers } = await afterFirstResultLeftUnconfirmed();

      const second = handlers.onConfirm();
      secondResult.resolve(SECOND_PREVIEW);
      await second;

      // 前の保留の破棄が先で、その後に、新しい識別子を記録して、確認画面へ進む。
      expect(calls).toEqual([
        "busy:true",
        `discardPending:${pendingId}`,
        `showConfirm:dummy-preview-2:owned=${other}`,
        "busy:false",
      ]);
      expect(ownedPendingId.current).toBe(other);
    });

    it("前の保留の破棄が終わるのを待たずに、新しい識別子を記録して、確認画面へ進む(待つ間に画面が閉じられうるため)", async () => {
      const discard = deferred<void>();
      const { calls, ownedPendingId, secondResult, handlers } = await afterFirstResultLeftUnconfirmed({
        discardPending: (discardedId) => {
          calls.push(`discardPending:${discardedId}`);
          return discard.promise;
        },
      });

      const second = handlers.onConfirm();
      secondResult.resolve(SECOND_PREVIEW);
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      expect(calls).toEqual([
        "busy:true",
        `discardPending:${pendingId}`,
        `showConfirm:dummy-preview-2:owned=${other}`,
        "busy:false",
      ]);
      expect(ownedPendingId.current).toBe(other);

      discard.resolve();
      await second;
    });

    it("前の保留の破棄に失敗しても、エラーは表示せず、確認画面へ進む(前の保留の後始末の失敗を、新しい結果の失敗にしない)", async () => {
      const { calls, ownedPendingId, secondResult, handlers } = await afterFirstResultLeftUnconfirmed({
        discardPending: (discardedId) => {
          calls.push(`discardPending:${discardedId}`);
          return Promise.reject(new Error("dummy discard failure"));
        },
      });

      const second = handlers.onConfirm();
      secondResult.resolve(SECOND_PREVIEW);
      await second;

      expect(calls).toEqual([
        "busy:true",
        `discardPending:${pendingId}`,
        `showConfirm:dummy-preview-2:owned=${other}`,
        "busy:false",
      ]);
      expect(ownedPendingId.current).toBe(other);
    });

    it("前の保留の破棄の発行が、同期的に例外を投げても、エラーは表示せず、新しい識別子を記録して、確認画面へ進む(新しい保留を、所有しないまま残さない)", async () => {
      const { calls, ownedPendingId, secondResult, handlers } = await afterFirstResultLeftUnconfirmed({
        discardPending: (discardedId) => {
          calls.push(`discardPending:${discardedId}`);
          throw new Error("dummy synchronous failure");
        },
      });

      const second = handlers.onConfirm();
      secondResult.resolve(SECOND_PREVIEW);
      await second;

      expect(calls).toEqual([
        "busy:true",
        `discardPending:${pendingId}`,
        `showConfirm:dummy-preview-2:owned=${other}`,
        "busy:false",
      ]);
      expect(ownedPendingId.current).toBe(other);
    });
  });

  it("開いたままかは、OKを押した時の画面(開いた回の番号)で判定する。同じファイルを開き直した別の画面は、開いたままとは見なさない", async () => {
    const openSessions = new Set([2]);
    const asked: number[] = [];
    const { calls, ownedPendingId, decryption, handlers } = setup({
      isStillOpen: (session) => {
        asked.push(session);
        return openSessions.has(session);
      },
    });

    const confirming = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await confirming;
    expect(asked).toEqual([TARGET.session]);
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, `discardPending:${pendingId}`, "busy:false"]);
    // 別の画面の結果は、この画面の所有にならない。
    expect(ownedPendingId.current).toBeNull();
  });

  it("破棄が終わるまで、待ちを解かない(この画面が始めた復号の後始末が終わってから、次の復号を受け付ける)", async () => {
    const discard = deferred<void>();
    const { calls, decryption, handlers } = setup({
      isStillOpen: () => false,
      discardPending: (discardedId) => {
        calls.push(`discardPending:${discardedId}`);
        return discard.promise;
      },
    });

    const confirming = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, `discardPending:${pendingId}`]);

    discard.resolve();
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, `discardPending:${pendingId}`, "busy:false"]);
  });

  it("破棄に失敗しても、待ちは解ける(次の復号を始められなくならない)", async () => {
    const { calls, decryption, handlers } = setup({
      isStillOpen: () => false,
      discardPending: (discardedId) => {
        calls.push(`discardPending:${discardedId}`);
        return Promise.reject(new Error("dummy discard failure"));
      },
    });

    const confirming = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await confirming;
    expect(calls).toEqual(["busy:true", PREVIEW_CALL, `discardPending:${pendingId}`, "busy:false"]);
  });

  it("復号している間の再入は、復号を重ねて実行しない", async () => {
    const { calls, decryption, handlers } = setup();

    const first = handlers.onConfirm();
    const second = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await Promise.all([first, second]);
    expect(calls.filter((call) => call.startsWith("preview:"))).toHaveLength(1);
  });

  it("復号が終わったら、次のOKでは、再び復号できる", async () => {
    const { calls, decryption, handlers } = setup();

    const first = handlers.onConfirm();
    decryption.resolve(PREVIEW);
    await first;

    calls.length = 0;
    const again = handlers.onConfirm();
    await again;
    expect(calls.filter((call) => call.startsWith("preview:"))).toHaveLength(1);
  });

  it("パスフレーズ入力画面が開いていなければ(閉じる途中に届いた押下)、何もしない", async () => {
    const { calls, ownedPendingId, handlers } = setup({ target: () => null });

    await handlers.onConfirm();
    expect(calls).toEqual([]);
    expect(ownedPendingId.current).toBeNull();
  });
});
