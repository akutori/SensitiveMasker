import { useId, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { flushSync } from "react-dom";
import { Eye, EyeOff } from "lucide-react";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { ValidationErrorBox } from "./validation-error-box";

export interface ImportPassphraseDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  fileName: string;
  passphrase: string;
  onPassphraseChange: (value: string) => void;
  errorMessage?: string;
  onConfirm: () => void;
  // 復号している間はtrue。OKを押せず、入力も変えられない(復号が重なるのを防ぐ)。閉じることはできる。
  busy?: boolean;
}

export function ImportPassphraseDialog({
  open,
  onOpenChange,
  fileName,
  passphrase,
  onPassphraseChange,
  errorMessage,
  onConfirm,
  busy = false,
}: ImportPassphraseDialogProps) {
  const inputId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [revealed, setRevealed] = useState(false);

  // 開くたびに伏せ字から始める(前回の表示状態を引き継がない)。呼び出し元はopenプロパティ
  // だけを切り替えるため、revealedはopen=falseの間も保持され続ける。
  useLayoutEffect(() => {
    if (open) setRevealed(false);
  }, [open]);

  // マウスやタッチで「表示」を押した後も、続けて入力できるよう、入力欄へフォーカスとキャレット位置を
  // 戻す(ボタンにフォーカスが残ると、続けて打った文字が入力されない)。キーボードで押したときは、
  // 繰り返し切り替えられるよう、フォーカスをボタンに残す(キーボード操作のclickはdetailが0)。
  const toggleRevealed = (event: MouseEvent<HTMLButtonElement>) => {
    const input = inputRef.current;
    const selectionStart = input?.selectionStart ?? null;
    const selectionEnd = input?.selectionEnd ?? null;
    // 入力欄の種類の切り替えを、フォーカスとキャレットの復元より前に反映させる。
    flushSync(() => setRevealed((v) => !v));
    if (event.detail === 0 || !input) return;
    input.focus();
    if (selectionStart !== null && selectionEnd !== null) {
      input.setSelectionRange(selectionStart, selectionEnd);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>パスフレーズを入力</DialogTitle>
          <DialogDescription>選択したファイル: {fileName}</DialogDescription>
        </DialogHeader>

        <div className="grid gap-2">
          <Label htmlFor={inputId}>パスフレーズ:</Label>
          <div className="flex gap-2">
            <Input
              id={inputId}
              ref={inputRef}
              type={revealed ? "text" : "password"}
              value={passphrase}
              onChange={(e) => onPassphraseChange(e.target.value)}
              readOnly={busy}
              aria-invalid={!!errorMessage}
              // 表示にしたとき、入力したパスフレーズが自動補完の履歴や綴り確認の対象にならないようにする。
              // WebView2はこの属性だけでは自動補完を止めない場合があるため、tauri.conf.jsonでも無効にしている。
              autoComplete="off"
              spellCheck={false}
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={toggleRevealed}
              aria-controls={inputId}
              aria-label={revealed ? "パスフレーズを隠す" : "パスフレーズを表示"}
            >
              {revealed ? <EyeOff /> : <Eye />}
            </Button>
          </div>
        </div>

        {errorMessage && <ValidationErrorBox message={errorMessage} />}

        <DialogFooter>
          <Button
            onClick={onConfirm}
            disabled={busy || passphrase.trim().length === 0}
            aria-busy={busy}
          >
            OK
          </Button>
          <DialogClose asChild>
            <Button variant="outline">キャンセル</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
