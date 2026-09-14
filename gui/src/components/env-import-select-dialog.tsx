import { useEffect, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import type { EnvCandidate } from "@/lib/env-import-ipc";

const MASK_PLACEHOLDER = "••••••••••••";

export interface EnvImportSelectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  candidates: EnvCandidate[];
  onConfirm: (selected: EnvCandidate[]) => void;
}

export function EnvImportSelectDialog({
  open,
  onOpenChange,
  candidates,
  onConfirm,
}: EnvImportSelectDialogProps) {
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [revealed, setRevealed] = useState(false);

  // ファイルを選び直すたびにcandidatesは新しい配列になる。既定選択・マスク状態を
  // その都度リセットする(前回ファイルの選択状態を次のファイルへ持ち越さないため)。
  useEffect(() => {
    setSelectedKeys(new Set(candidates.filter((c) => c.includedByDefault).map((c) => c.key)));
    setRevealed(false);
  }, [candidates]);

  const toggleKey = (key: string, checked: boolean) => {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  };

  const included = candidates.filter((c) => c.includedByDefault);
  const excluded = candidates.filter((c) => !c.includedByDefault);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>取り込み対象を選択</DialogTitle>
        </DialogHeader>

        <div className="flex items-center justify-between">
          <p className="text-sm text-muted-foreground">.envから読み取った項目です。</p>
          <Button type="button" variant="outline" size="sm" onClick={() => setRevealed((v) => !v)}>
            {revealed ? <EyeOff /> : <Eye />}
            {revealed ? "値を隠す" : "値を表示"}
          </Button>
        </div>

        {candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground">取り込める項目が見つかりませんでした</p>
        ) : (
          <div className="grid max-h-64 gap-0.5 overflow-x-hidden overflow-y-auto">
            {included.map((c) => (
              <label key={c.key} className="flex min-w-0 items-center gap-2.5 rounded-md px-1 py-1.5">
                <Checkbox
                  checked={selectedKeys.has(c.key)}
                  onCheckedChange={(checked) => toggleKey(c.key, checked === true)}
                />
                <span className="min-w-0 flex-1">
                  <span className="block text-sm font-medium">{c.key}</span>
                  <span className="block truncate font-mono text-xs text-muted-foreground">
                    {revealed ? c.value : MASK_PLACEHOLDER}
                  </span>
                </span>
              </label>
            ))}

            {excluded.length > 0 && (
              <p className="px-1 pt-2 pb-0.5 text-xs text-muted-foreground">
                除外された項目({excluded.length}件)
              </p>
            )}
            {excluded.map((c) => (
              <label key={c.key} className="flex min-w-0 items-center gap-2.5 rounded-md px-1 py-1.5">
                <Checkbox
                  checked={selectedKeys.has(c.key)}
                  onCheckedChange={(checked) => toggleKey(c.key, checked === true)}
                />
                <span className="min-w-0 flex-1">
                  <span className="block text-sm">{c.key}</span>
                  <span className="block truncate font-mono text-xs text-muted-foreground">
                    {revealed ? c.value : MASK_PLACEHOLDER}
                  </span>
                </span>
              </label>
            ))}
          </div>
        )}

        <DialogFooter className="items-center sm:justify-between">
          <span className="text-sm text-muted-foreground">{selectedKeys.size}件選択中</span>
          <div className="flex gap-2">
            <DialogClose asChild>
              <Button type="button" variant="outline">
                キャンセル
              </Button>
            </DialogClose>
            <Button
              type="button"
              disabled={selectedKeys.size === 0}
              onClick={() => onConfirm(candidates.filter((c) => selectedKeys.has(c.key)))}
            >
              次へ
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
