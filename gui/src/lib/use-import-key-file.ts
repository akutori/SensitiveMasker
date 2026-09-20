import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { openFileDialog } from "./file-dialog";
import { KEY_FILE_FILTERS } from "./key-file";
import { subscribeKeyFileDrop, type KeyFileDropHandlers } from "./key-file-drop";
import { subscribeWhileEnabled } from "./subscription";

// 鍵ファイルで復号するインポートの、鍵ファイル入力画面の状態(選んだ鍵ファイル・エラーの文・ドラッグ中か)と、
// 「鍵ファイルを選択」の操作を、メイン画面・プロファイル管理画面で共有する。
// enabledは、この画面が開いている間だけtrue(ドロップの購読は、その間だけ行う)。busyは、復号している間だけtrue
// (その間は、鍵ファイルを選び直せない。復号の結果の文言と、画面に出ている鍵ファイルの名前が、食い違うため)。

function useKeyFileDrop(enabled: boolean, handlers: KeyFileDropHandlers): void {
  const latest = useRef(handlers);
  useLayoutEffect(() => {
    latest.current = handlers;
  });

  useEffect(
    () =>
      subscribeWhileEnabled(enabled, () =>
        subscribeKeyFileDrop({
          onActiveChange: (active) => latest.current.onActiveChange(active),
          onPicked: (path) => latest.current.onPicked(path),
          onRejected: (message) => latest.current.onRejected(message),
        })
      ),
    [enabled]
  );
}

export function useImportKeyFile(enabled: boolean, busy: boolean) {
  const [keyFilePath, setKeyFilePath] = useState<string | null>(null);
  const [error, setError] = useState<string | undefined>();
  const [dragActive, setDragActive] = useState(false);

  useKeyFileDrop(enabled, {
    onActiveChange: setDragActive,
    onPicked: (path) => {
      if (busy) return;
      setKeyFilePath(path);
      setError(undefined);
    },
    onRejected: (message) => {
      if (!busy) setError(message);
    },
  });

  const selectKeyFile = async () => {
    const path = await openFileDialog({ multiple: false, filters: KEY_FILE_FILTERS }, "key");
    if (!path || Array.isArray(path)) return;
    setKeyFilePath(path);
    setError(undefined);
  };

  // 開くたびに、前回の選択・エラー・強調を引き継がない。
  const reset = () => {
    setKeyFilePath(null);
    setError(undefined);
    setDragActive(false);
  };

  return { keyFilePath, error, setError, dragActive, selectKeyFile, reset };
}
