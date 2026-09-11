import { useId, useLayoutEffect, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";

export interface ExportModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  target: string;
  passphrase: string;
  onCopy: () => void;
  onRegenerate: () => void;
  onExport: () => void;
}

export function ExportModal({
  open,
  onOpenChange,
  target,
  passphrase,
  onCopy,
  onRegenerate,
  onExport,
}: ExportModalProps) {
  const inputId = useId();
  const [revealed, setRevealed] = useState(false);

  // 開くたびに非表示から始める(前回の表示状態を引き継がない)。呼び出し元は
  // このコンポーネント自体を条件付きレンダリングせずopenプロパティのみ切り替えるため、
  // revealed自体はopen=falseの間も(Dialog内部の描画状態と無関係に)保持され続ける。
  // useEffect(ペイント後に発火)だと前回revealed=trueのまま新しいパスフレーズが
  // 一瞬平文で描画されてしまうため、useLayoutEffectでペイント前に補正する。
  useLayoutEffect(() => {
    if (open) setRevealed(false);
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>エクスポート: {target}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-2">
          <Label htmlFor={inputId}>生成されたパスフレーズ(自動生成):</Label>
          <div className="flex gap-2">
            <Input
              id={inputId}
              type={revealed ? "text" : "password"}
              value={passphrase}
              readOnly
              className="bg-muted"
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
            <Button variant="outline" onClick={onCopy}>
              コピー
            </Button>
            <Button variant="outline" onClick={onRegenerate}>
              再生成
            </Button>
          </div>
          <p
            className="text-sm text-muted-foreground"
            data-a11y-verified-contrast="dialog-overlay-geometry-false-positive"
          >
            コピー後、自動クリアを試みます(確実ではありません)。Windows環境ではクリップボード履歴・クラウド同期の対象から除外されます。それ以外の環境では現時点で未対応のため、クリア後も履歴に残る場合があります。確実に消すには手動でクリアしてください。
          </p>
        </div>

        <DialogFooter>
          <Button onClick={onExport}>エクスポート</Button>
          <DialogClose asChild>
            <Button variant="outline">キャンセル</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
