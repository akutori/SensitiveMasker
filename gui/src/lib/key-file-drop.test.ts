import { beforeEach, describe, expect, it, vi } from "vitest";

type DragDropPayload =
  | { type: "enter"; paths: string[]; position: { x: number; y: number } }
  | { type: "over"; position: { x: number; y: number } }
  | { type: "drop"; paths: string[]; position: { x: number; y: number } }
  | { type: "leave" };

const webview = vi.hoisted(() => {
  const state: { listener: ((event: { payload: unknown }) => void) | null; unlisten: () => void } = {
    listener: null,
    unlisten: () => {},
  };
  return {
    state,
    getCurrentWebview: vi.fn(() => ({
      onDragDropEvent: vi.fn(async (listener: (event: { payload: unknown }) => void) => {
        state.listener = listener;
        return state.unlisten;
      }),
    })),
  };
});
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: webview.getCurrentWebview }));

import { subscribeKeyFileDrop } from "./key-file-drop";

const AT = { x: 1, y: 2 };

function emit(payload: DragDropPayload) {
  webview.state.listener?.({ payload });
}

describe("subscribeKeyFileDrop", () => {
  const handlers = { onActiveChange: vi.fn(), onPicked: vi.fn(), onRejected: vi.fn() };

  beforeEach(() => {
    handlers.onActiveChange.mockReset();
    handlers.onPicked.mockReset();
    handlers.onRejected.mockReset();
    webview.state.listener = null;
    webview.state.unlisten = vi.fn();
  });

  it("ファイルを、ウィンドウの上へ持ってくると強調し、離れると戻す", async () => {
    await subscribeKeyFileDrop(handlers);

    emit({ type: "enter", paths: ["C:\\dummy\\a.smxkey"], position: AT });
    emit({ type: "over", position: AT });
    expect(handlers.onActiveChange).toHaveBeenLastCalledWith(true);
    emit({ type: "leave" });
    expect(handlers.onActiveChange).toHaveBeenLastCalledWith(false);
    expect(handlers.onPicked).not.toHaveBeenCalled();
    expect(handlers.onRejected).not.toHaveBeenCalled();
  });

  it("鍵ファイルを1つドロップすると、強調をやめて、そのパスを知らせる", async () => {
    await subscribeKeyFileDrop(handlers);

    emit({ type: "drop", paths: ["C:\\dummy\\a.smxkey"], position: AT });

    expect(handlers.onActiveChange).toHaveBeenLastCalledWith(false);
    expect(handlers.onPicked).toHaveBeenCalledWith("C:\\dummy\\a.smxkey");
    expect(handlers.onRejected).not.toHaveBeenCalled();
  });

  it("鍵ファイルでないファイルをドロップすると、強調をやめて、選べなかった理由を知らせる", async () => {
    await subscribeKeyFileDrop(handlers);

    emit({ type: "drop", paths: ["C:\\dummy\\a.txt"], position: AT });

    expect(handlers.onActiveChange).toHaveBeenLastCalledWith(false);
    expect(handlers.onPicked).not.toHaveBeenCalled();
    expect(handlers.onRejected).toHaveBeenCalledWith("拡張子が.smxkeyのファイルをドロップしてください");
  });

  it("返された購読の解除を、そのまま返す", async () => {
    const stop = await subscribeKeyFileDrop(handlers);

    expect(stop).toBe(webview.state.unlisten);
  });
});
