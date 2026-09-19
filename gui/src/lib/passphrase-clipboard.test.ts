import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CLIPBOARD_CLEAR_DELAY_MS } from "./clipboard-clear-delay";
import { createPassphraseClipboard, type PassphraseClipboardDeps } from "./passphrase-clipboard";

const VALUE = "dummy-passphrase-0001";
const OTHER_VALUE = "dummy-passphrase-0002";

type ClearOutcome = { outcome: "cleared" | "skipped_content_changed" | "skipped_unable_to_verify" };

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function setup(overrides: Partial<PassphraseClipboardDeps> = {}) {
  const notified: string[] = [];
  const write = vi.fn(async (_text: string) => {});
  const clearIfMatches = vi.fn(async (_expected: string): Promise<ClearOutcome> => ({ outcome: "cleared" }));
  const tracked: unknown[] = [];
  const track: PassphraseClipboardDeps["track"] = (operation) => {
    tracked.push(operation);
    return operation;
  };
  const deps: PassphraseClipboardDeps = {
    write,
    clearIfMatches,
    track,
    notify: {
      copied: () => notified.push("copied"),
      copyFailed: () => notified.push("copyFailed"),
      notCleared: () => notified.push("notCleared"),
    },
    ...overrides,
  };
  return { clipboard: createPassphraseClipboard(deps), write, clearIfMatches, tracked, notified };
}

// 決着済みのPromiseの後続処理(マイクロタスク)を進める。
const settle = () => vi.advanceTimersByTimeAsync(0);

describe("createPassphraseClipboard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("自動クリア", () => {
    it("コピーに成功すると通知し、設定した時間ちょうどで、書き込んだ値の一致を確認してクリアする", async () => {
      const { clipboard, write, clearIfMatches, notified } = setup();

      clipboard.copy(VALUE);
      await settle();
      expect(write).toHaveBeenCalledWith(VALUE);
      expect(notified).toEqual(["copied"]);

      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS - 1);
      expect(clearIfMatches).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
      expect(clearIfMatches).toHaveBeenCalledWith(VALUE);
    });

    it("時間を指定すると、その時間で発火する", async () => {
      const { clipboard, clearIfMatches } = setup({ delayMs: 1_000 });

      clipboard.copy(VALUE);
      await settle();
      await vi.advanceTimersByTimeAsync(999);
      expect(clearIfMatches).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
    });

    it("続けてコピーすると、前のタイマーは取り消され、最後のコピーから数え直す", async () => {
      const { clipboard, clearIfMatches } = setup();

      clipboard.copy(VALUE);
      await settle();
      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS / 2);
      clipboard.copy(OTHER_VALUE);
      await settle();

      // 最初のコピーから設定時間が経っても、クリアされない。
      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS / 2 + 1);
      expect(clearIfMatches).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS / 2);
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
      expect(clearIfMatches).toHaveBeenCalledWith(OTHER_VALUE);
    });

    it("クリアを確認できなかったときだけ、手動でのクリアを促す", async () => {
      for (const [outcome, expected] of [
        ["cleared", []],
        ["skipped_content_changed", []],
        ["skipped_unable_to_verify", ["notCleared"]],
      ] as const) {
        const { clipboard, notified } = setup({
          clearIfMatches: async () => ({ outcome }),
        });
        clipboard.copy(VALUE);
        await settle();
        notified.length = 0;
        await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS);
        expect(notified, outcome).toEqual(expected);
      }
    });

    it("クリアの呼び出し自体が失敗したときも、手動でのクリアを促す", async () => {
      const { clipboard, notified } = setup({
        clearIfMatches: async () => {
          throw new Error("dummy failure");
        },
      });
      clipboard.copy(VALUE);
      await settle();
      notified.length = 0;
      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS);
      expect(notified).toEqual(["notCleared"]);
    });
  });

  describe("コピーの失敗", () => {
    it("書き込みに失敗したら、失敗を通知し、タイマーは設置しない", async () => {
      const { clipboard, notified, clearIfMatches } = setup({
        write: async () => {
          throw new Error("dummy failure");
        },
      });

      clipboard.copy(VALUE);
      await settle();
      expect(notified).toEqual(["copyFailed"]);

      // 失敗の直後に一度だけ試みたクリア以外は、設定時間が経っても、何も起きない。
      clearIfMatches.mockClear();
      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS * 2);
      expect(clearIfMatches).not.toHaveBeenCalled();
    });

    it("書き込みの後段だけが失敗しても、値が実際にはクリップボードに残ることがあるため、一致する場合だけ消す", async () => {
      const { clipboard, clearIfMatches } = setup({
        write: async () => {
          throw new Error("dummy failure");
        },
      });

      clipboard.copy(VALUE);
      await settle();
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
      expect(clearIfMatches).toHaveBeenCalledWith(VALUE);
    });

    it("失敗の後のクリアの結果は、追加で通知しない(失敗の通知だけで足りる)", async () => {
      const { clipboard, notified } = setup({
        write: async () => {
          throw new Error("dummy failure");
        },
        clearIfMatches: async () => ({ outcome: "skipped_unable_to_verify" }),
      });

      clipboard.copy(VALUE);
      await settle();
      expect(notified).toEqual(["copyFailed"]);
    });

    it("既に次のコピーが始まっていれば、古いコピーの失敗は、通知もクリアもしない", async () => {
      const failing = deferred();
      const write = vi
        .fn<(text: string) => Promise<void>>()
        .mockImplementationOnce(() => failing.promise)
        .mockResolvedValue(undefined);
      const { clipboard, notified, clearIfMatches } = setup({ write });

      clipboard.copy(VALUE);
      clipboard.copy(OTHER_VALUE);
      failing.reject(new Error("dummy failure"));
      await settle();

      expect(notified).toEqual(["copied"]);
      expect(clearIfMatches).not.toHaveBeenCalled();
    });
  });

  describe("再生成(clearNow)", () => {
    it("タイマーを取り消し、コピー済みの値を、その場でクリアする", async () => {
      const { clipboard, clearIfMatches } = setup();
      clipboard.copy(VALUE);
      await settle();

      clipboard.clearNow();
      await settle();
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
      expect(clearIfMatches).toHaveBeenCalledWith(VALUE);

      // 取り消したタイマーは、発火しない。
      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS * 2);
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
    });

    it("コピー済みの値が無ければ、何もしない(二度目も同じ)", async () => {
      const { clipboard, clearIfMatches } = setup();
      clipboard.clearNow();
      await settle();
      expect(clearIfMatches).not.toHaveBeenCalled();

      clipboard.copy(VALUE);
      await settle();
      clipboard.clearNow();
      await settle();
      clipboard.clearNow();
      await settle();
      expect(clearIfMatches).toHaveBeenCalledTimes(1);
    });

    it("コピーの書き込みの応答を待つ間に再生成されたら、そのコピーには、通知もタイマーも設置しない", async () => {
      const writing = deferred();
      const { clipboard, notified, clearIfMatches } = setup({ write: () => writing.promise });

      clipboard.copy(VALUE);
      clipboard.clearNow();
      writing.resolve();
      await settle();

      expect(notified).toEqual([]);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS * 2);
      expect(clearIfMatches).not.toHaveBeenCalled();
    });

    it("再生成のクリアを確認できなかったときも、手動でのクリアを促す", async () => {
      const { clipboard, notified } = setup({
        clearIfMatches: async () => ({ outcome: "skipped_unable_to_verify" }),
      });
      clipboard.copy(VALUE);
      await settle();
      notified.length = 0;

      clipboard.clearNow();
      await settle();
      expect(notified).toEqual(["notCleared"]);
    });
  });

  it("書き込みとクリアの応答待ちは、全て渡された関数で追跡する", async () => {
    const { clipboard, tracked } = setup();
    clipboard.copy(VALUE);
    await settle();
    await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_DELAY_MS);
    // 書き込み1件と、自動クリア1件。
    expect(tracked).toHaveLength(2);
  });
});
