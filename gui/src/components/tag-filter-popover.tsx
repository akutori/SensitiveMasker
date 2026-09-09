import { useEffect, useId, useRef, useState } from "react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";

export interface TagFilterPopoverProps {
  availableTags: string[];
  selectedTags: string[];
  onSelectedTagsChange: (tags: string[]) => void;
  // selectedTagsの反映が非同期(IPC等)の呼び出し元向け。反映待ちの間に連続で
  // チェックを変更すると、後発の変更がselectedTagsプロパティの更新前の値を基準に
  // 計算されてしまい、先発の変更を無警告で上書きしてしまうため、その間は操作を止める。
  disabled?: boolean;
}

export function TagFilterPopover({
  availableTags,
  selectedTags,
  onSelectedTagsChange,
  disabled = false,
}: TagFilterPopoverProps) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  // selectedTagsプロパティへの反映が非同期の呼び出し元向け: 反映が間に合う前に
  // 連続でトグルされても、直前の自分の変更を基準に次の値を計算できるようにする
  // (プロパティの古い値を基準にすると、後発の変更が先発の変更を無警告で
  // 打ち消してしまう)。
  const [pendingTags, setPendingTags] = useState<string[] | null>(null);
  const effectiveTags = pendingTags ?? selectedTags;

  useEffect(() => {
    setPendingTags(null);
  }, [selectedTags]);

  const wasDisabledRef = useRef(disabled);
  useEffect(() => {
    // disabledがtrue→falseに変わるのは反映(成功/失敗いずれか)の完了合図。
    // 失敗時はselectedTagsが変化しないため上のeffectだけでは破棄されず、
    // 実際には反映されていないのに反映済みに見えてしまうため、ここでも破棄する。
    if (wasDisabledRef.current && !disabled) setPendingTags(null);
    wasDisabledRef.current = disabled;
  }, [disabled]);

  const filteredTags = availableTags.filter((tag) =>
    tag.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const toggleTag = (tag: string, checked: boolean) => {
    const next = checked
      ? [...effectiveTags, tag]
      : effectiveTags.filter((t) => t !== tag);
    setPendingTags(next);
    onSelectedTagsChange(next);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setSearchQuery("");
      }}
    >
      <PopoverTrigger asChild>
        <Button variant="outline">
          タグ ({effectiveTags.length}) {open ? "▲" : "▼"}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72">
        <Input
          placeholder="🔍 タグを検索..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />

        <div className="grid max-h-64 gap-1 overflow-y-auto">
          {filteredTags.map((tag) => {
            const checkboxId = `${id}-${tag}`;
            return (
              <div key={tag} className="flex items-center gap-2 px-1 py-1">
                <Checkbox
                  id={checkboxId}
                  checked={effectiveTags.includes(tag)}
                  disabled={disabled}
                  onCheckedChange={(checked) => toggleTag(tag, checked === true)}
                />
                <Label htmlFor={checkboxId} className="font-normal">
                  {tag}
                </Label>
              </div>
            );
          })}
        </div>

        <div className="flex items-center justify-between border-t pt-2.5">
          <Button
            size="sm"
            variant="outline"
            disabled={disabled}
            onClick={() => {
              setPendingTags([]);
              onSelectedTagsChange([]);
            }}
          >
            すべて解除
          </Button>
          <span className="text-sm text-muted-foreground">
            {effectiveTags.length}件選択中
          </span>
        </div>
      </PopoverContent>
    </Popover>
  );
}
