import { useId, useState } from "react";
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
}

export function TagFilterPopover({
  availableTags,
  selectedTags,
  onSelectedTagsChange,
}: TagFilterPopoverProps) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const filteredTags = availableTags.filter((tag) =>
    tag.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const toggleTag = (tag: string, checked: boolean) => {
    onSelectedTagsChange(
      checked ? [...selectedTags, tag] : selectedTags.filter((t) => t !== tag)
    );
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
          タグ ({selectedTags.length}) {open ? "▲" : "▼"}
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
                  checked={selectedTags.includes(tag)}
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
            onClick={() => onSelectedTagsChange([])}
          >
            すべて解除
          </Button>
          <span className="text-sm text-muted-foreground">
            {selectedTags.length}件選択中
          </span>
        </div>
      </PopoverContent>
    </Popover>
  );
}
