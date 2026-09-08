import { useId } from "react";
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>パスフレーズを入力</DialogTitle>
          <DialogDescription>選択したファイル: {fileName}</DialogDescription>
        </DialogHeader>

        <div className="grid gap-2">
          <Label htmlFor={inputId}>パスフレーズ:</Label>
          <Input
            id={inputId}
            type="password"
            value={passphrase}
            onChange={(e) => onPassphraseChange(e.target.value)}
            aria-invalid={!!errorMessage}
          />
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
