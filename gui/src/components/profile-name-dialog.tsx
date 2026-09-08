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
import { ValidationErrorBox } from "./validation-error-box";

export interface ProfileNameDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  name: string;
  onNameChange: (value: string) => void;
  errorMessage?: string;
  onConfirm: () => void;
}

export function ProfileNameDialog({
  open,
  onOpenChange,
  name,
  onNameChange,
  errorMessage,
  onConfirm,
}: ProfileNameDialogProps) {
  const inputId = useId();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>プロファイル名を入力</DialogTitle>
        </DialogHeader>

        <div className="grid gap-2">
          <Label htmlFor={inputId}>プロファイル名</Label>
          <Input
            id={inputId}
            value={name}
            onChange={(e) => onNameChange(e.target.value)}
            aria-invalid={!!errorMessage}
          />
        </div>

        {errorMessage && <ValidationErrorBox message={errorMessage} />}

        <DialogFooter>
          <Button onClick={onConfirm} disabled={name.trim().length === 0}>
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
