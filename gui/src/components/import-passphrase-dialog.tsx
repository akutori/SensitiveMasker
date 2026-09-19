import { useId, useLayoutEffect, useState } from "react";
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
}

export function ImportPassphraseDialog({
  open,
  onOpenChange,
  fileName,
  passphrase,
  onPassphraseChange,
  errorMessage,
  onConfirm,
}: ImportPassphraseDialogProps) {
  const inputId = useId();
  const [revealed, setRevealed] = useState(false);

  // 開くたびに伏せ字から始める(前回の表示状態を引き継がない)。呼び出し元はopenプロパティ
  // だけを切り替えるためrevealedはopen=falseの間も保持され続け、useEffect(ペイント後に
  // 発火)だと前回revealed=trueのまま一瞬平文で描画されるため、useLayoutEffectで補正する。
  useLayoutEffect(() => {
    if (open) setRevealed(false);
  }, [open]);

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
              type={revealed ? "text" : "password"}
              value={passphrase}
              onChange={(e) => onPassphraseChange(e.target.value)}
              aria-invalid={!!errorMessage}
              // 表示にしたとき、入力したパスフレーズが自動補完の履歴や綴り確認の対象にならないようにする。
              autoComplete="off"
              spellCheck={false}
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={() => setRevealed((v) => !v)}
              aria-label={revealed ? "パスフレーズを隠す" : "パスフレーズを表示"}
            >
              {revealed ? <EyeOff /> : <Eye />}
            </Button>
          </div>
        </div>

        {errorMessage && <ValidationErrorBox message={errorMessage} />}

        <DialogFooter>
          <Button onClick={onConfirm} disabled={passphrase.trim().length === 0}>
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
