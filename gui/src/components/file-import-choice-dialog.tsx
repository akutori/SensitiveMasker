import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";

export interface FileImportChoiceDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  filePath: string;
  onLoadIntoInput: () => void;
  onMaskAndSaveAs: () => void;
}

export function FileImportChoiceDialog({
  open,
  onOpenChange,
  filePath,
  onLoadIntoInput,
  onMaskAndSaveAs,
}: FileImportChoiceDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>ファイル取り込み方法の選択</DialogTitle>
        </DialogHeader>

        <div className="grid gap-2">
          <Label>選択したファイル:</Label>
          <div
            className="rounded-lg border border-input px-2.5 py-1.5 text-sm break-all"
            data-a11y-verified-contrast="dialog-overlay-geometry-false-positive"
          >
            {filePath}
          </div>
        </div>

        <div className="grid gap-8">
          <Label>ファイルの取り込み方法を選択してください:</Label>
          <div className="grid gap-2">
            <Button variant="outline" className="h-auto w-full py-2" onClick={onLoadIntoInput}>
              入力テキスト欄に読み込む
            </Button>
            <Button
              variant="outline"
              className="h-auto w-full py-2"
              onClick={onMaskAndSaveAs}
            >
              直接マスクして別ファイルに保存
            </Button>
          </div>
        </div>

        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline">キャンセル</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
