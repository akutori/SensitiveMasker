import { useEffect, useId, useState } from "react";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ValidationErrorBox } from "./validation-error-box";
import { DeleteConfirmDialog } from "./delete-confirm-dialog";

export interface Tag {
  id: string;
  name: string;
}

export interface TagManagementDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  tags: Tag[];
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
  newTagName: string;
  onNewTagNameChange: (name: string) => void;
  onAddTag: () => void;
  onRenameTag: (id: string, newName: string) => void;
  onDeleteTag: (id: string) => void;
  errorMessage?: string;
  invalidTagId?: string | "new";
}

export function TagManagementDialog({
  open,
  onOpenChange,
  tags,
  searchQuery,
  onSearchQueryChange,
  newTagName,
  onNewTagNameChange,
  onAddTag,
  onRenameTag,
  onDeleteTag,
  errorMessage,
  invalidTagId,
}: TagManagementDialogProps) {
  const id = useId();
  const [editingTagId, setEditingTagId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");
  const [lastSubmittedName, setLastSubmittedName] = useState<string | null>(null);
  const [pendingDeleteTag, setPendingDeleteTag] = useState<Tag | null>(null);

  const filteredTags = tags.filter((tag) =>
    tag.name.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const startRename = (tag: Tag) => {
    setEditingTagId(tag.id);
    setDraftName(tag.name);
    setLastSubmittedName(null);
  };

  const commitRename = (tag: Tag) => {
    setLastSubmittedName(draftName);
    onRenameTag(tag.id, draftName);
  };

  const cancelRename = () => {
    setEditingTagId(null);
    setDraftName("");
  };

  // 直前に送信した値からdraftNameが変わったら、古いエラーは既に無関係なので表示しない
  // (親が新しいerrorMessageを送ってくるまで待たず、入力修正の時点で即座にクリアされたように見せる)
  const isRenameErrorVisible = (tag: Tag) =>
    invalidTagId === tag.id && !!errorMessage && draftName === lastSubmittedName;

  // リネームが重複名等で失敗した場合、親はtagsを更新しないため編集状態を維持する
  // (エラーはinvalidTagId/errorMessage経由で編集中の行に表示される)。
  // 実際に名前が反映されたことを検知できた時だけ編集モードを閉じる。
  useEffect(() => {
    const current = tags.find((tag) => tag.id === editingTagId);
    if (editingTagId !== null && current?.name === draftName) {
      setEditingTagId(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tags]);

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>タグ管理</DialogTitle>
          </DialogHeader>

          <Input
            placeholder="🔍 タグ名で検索..."
            value={searchQuery}
            onChange={(e) => onSearchQueryChange(e.target.value)}
          />

          <div>
            <div className="flex items-center gap-2">
              <Input
                id={`${id}-new-tag`}
                placeholder="新しいタグ名"
                value={newTagName}
                aria-invalid={invalidTagId === "new"}
                onChange={(e) => onNewTagNameChange(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") onAddTag();
                }}
              />
              <Button
                variant="outline"
                onClick={onAddTag}
                disabled={newTagName.trim().length === 0}
              >
                + 追加
              </Button>
            </div>
            {invalidTagId === "new" && errorMessage && (
              <div className="mt-2">
                <ValidationErrorBox message={errorMessage} />
              </div>
            )}
          </div>

          <div className="max-h-64 overflow-y-auto border-t pt-3">
            <div className="grid gap-2">
            {tags.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                タグがまだ登録されていません
              </p>
            ) : filteredTags.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                該当するタグが見つかりません
              </p>
            ) : (
              filteredTags.map((tag) => (
                <div key={tag.id}>
                  {editingTagId === tag.id ? (
                    <div className="flex items-center gap-2">
                      <Input
                        autoFocus
                        value={draftName}
                        aria-invalid={isRenameErrorVisible(tag)}
                        onChange={(e) => setDraftName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitRename(tag);
                          if (e.key === "Escape") cancelRename();
                        }}
                      />
                      <Button size="sm" onClick={() => commitRename(tag)}>
                        保存
                      </Button>
                      <Button size="sm" variant="outline" onClick={cancelRename}>
                        キャンセル
                      </Button>
                    </div>
                  ) : (
                    <div className="flex items-center gap-2">
                      <div
                        className="flex-1 rounded-lg border border-input px-2.5 py-1.5 text-sm"
                        data-a11y-verified-contrast="dialog-overlay-geometry-false-positive"
                      >
                        {tag.name}
                      </div>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => startRename(tag)}
                      >
                        リネーム
                      </Button>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => setPendingDeleteTag(tag)}
                      >
                        削除
                      </Button>
                    </div>
                  )}
                  {isRenameErrorVisible(tag) && errorMessage && (
                    <div className="mt-2">
                      <ValidationErrorBox message={errorMessage} />
                    </div>
                  )}
                </div>
              ))
            )}
            </div>
          </div>

          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">閉じる</Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <DeleteConfirmDialog
        open={pendingDeleteTag !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingDeleteTag(null);
        }}
        targetType="タグ"
        targetName={pendingDeleteTag?.name ?? ""}
        onConfirm={() => {
          if (pendingDeleteTag) onDeleteTag(pendingDeleteTag.id);
          setPendingDeleteTag(null);
        }}
      />
    </>
  );
}
