import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { openFileDialog } from "./file-dialog";
import { KEY_FILE_FILTERS } from "./key-file";
import { subscribeKeyFileDrop, type KeyFileDropHandlers } from "./key-file-drop";

// 鍵ファイルで復号するインポートの、鍵ファイル入力画面の状態(選んだ鍵ファイル・エラーの文・ドラッグ中か)と、
// 「鍵ファイルを選択」の操作を、メイン画面・プロファイル管理画面で共有する。
// enabledは、この画面が開いている間だけtrue(ドロップの購読は、その間だけ行う)。

function useKeyFileDrop(enabled: boolean, handlers: KeyFileDropHandlers): void {
  const latest = useRef(handlers);
  useLayoutEffect(() => {
    latest.current = handlers;
  });

  useEffect(() => {
    if (!enabled) return;
    const unlisten = subscribeKeyFileDrop({
      onActiveChange: (active) => latest.current.onActiveChange(active),
      onPicked: (path) => latest.current.onPicked(path),
      onRejected: (message) => latest.current.onRejected(message),
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [enabled]);
}

export function useImportKeyFile(enabled: boolean) {
  const [keyFilePath, setKeyFilePath] = useState<string | null>(null);
  const [error, setError] = useState<string | undefined>();
  const [dragActive, setDragActive] = useState(false);

  useKeyFileDrop(enabled, {
    onActiveChange: setDragActive,
    onPicked: (path) => {
      setKeyFilePath(path);
      setError(undefined);
    },
    onRejected: setError,
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
