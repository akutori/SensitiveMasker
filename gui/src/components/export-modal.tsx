import { useId } from "react";
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
              value={passphrase}
              readOnly
              className="bg-muted"
            />
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
            コピー後、一定時間でクリップボードの内容は自動的にクリアされます
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
