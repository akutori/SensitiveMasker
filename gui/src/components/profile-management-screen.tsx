import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { TagFilterPopover } from "./tag-filter-popover";
import { DeleteConfirmDialog } from "./delete-confirm-dialog";
import { ProfileNameDialog } from "./profile-name-dialog";
import { cn } from "cn";

export interface Profile {
  id: string;
  name: string;
  isActive: boolean;
  isFavorite: boolean;
  updatedAt: string;
  ruleCount: number;
  tags: string[];
}

export interface SortOption {
  value: string;
  label: string;
}

export interface ProfileManagementScreenProps {
  profiles: Profile[];
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
  sortOptions: SortOption[];
  sortValue: string;
  onSortValueChange: (value: string) => void;
  favoritesOnly: boolean;
  onFavoritesOnlyChange: (value: boolean) => void;
  availableTags: string[];
  selectedTags: string[];
  onSelectedTagsChange: (tags: string[]) => void;
  onClose: () => void;
  onNewProfile: () => void;
  onManageTags: () => void;
  onExportAll: () => void;
  onImport: () => void;
  onToggleFavorite: (id: string) => void;
  onRowClick: (id: string) => void;
  onEditProfile: (id: string) => void;
  onDuplicateProfile: (id: string, newName: string) => void;
  onExportProfile: (id: string) => void;
  onDeleteProfile: (id: string) => void;
}

export function ProfileManagementScreen({
  profiles,
  searchQuery,
  onSearchQueryChange,
  sortOptions,
  sortValue,
  onSortValueChange,
  favoritesOnly,
  onFavoritesOnlyChange,
  availableTags,
  selectedTags,
  onSelectedTagsChange,
  onClose,
  onNewProfile,
  onManageTags,
  onExportAll,
  onImport,
  onToggleFavorite,
  onRowClick,
  onEditProfile,
  onDuplicateProfile,
  onExportProfile,
  onDeleteProfile,
}: ProfileManagementScreenProps) {
  const [pendingDeleteProfile, setPendingDeleteProfile] = useState<Profile | null>(
    null
  );
  const [duplicatingProfile, setDuplicatingProfile] = useState<Profile | null>(null);
  const [duplicateName, setDuplicateName] = useState("");
  const [duplicateError, setDuplicateError] = useState<string | undefined>();

  const startDuplicate = (profile: Profile) => {
    setDuplicatingProfile(profile);
    setDuplicateName(`${profile.name} のコピー`);
    setDuplicateError(undefined);
  };

  const confirmDuplicate = () => {
    if (profiles.some((p) => p.name === duplicateName)) {
      setDuplicateError("同じ名前のプロファイルが既に存在します");
      return;
    }
    if (duplicatingProfile) onDuplicateProfile(duplicatingProfile.id, duplicateName);
    setDuplicatingProfile(null);
  };

  const visibleProfiles = profiles.filter((profile) => {
    if (favoritesOnly && !profile.isFavorite) return false;
    if (
      searchQuery &&
      !profile.name.toLowerCase().includes(searchQuery.toLowerCase())
    )
      return false;
    if (selectedTags.length > 0 && !selectedTags.every((t) => profile.tags.includes(t)))
      return false;
    return true;
  });

  return (
    <TooltipProvider>
      <div className="p-5">
        <div className="flex items-center justify-between">
          <h1 className="text-lg font-bold">プロファイル管理</h1>
          <Button variant="outline" onClick={onClose}>
            閉じる(メイン画面へ)
          </Button>
        </div>

        <div className="mt-5 flex flex-wrap items-center gap-2">
          <Button variant="outline" onClick={onNewProfile}>
            + 新規プロファイル
          </Button>
          <div className="flex-1" />
          <Button variant="outline" onClick={onManageTags}>
            タグを管理
          </Button>
          <Button variant="outline" onClick={onExportAll}>
            全体エクスポート
          </Button>
          <Button variant="outline" onClick={onImport}>
            インポート
          </Button>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Input
            placeholder="🔍 プロファイル名で検索..."
            value={searchQuery}
            onChange={(e) => onSearchQueryChange(e.target.value)}
            className="max-w-56"
          />
          <Select value={sortValue} onValueChange={onSortValueChange}>
            <SelectTrigger className="w-48" aria-label="並び替え">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sortOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            aria-pressed={favoritesOnly}
            className={cn(favoritesOnly && "bg-accent")}
            onClick={() => onFavoritesOnlyChange(!favoritesOnly)}
          >
            ★ お気に入りのみ
          </Button>
        </div>

        <div className="mt-3">
          <TagFilterPopover
            availableTags={availableTags}
            selectedTags={selectedTags}
            onSelectedTagsChange={onSelectedTagsChange}
          />
        </div>

        <div className="mt-4 grid gap-2 border-t pt-4">
          {profiles.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              プロファイルがまだありません。「+ 新規プロファイル」から作成してください
            </p>
          ) : visibleProfiles.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              該当するプロファイルが見つかりません
            </p>
          ) : (
            visibleProfiles.map((profile) => (
              <div
                key={profile.id}
                onClick={() => onRowClick(profile.id)}
                className={cn(
                  "flex cursor-pointer items-start gap-3 rounded-lg border p-3",
                  profile.isActive ? "border-2 border-foreground bg-muted" : "border-border"
                )}
              >
                <button
                  aria-label={profile.isFavorite ? "お気に入りから外す" : "お気に入りに追加"}
                  data-a11y-verified-contrast="non-text-glyph-contrast-unreliable"
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleFavorite(profile.id);
                  }}
                  className="text-lg leading-none"
                >
                  {profile.isFavorite ? "★" : "☆"}
                </button>

                <div className="flex-1">
                  <div className="flex items-center gap-2 text-sm font-bold">
                    {profile.name}
                    {profile.isActive && (
                      <span className="rounded-full bg-foreground px-2 py-0.5 text-xs font-normal text-background">
                        使用中
                      </span>
                    )}
                  </div>
                  <div className="mt-1 text-xs text-foreground">
                    更新: {profile.updatedAt} ・ ルール{profile.ruleCount}件
                    {profile.tags.length > 0 && ` ・ ${profile.tags.join(", ")}`}
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={(e) => {
                      e.stopPropagation();
                      onEditProfile(profile.id);
                    }}
                  >
                    編集
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={(e) => {
                      e.stopPropagation();
                      startDuplicate(profile);
                    }}
                  >
                    複製
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={(e) => {
                      e.stopPropagation();
                      onExportProfile(profile.id);
                    }}
                  >
                    エクスポート
                  </Button>
                  {profile.isActive ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span tabIndex={0}>
                          <Button size="sm" variant="outline" disabled>
                            削除
                          </Button>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent>
                        アクティブなプロファイルは削除できません
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={(e) => {
                        e.stopPropagation();
                        setPendingDeleteProfile(profile);
                      }}
                    >
                      削除
                    </Button>
                  )}
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      <DeleteConfirmDialog
        open={pendingDeleteProfile !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingDeleteProfile(null);
        }}
        targetType="プロファイル"
        targetName={pendingDeleteProfile?.name ?? ""}
        onConfirm={() => {
          if (pendingDeleteProfile) onDeleteProfile(pendingDeleteProfile.id);
          setPendingDeleteProfile(null);
        }}
      />

      <ProfileNameDialog
        open={duplicatingProfile !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setDuplicatingProfile(null);
        }}
        name={duplicateName}
        onNameChange={(name) => {
          setDuplicateName(name);
          setDuplicateError(undefined);
        }}
        errorMessage={duplicateError}
        onConfirm={confirmDuplicate}
      />
    </TooltipProvider>
  );
}
