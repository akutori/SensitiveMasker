import { describe, expect, it } from "vitest";
import { suppressContextMenu } from "./suppress-context-menu";

const contextMenu = () => new Event("contextmenu", { cancelable: true });

describe("suppressContextMenu", () => {
  it("contextmenuの既定の動作(メニューを出すこと)を止める", () => {
    const target = new EventTarget();
    suppressContextMenu(target);

    const event = contextMenu();
    target.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
  });

  it("contextmenu以外のイベントは、止めない", () => {
    const target = new EventTarget();
    suppressContextMenu(target);

    const event = new Event("click", { cancelable: true });
    target.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
  });

  it("返された関数を呼ぶと、止めるのをやめる(以後のcontextmenuは、既定の動作のまま)", () => {
    const target = new EventTarget();
    const stop = suppressContextMenu(target);
    stop();

    const event = contextMenu();
    target.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
  });

  it("止めていない間に、後から止め直せる", () => {
    const target = new EventTarget();
    suppressContextMenu(target)();
    suppressContextMenu(target);

    const event = contextMenu();
    target.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
  });

  it("画面の中の要素が、伝播を止めても、キャプチャの段階で止まる", () => {
    // Nodeの素のEventTargetには、キャプチャの経路が無い。要素の入れ子は、addEventListenerの第3引数で、キャプチャを
    // 実装が指定していることで確かめる。
    const calls: Array<[string, unknown]> = [];
    const target = {
      addEventListener: (type: string, _listener: unknown, options: unknown) => calls.push([type, options]),
      removeEventListener: () => {},
    } as unknown as EventTarget;

    suppressContextMenu(target);

    expect(calls).toEqual([["contextmenu", { capture: true }]]);
  });
});
