import { useId } from "react";
import { cn } from "cn";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Button } from "@/components/ui/button";

export interface TemplateOption {
  value: string;
  label: string;
}

export const DEFAULT_TEMPLATES: TemplateOption[] = [
  { value: "general", label: "汎用 (general)" },
  { value: "sip", label: "SIP" },
];

export interface TemplateSelectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  templates: TemplateOption[];
  value: string;
  onValueChange: (value: string) => void;
  onConfirm: () => void;
}

export function TemplateSelectDialog({
  open,
  onOpenChange,
  templates,
  value,
  onValueChange,
  onConfirm,
}: TemplateSelectDialogProps) {
  const groupId = useId();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>テンプレートを選択</DialogTitle>
        </DialogHeader>

        <div className="grid gap-2">
          <Label id={groupId}>元になるテンプレートを選んでください:</Label>
          <RadioGroup
            value={value}
            onValueChange={onValueChange}
            aria-labelledby={groupId}
            className="py-2"
          >
            {templates.map((template, index) => (
              <div
                key={template.value}
                className={cn(
                  "flex items-center gap-2",
                  index === 0 && "pt-1.5",
                  index === templates.length - 1 && "pb-1.5"
                )}
              >
                <RadioGroupItem
                  value={template.value}
                  id={`${groupId}-${template.value}`}
                />
                <Label htmlFor={`${groupId}-${template.value}`}>
                  {template.label}
                </Label>
              </div>
            ))}
          </RadioGroup>
        </div>

        <DialogFooter>
          <Button onClick={onConfirm}>OK</Button>
          <DialogClose asChild>
            <Button variant="outline">キャンセル</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
