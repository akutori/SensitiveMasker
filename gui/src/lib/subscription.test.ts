import { describe, expect, it, vi } from "vitest";
import { subscribeUntilStopped, subscribeWhileEnabled } from "./subscription";

// 解除する関数のPromiseを、テストが、好きな時に、決着させる。
function deferredUnlisten() {
  const stop = vi.fn();
  let resolve!: (stop: () => void) => void;
  const promise = new Promise<() => void>((r) => {
    resolve = r;
  });
  return { stop, promise, settle: () => resolve(stop) };
}

// Promiseの後続(then)が動くまで待つ。
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe("subscribeUntilStopped", () => {
  it("購読を1回だけ始め、止めるまでは、解除しない", async () => {
    const unlisten = deferredUnlisten();
    const listen = vi.fn(() => unlisten.promise);

    subscribeUntilStopped(listen);
    unlisten.settle();
    await flush();

    expect(listen).toHaveBeenCalledTimes(1);
    expect(unlisten.stop).not.toHaveBeenCalled();
  });

  it("止めると、購読を解除する", async () => {
    const unlisten = deferredUnlisten();
    unlisten.settle();

    const stop = subscribeUntilStopped(() => unlisten.promise);
    stop();
    await flush();

    expect(unlisten.stop).toHaveBeenCalledTimes(1);
  });

  it("購読が確立する前に止めても、確立した時点で解除する", async () => {
    const unlisten = deferredUnlisten();

    const stop = subscribeUntilStopped(() => unlisten.promise);
    stop();
    await flush();
    expect(unlisten.stop).not.toHaveBeenCalled();

    unlisten.settle();
    await flush();
    expect(unlisten.stop).toHaveBeenCalledTimes(1);
  });

  it("止めるのを2回呼んでも、解除は1回だけ", async () => {
    const unlisten = deferredUnlisten();
    unlisten.settle();

    const stop = subscribeUntilStopped(() => unlisten.promise);
    stop();
    stop();
    await flush();

    expect(unlisten.stop).toHaveBeenCalledTimes(1);
  });
});

describe("subscribeWhileEnabled", () => {
  it("無効なときは、購読せず、後始末も返さない", () => {
    const listen = vi.fn(() => Promise.resolve(() => {}));

    const stop = subscribeWhileEnabled(false, listen);

    expect(listen).not.toHaveBeenCalled();
    expect(stop).toBeUndefined();
  });

  it("有効なときは、購読し、返された後始末で解除する", async () => {
    const unlisten = deferredUnlisten();
    unlisten.settle();
    const listen = vi.fn(() => unlisten.promise);

    const stop = subscribeWhileEnabled(true, listen);
    expect(listen).toHaveBeenCalledTimes(1);
    stop?.();
    await flush();

    expect(unlisten.stop).toHaveBeenCalledTimes(1);
  });
});
