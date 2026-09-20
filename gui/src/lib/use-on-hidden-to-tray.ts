import { useEffect, useLayoutEffect, useRef } from "react";
import { onMainWindowHiddenToTray } from "./profile-ipc";
import { subscribeUntilStopped } from "./subscription";

// ウィンドウがトレイへ格納されたとき(Rust側が知らせる)に、handlerを呼ぶ。handlerは、描画のたびに新しくなるため、
// 最新のものを参照へ写し、購読は、この画面が表示されている間、1回だけにする。
export function useOnHiddenToTray(handler: () => void): void {
  const latestHandler = useRef(handler);
  useLayoutEffect(() => {
    latestHandler.current = handler;
  });

  useEffect(
    () => subscribeUntilStopped(() => onMainWindowHiddenToTray(() => latestHandler.current())),
    []
  );
}
