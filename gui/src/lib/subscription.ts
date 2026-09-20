// イベントの購読(Tauriのlistenが返す、購読を解除する関数のPromise)を、Reactの効果(useEffect)の後始末(同期の関数)へ
// 結ぶ。
//
// 購読が確立する前に後始末が呼ばれても、確立した時点で解除する(効果は、確立を待たずに片付けられるため)。後始末を
// 2回呼んでも、解除は1回だけ行う。

export function subscribeUntilStopped(listen: () => Promise<() => void>): () => void {
  const unlisten = listen();
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    void unlisten.then((stop) => stop());
  };
}

// enabledの間だけ購読する。無効なときは、購読せず、後始末も返さない。
export function subscribeWhileEnabled(
  enabled: boolean,
  listen: () => Promise<() => void>
): (() => void) | undefined {
  return enabled ? subscribeUntilStopped(listen) : undefined;
}
