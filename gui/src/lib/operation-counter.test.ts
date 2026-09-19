import { describe, expect, it } from "vitest";
import { createOperationCounter } from "./operation-counter";

// 決着を、テストから握れるPromise。
function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("createOperationCounter", () => {
  it("何も実行していなければ、待ちではない", () => {
    const counter = createOperationCounter(() => {});
    expect(counter.isBusy()).toBe(false);
  });

  it("実行を渡した時点で待ちになり、成功して決着すると、待ちでなくなる", async () => {
    const changes: boolean[] = [];
    const counter = createOperationCounter((busy) => changes.push(busy));
    const operation = deferred<string>();

    const tracked = counter.track(operation.promise);
    expect(counter.isBusy()).toBe(true);
    expect(changes).toEqual([true]);

    operation.resolve("done");
    expect(await tracked).toBe("done");
    expect(counter.isBusy()).toBe(false);
    expect(changes).toEqual([true, false]);
  });

  it("失敗して決着しても、必ず待ちでなくなり、失敗はそのまま呼び出し元へ伝わる(戻し忘れると、操作が無効のまま戻らない)", async () => {
    const counter = createOperationCounter(() => {});
    const operation = deferred();

    const tracked = counter.track(operation.promise);
    expect(counter.isBusy()).toBe(true);

    operation.reject(new Error("dummy failure"));
    await expect(tracked).rejects.toThrow("dummy failure");
    expect(counter.isBusy()).toBe(false);
  });

  it("重なった実行は、最後の1件が決着するまで、待ちのままである", async () => {
    const changes: boolean[] = [];
    const counter = createOperationCounter((busy) => changes.push(busy));
    const first = deferred();
    const second = deferred();

    const trackedFirst = counter.track(first.promise);
    const trackedSecond = counter.track(second.promise);

    first.resolve();
    await trackedFirst;
    expect(counter.isBusy()).toBe(true);
    expect(changes[changes.length - 1]).toBe(true);

    second.resolve();
    await trackedSecond;
    expect(counter.isBusy()).toBe(false);
    expect(changes[changes.length - 1]).toBe(false);
  });
});
