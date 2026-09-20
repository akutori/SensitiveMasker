import { useId, useLayoutEffect, useRef, useState } from "react";
import { CircleCheck, Eye, EyeOff } from "lucide-react";
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
import { CLIPBOARD_CLEAR_DELAY_SECONDS } from "@/lib/clipboard-clear-delay";
import { isPassphraseAtRisk, type ExportPhase } from "@/lib/export-dialog-state";

export interface ExportModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  target: string;
  passphrase: string;
  // editing: パスフレーズの確認・コピー・再生成・エクスポートができる。
  // choosing: 保存先を選んでいる(まだ何も書き出していない)。書き出すパスフレーズを変えさせないため、
  //   再生成・エクスポートは無効にするが、閉じる操作は受け付ける(閉じると、書き出しは取り消される)。
  // writing: 書き込み中。書き出すパスフレーズを変えさせず、失わせないため、全ての操作を無効にし、
  //   閉じる操作も受け付けない。
  // exported: 書き出し済み。パスフレーズの表示・コピーと、閉じることだけができる
  //   (再生成できると、書き出したファイルと画面のパスフレーズが食い違うため)。
  //   唯一の表示を誤って失わないよう、閉じるのは「閉じる」と×だけにする。
  status: ExportPhase;
  // コピー/クリアのIPC応答待ちの間はtrue。この間はコピー・再生成を無効化する(応答が
  // 返る前にもう一方を押すと世代カウンタが進み、自動クリアの設置が行われなくなるため)。
  clipboardBusy: boolean;
  onCopy: () => void;
  onRegenerate: () => void;
  onExport: () => void;
}

export function ExportModal({
  open,
  onOpenChange,
  target,
  passphrase,
  status,
  clipboardBusy,
  onCopy,
  onRegenerate,
  onExport,
}: ExportModalProps) {
  const inputId = useId();
  const passphraseInputRef = useRef<HTMLInputElement>(null);
  const [revealed, setRevealed] = useState(false);
  const choosing = status === "choosing";
  const writing = status === "writing";
  const exported = status === "exported";

  // 実行中(保存先の選択中・書き込み中)は、押したボタンが無効になってフォーカスを失い、フォーカストラップが
  // 効かなくなる(Tabで背景の画面へ出られる)ため、フォーカスを入力欄へ移しておく。
  useLayoutEffect(() => {
    if (choosing || writing) passphraseInputRef.current?.focus();
  }, [choosing, writing]);

  // Escapeと背景を押す操作は、操作の意図を確かめずに閉じてしまう。編集中(まだ何も書き出して
  // いない)以外は、パスフレーズを失うため受け付けない。
  const preventImplicitClose = (event: Event) => {
    if (isPassphraseAtRisk(status)) event.preventDefault();
  };

  // 開くたびに非表示から始める(前回の表示状態を引き継がない)。呼び出し元は
  // このコンポーネント自体を条件付きレンダリングせずopenプロパティのみ切り替えるため、
  // revealed自体はopen=falseの間も(Dialog内部の描画状態と無関係に)保持され続ける。
  // useEffect(ペイント後に発火)だと前回revealed=trueのまま新しいパスフレーズが
  // 一瞬平文で描画されてしまうため、useLayoutEffectでペイント前に補正する。
  useLayoutEffect(() => {
    if (open) setRevealed(false);
  }, [open]);

  // 成功した時点で伏せ字へ戻す(表示したまま、画面共有や録画に残さないため)。
  useLayoutEffect(() => {
    if (exported) setRevealed(false);
  }, [exported]);

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

        <div className="grid gap-2">
          <Label htmlFor={inputId}>生成されたパスフレーズ(自動生成):</Label>
          <div className="flex gap-2">
            <Input
              id={inputId}
              ref={passphraseInputRef}
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
              disabled={writing}
              aria-controls={inputId}
              aria-label={revealed ? "パスフレーズを隠す" : "パスフレーズを表示"}
            >
              {revealed ? <EyeOff /> : <Eye />}
            </Button>
            <Button variant="outline" onClick={onCopy} disabled={clipboardBusy || writing}>
              コピー
            </Button>
            {!exported && (
              <Button
                variant="outline"
                onClick={onRegenerate}
                disabled={clipboardBusy || choosing || writing}
              >
                再生成
              </Button>
            )}
          </div>
          {/* 読み上げの領域は、書き出す前から置いておく(領域ごと後から現れると、読み上げられないことがあるため)。 */}
          <div
            role="status"
            className={
              exported
                ? "flex items-start gap-2 rounded-md border border-border bg-muted p-3 text-sm text-foreground"
                : "sr-only"
            }
          >
            {exported && (
              <>
                <CircleCheck className="mt-0.5 size-4 shrink-0" />
                <span>
                  エクスポートが完了しました。このパスフレーズは、書き出したファイルを取り込むときに必要です。この画面を閉じると、二度と表示できません。
                </span>
              </>
            )}
          </div>
          {/* 実行の進み具合。領域は、実行の前から置いておく(領域ごと後から現れると、読み上げられないことがあるため)。
              1行分の高さを常に確保し、文が出入りしても、画面の高さが動かないようにする。 */}
          <p className="min-h-5 text-sm text-muted-foreground" aria-live="polite">
            {choosing && "保存先を選択しています…(この画面を閉じると、書き出しは取り消されます)"}
            {writing && "書き込んでいます…"}
          </p>
          <p
            className="text-sm text-muted-foreground"
            data-a11y-verified-contrast="dialog-overlay-geometry-false-positive"
          >
            コピーの{CLIPBOARD_CLEAR_DELAY_SECONDS}秒後に、自動クリアを試みます(確実ではありません)。Windows環境ではクリップボード履歴・クラウド同期の対象から除外されます。それ以外の環境では現時点で未対応のため、クリア後も履歴に残る場合があります。確実に消すには手動でクリアしてください。
          </p>
        </div>

        <DialogFooter>
          {exported ? (
            <DialogClose asChild>
              <Button>閉じる</Button>
            </DialogClose>
          ) : (
            <>
              <Button onClick={onExport} disabled={choosing || writing}>
                エクスポート
              </Button>
              <DialogClose asChild>
                <Button variant="outline" disabled={writing}>
                  キャンセル
                </Button>
              </DialogClose>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
