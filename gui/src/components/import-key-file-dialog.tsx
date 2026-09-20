import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { ValidationErrorBox } from "./validation-error-box";

export interface ImportKeyFileDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  // 取り込むファイル(.smx)の名前。
  fileName: string;
  // 選んだ鍵ファイルの名前(選んでいなければnull)。
  keyFileName: string | null;
  // ファイルをドラッグして、ウィンドウの上に来ている間、true(ドロップの受け皿を強調する)。
  dragActive: boolean;
  onSelectKeyFile: () => void;
  errorMessage?: string;
  onConfirm: () => void;
  // 復号している間はtrue。OKを押せない(復号が重なるのを防ぐ)。閉じることはできる。
  busy?: boolean;
}

export function ImportKeyFileDialog({
  open,
  onOpenChange,
  fileName,
  keyFileName,
  dragActive,
  onSelectKeyFile,
  errorMessage,
  onConfirm,
  busy = false,
}: ImportKeyFileDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>鍵ファイルを選択</DialogTitle>
          <DialogDescription>選択したファイル: {fileName}</DialogDescription>
        </DialogHeader>

        <p className="text-sm">
          このファイルは、鍵ファイルで暗号化されています。エクスポートするときに保存した鍵ファイル(.smxkey)を、選択するか、
          ここへドロップしてください。
        </p>

        {/* ドロップの受け皿。ドロップは、マウス操作だけのため、同じことを、キーボードでも行える「鍵ファイルを選択」を置く。 */}
        <div
          data-drag-active={dragActive}
          className={
            dragActive
              ? "flex flex-col items-center gap-2 rounded-md border-2 border-dashed border-primary bg-muted p-4 text-sm"
              : "flex flex-col items-center gap-2 rounded-md border-2 border-dashed border-border p-4 text-sm"
          }
        >
          <span className={keyFileName ? "font-medium" : "text-muted-foreground"}>
            {dragActive ? "ここへドロップしてください" : (keyFileName ?? "鍵ファイルが選択されていません")}
          </span>
          <Button type="button" variant="outline" onClick={onSelectKeyFile} disabled={busy}>
            鍵ファイルを選択…
          </Button>
        </div>

        {errorMessage && <ValidationErrorBox message={errorMessage} />}

        {/* ボタンの行は、どの幅でも横並びにする(縦に積まれる幅では、文言の分だけ、高さが増える)。 */}
        <DialogFooter className="flex-row justify-end">
          {/* OKが無効になる理由を、見える文言と読み上げの両方で伝える。領域は、復号する前から置いておく
              (領域ごと後から現れると、読み上げられないことがあるため)。 */}
          <p
            role="status"
            className={busy ? "mr-auto self-center text-sm text-muted-foreground" : "sr-only"}
          >
            {busy ? "復号しています…" : null}
          </p>
          <Button onClick={onConfirm} disabled={busy || keyFileName === null} aria-busy={busy}>
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
