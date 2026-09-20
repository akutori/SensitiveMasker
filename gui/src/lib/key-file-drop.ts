import { getCurrentWebview } from "@tauri-apps/api/webview";
import { pickKeyFileFromDroppedPaths } from "./key-file";

// Tauri v2では、DOMのondropは発火せず、ウィンドウへドロップされたファイルのパスは、onDragDropEventで受け取る
// (ウィンドウの設定dragDropEnabledが、既定で有効)。

export interface KeyFileDropHandlers {
  // ファイルをドラッグして、ウィンドウの上に来ている間、true(ドロップの受け皿の強調に使う)。
  onActiveChange: (active: boolean) => void;
  // ドロップされたファイルが、鍵ファイルとして選べたとき。
  onPicked: (path: string) => void;
  // ドロップされたファイルが、鍵ファイルとして選べなかったとき(理由を、利用者へ示す)。
  onRejected: (message: string) => void;
}

// ドロップの購読を始める。返り値の関数(Promiseの解決値)で、購読をやめる。
export function subscribeKeyFileDrop(handlers: KeyFileDropHandlers): Promise<() => void> {
  return getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    switch (payload.type) {
      case "enter":
        handlers.onActiveChange(true);
        break;
      case "leave":
        handlers.onActiveChange(false);
        break;
      case "drop": {
        handlers.onActiveChange(false);
        const pick = pickKeyFileFromDroppedPaths(payload.paths);
        if (pick.kind === "picked") handlers.onPicked(pick.path);
        else handlers.onRejected(pick.message);
        break;
      }
      default:
        // over(ドラッグ中の位置の通知)は、何もしない。
        break;
    }
  });
}
