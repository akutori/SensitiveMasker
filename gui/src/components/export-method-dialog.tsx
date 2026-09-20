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
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Button } from "@/components/ui/button";

// エクスポートしたファイルを復号する方法。key_file: 鍵ファイル(別に保存する、復号のための鍵)。passphrase: 生成した
// パスフレーズ。どちらの方法でも、取り込むときは、ファイルの先頭のヘッダーから、方法が自動で分かる。
export type ExportMethod = "key_file" | "passphrase";

export interface ExportMethodDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  // エクスポートの対象(プロファイル名・「全プロファイル」)。
  target: string;
  method: ExportMethod;
  onMethodChange: (method: ExportMethod) => void;
  onNext: () => void;
}

// 上に置いたものが、推奨(呼び出し元は、初期値をkey_fileにする)。
const OPTIONS: ReadonlyArray<{ value: ExportMethod; title: string; description: string }> = [
  { value: "key_file", title: "鍵ファイル(推奨)", description: "復号キーを使用してインポートします" },
  { value: "passphrase", title: "パスフレーズ", description: "生成されたパスフレーズを使ってインポートします" },
];

export function ExportMethodDialog({
  open,
  onOpenChange,
  target,
  method,
  onMethodChange,
  onNext,
}: ExportMethodDialogProps) {
  const groupId = useId();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>エクスポート: {target}</DialogTitle>
          <DialogDescription>エクスポートしたファイルを、どの方法でインポート(復号)するかを選んでください。</DialogDescription>
        </DialogHeader>

        <RadioGroup
          value={method}
          onValueChange={(value) => onMethodChange(value as ExportMethod)}
          aria-label="インポートの方法"
        >
          {OPTIONS.map((option) => {
            const id = `${groupId}-${option.value}`;
            return (
              <label
                key={option.value}
                htmlFor={id}
                className="flex cursor-pointer items-start gap-3 rounded-md border border-border p-3 has-[[aria-checked=true]]:border-primary has-[[aria-checked=true]]:bg-muted"
              >
                <RadioGroupItem id={id} value={option.value} className="mt-0.5" />
                <span className="grid gap-0.5">
                  <span className="font-medium">{option.title}</span>
                  <span className="text-sm text-muted-foreground">{option.description}</span>
                </span>
              </label>
            );
          })}
        </RadioGroup>

        {/* ボタンの行は、どの幅でも横並びにする(縦に積まれる幅では、文言の分だけ、高さが増える)。 */}
        <DialogFooter className="flex-row justify-end">
          <Button onClick={onNext}>次へ</Button>
          <DialogClose asChild>
            <Button variant="outline">キャンセル</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
