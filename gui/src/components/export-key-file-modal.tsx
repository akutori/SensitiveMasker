import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

// idle: 何も始めていない。choosingKeyFile・choosingExportFile: 保存先を選んでいる(まだ何も書き出していないため、閉じられる。
// 閉じると、書き出しは取り消される)。writing: 書き込み中(閉じる操作を受け付けない。書き込みが終わると、呼び出し元が閉じる)。
export type ExportKeyFilePhase = "idle" | "choosingKeyFile" | "choosingExportFile" | "writing";

export interface ExportKeyFileModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  target: string;
  phase: ExportKeyFilePhase;
  onExport: () => void;
}

const PROGRESS_TEXT: Record<ExportKeyFilePhase, string> = {
  idle: "",
  choosingKeyFile: "鍵ファイルの保存先を選択しています…",
  choosingExportFile: "エクスポートするファイルの保存先を選択しています…",
  writing: "書き込んでいます…",
};

export function ExportKeyFileModal({ open, onOpenChange, target, phase, onExport }: ExportKeyFileModalProps) {
  const writing = phase === "writing";
  const busy = phase !== "idle";

  // Escapeと背景を押す操作は、書き込み中だけ、受け付けない(保存先を選ぶ間は、何も書き出していないため、閉じられる)。
  const preventImplicitClose = (event: Event) => {
    if (writing) event.preventDefault();
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={!writing}
        onEscapeKeyDown={preventImplicitClose}
        onInteractOutside={preventImplicitClose}
      >
        <DialogHeader>
          <DialogTitle>エクスポート: {target}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-2 text-sm">
          <p>
            エクスポートしたファイルを復号するための鍵ファイル(.smxkey)を、別に保存します。鍵ファイルは、エクスポートした
            ファイルとは別の場所に、保管してください。
          </p>
          <p className="font-medium">この鍵ファイルを紛失すると、エクスポートしたファイルは二度と復号できません。</p>
          {/* 実行の進み具合。領域は、実行の前から置いておく(領域ごと後から現れると、読み上げられないことがあるため)。
              1行分の高さを常に確保し、文が出入りしても、画面の高さが動かないようにする。 */}
          <p className="min-h-5 text-muted-foreground" aria-live="polite">
            {PROGRESS_TEXT[phase]}
          </p>
        </div>

        {/* ボタンの行は、どの幅でも横並びにする(縦に積まれる幅では、文言の分だけ、高さが増える)。 */}
        <DialogFooter className="flex-row justify-end">
          <Button onClick={onExport} disabled={busy}>
            エクスポート
          </Button>
          <DialogClose asChild>
            <Button variant="outline" disabled={writing}>
              キャンセル
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
