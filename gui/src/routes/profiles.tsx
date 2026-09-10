import { useRef, useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { ProfileManagementScreen, type SortOption } from "@/components/profile-management-screen";
import { ProfileNameDialog } from "@/components/profile-name-dialog";
import { TemplateSelectDialog, DEFAULT_TEMPLATES } from "@/components/template-select-dialog";
import { TagManagementDialog } from "@/components/tag-management-dialog";
import { ExportModal } from "@/components/export-modal";
import { ImportPassphraseDialog } from "@/components/import-passphrase-dialog";
import { ImportConfirmDialog, type ImportPreviewRow } from "@/components/import-confirm-dialog";
import { useAppState } from "@/lib/app-state";
import type { ImportPreviewDto } from "@/lib/profile-ipc";

export const Route = createFileRoute("/profiles")({
  component: ProfilesRoute,
});

const SORT_OPTIONS: SortOption[] = [
  { value: "updated_desc", label: "更新日時が新しい順" },
  { value: "updated_asc", label: "更新日時が古い順" },
  { value: "name_asc", label: "名前順" },
];

const EXPORT_FILE_FILTERS = [{ name: "SensitiveMasker Export", extensions: ["smx"] }];
// コピー後この時間が経過したら、クリップボードの中身がまだこのパスフレーズの
// ままであることを確認した上でクリアする(モックアップ6の要件)。
const CLIPBOARD_CLEAR_DELAY_MS = 30_000;

function sortProfiles<T extends { name: string; updatedAt: string }>(
  profiles: T[],
  sortValue: string
): T[] {
  const sorted = [...profiles];
  if (sortValue === "name_asc") sorted.sort((a, b) => a.name.localeCompare(b.name));
  else if (sortValue === "updated_asc")
    sorted.sort((a, b) => a.updatedAt.localeCompare(b.updatedAt));
  else sorted.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  return sorted;
}

function generatePassphrase(): string {
  return crypto.randomUUID().replace(/-/g, "").slice(0, 20);
}

function toImportPreviewRows(preview: ImportPreviewDto): ImportPreviewRow[] {
  if (preview.kind === "single") {
    return [{ profileName: preview.name, result: "新規プロファイルとして追加されます" }];
  }
  return preview.entries.map((entry) => ({
    profileName: entry.original_name,
    result: entry.renamed
      ? `名前が重複するため「${entry.resolved_name}」として追加されます`
      : "新規プロファイルとして追加されます",
  }));
}

type DialogState =
  | { kind: "none" }
  | { kind: "newProfile" }
  | { kind: "templateSelect" }
  | { kind: "profileNameFromTemplate"; templateValue: string }
  | { kind: "tagManagement" }
  | { kind: "export"; target: string; profileId: string | null }
  | { kind: "importPassphrase"; sourcePath: string; fileName: string }
  | { kind: "importConfirm"; rows: ImportPreviewRow[] };

function ProfilesRoute() {
  const navigate = useNavigate();
  const appState = useAppState();
  const { profiles, tags } = appState;

  const [searchQuery, setSearchQuery] = useState("");
  const [sortValue, setSortValue] = useState(SORT_OPTIONS[0].value);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [selectedTags, setSelectedTags] = useState<string[]>([]);
  const [savingTagsCountsForProfileIds, setSavingTagsCountsForProfileIds] = useState<
    Map<string, number>
  >(() => new Map());

  const [dialog, setDialog] = useState<DialogState>({ kind: "none" });
  const [draftName, setDraftName] = useState("新しいプロファイル");
  const [draftError, setDraftError] = useState<string | undefined>();
  const [templateValue, setTemplateValue] = useState(DEFAULT_TEMPLATES[0].value);
  const [tagSearchQuery, setTagSearchQuery] = useState("");
  const [newTagName, setNewTagName] = useState("");
  const [tagError, setTagError] = useState<string | undefined>();
  const [invalidTagId, setInvalidTagId] = useState<string | "new" | undefined>();
  const [passphrase, setPassphrase] = useState("");
  const [importPassphrase, setImportPassphrase] = useState("");
  const [importPassphraseError, setImportPassphraseError] = useState<string | undefined>();
  const clipboardClearTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const closeDialog = () => setDialog({ kind: "none" });

  const cancelClipboardClear = () => {
    if (clipboardClearTimer.current) {
      clearTimeout(clipboardClearTimer.current);
      clipboardClearTimer.current = null;
    }
  };

  const copyPassphraseWithAutoClear = (value: string) => {
    navigator.clipboard.writeText(value).catch(() => {});
    cancelClipboardClear();
    clipboardClearTimer.current = setTimeout(() => {
      // 書き込み後に他の内容が上書きされている場合は消さない(意図しないクリアを防ぐ)。
      navigator.clipboard
        .readText()
        .then((current) => {
          if (current === value) return navigator.clipboard.writeText("");
        })
        .catch(() => {});
    }, CLIPBOARD_CLEAR_DELAY_MS);
  };

  const confirmNewProfileName = async (templateForSeed?: string) => {
    if (profiles.some((p) => p.name === draftName)) {
      setDraftError("同じ名前のプロファイルが既に存在します");
      return;
    }
    const id = await appState.createProfile(draftName, templateForSeed);
    closeDialog();
    navigate({ to: "/rules/$profileId", params: { profileId: id } });
  };

  return (
    <>
      <ProfileManagementScreen
        profiles={sortProfiles(profiles, sortValue).map((p) => ({
          id: p.id,
          name: p.name,
          isActive: p.isActive,
          isFavorite: p.isFavorite,
          updatedAt: p.updatedAt,
          ruleCount: p.ruleCount,
          tags: p.tags,
        }))}
        searchQuery={searchQuery}
        onSearchQueryChange={setSearchQuery}
        sortOptions={SORT_OPTIONS}
        sortValue={sortValue}
        onSortValueChange={setSortValue}
        favoritesOnly={favoritesOnly}
        onFavoritesOnlyChange={setFavoritesOnly}
        availableTags={tags.map((t) => t.name)}
        selectedTags={selectedTags}
        onSelectedTagsChange={setSelectedTags}
        onClose={() => navigate({ to: "/" })}
        onNewProfile={() => {
          setDraftName("新しいプロファイル");
          setDraftError(undefined);
          setDialog({ kind: "newProfile" });
        }}
        onCreateFromTemplate={() => {
          setTemplateValue(DEFAULT_TEMPLATES[0].value);
          setDialog({ kind: "templateSelect" });
        }}
        onManageTags={() => {
          setTagSearchQuery("");
          setNewTagName("");
          setTagError(undefined);
          setInvalidTagId(undefined);
          setDialog({ kind: "tagManagement" });
        }}
        onExportAll={() => {
          setPassphrase(generatePassphrase());
          setDialog({ kind: "export", target: "全プロファイル", profileId: null });
        }}
        onImport={async () => {
          const path = await openFileDialog({ multiple: false, filters: EXPORT_FILE_FILTERS });
          if (!path || Array.isArray(path)) return;
          setImportPassphrase("");
          setImportPassphraseError(undefined);
          setDialog({
            kind: "importPassphrase",
            sourcePath: path,
            fileName: path.split(/[\\/]/).pop() ?? path,
          });
        }}
        onToggleFavorite={(id) => appState.toggleFavorite(id)}
        onRowClick={(id) => appState.setActiveProfileId(id)}
        onEditProfile={(id) => navigate({ to: "/rules/$profileId", params: { profileId: id } })}
        onDuplicateProfile={(id, newName) => appState.duplicateProfile(id, newName)}
        onExportProfile={(id) => {
          const target = profiles.find((p) => p.id === id)?.name ?? "";
          setPassphrase(generatePassphrase());
          setDialog({ kind: "export", target, profileId: id });
        }}
        onDeleteProfile={(id) => appState.deleteProfile(id)}
        onProfileTagsChange={(id, tags) => {
          setSavingTagsCountsForProfileIds((prev) => {
            const next = new Map(prev);
            next.set(id, (next.get(id) ?? 0) + 1);
            return next;
          });
          appState
            .setProfileTags(id, tags)
            .catch(() => {})
            .finally(() => {
              // このidの件数だけを1減らす(同じ行への別の変更がまだ反映待ちの
              // 場合は0にせず残す)。
              setSavingTagsCountsForProfileIds((prev) => {
                const next = new Map(prev);
                const count = (next.get(id) ?? 1) - 1;
                if (count <= 0) next.delete(id);
                else next.set(id, count);
                return next;
              });
            });
        }}
        savingTagsCountsForProfileIds={savingTagsCountsForProfileIds}
      />

      <ProfileNameDialog
        open={dialog.kind === "newProfile"}
        onOpenChange={(open) => !open && closeDialog()}
        name={draftName}
        onNameChange={(name) => {
          setDraftName(name);
          setDraftError(undefined);
        }}
        errorMessage={draftError}
        onConfirm={() => confirmNewProfileName()}
      />

      <TemplateSelectDialog
        open={dialog.kind === "templateSelect"}
        onOpenChange={(open) => !open && closeDialog()}
        templates={DEFAULT_TEMPLATES}
        value={templateValue}
        onValueChange={setTemplateValue}
        onConfirm={() => {
          const label = DEFAULT_TEMPLATES.find((t) => t.value === templateValue)?.label ?? "";
          setDraftName(label);
          setDraftError(undefined);
          setDialog({ kind: "profileNameFromTemplate", templateValue });
        }}
      />

      <ProfileNameDialog
        open={dialog.kind === "profileNameFromTemplate"}
        onOpenChange={(open) => !open && closeDialog()}
        name={draftName}
        onNameChange={(name) => {
          setDraftName(name);
          setDraftError(undefined);
        }}
        errorMessage={draftError}
        onConfirm={() =>
          confirmNewProfileName(
            dialog.kind === "profileNameFromTemplate" ? dialog.templateValue : undefined
          )
        }
      />

      <TagManagementDialog
        open={dialog.kind === "tagManagement"}
        onOpenChange={(open) => !open && closeDialog()}
        tags={tags}
        searchQuery={tagSearchQuery}
        onSearchQueryChange={setTagSearchQuery}
        newTagName={newTagName}
        onNewTagNameChange={(name) => {
          setNewTagName(name);
          setTagError(undefined);
          setInvalidTagId(undefined);
        }}
        onAddTag={() => {
          if (tags.some((t) => t.name === newTagName)) {
            setTagError("同じ名前のタグが既に存在します");
            setInvalidTagId("new");
            return;
          }
          appState.createTag(newTagName);
          setNewTagName("");
        }}
        onRenameTag={(id, newName) => {
          if (tags.some((t) => t.id !== id && t.name === newName)) {
            setTagError("同じ名前のタグが既に存在します");
            setInvalidTagId(id);
            return;
          }
          appState.renameTag(id, newName);
        }}
        onDeleteTag={(id) => appState.deleteTag(id)}
        errorMessage={tagError}
        invalidTagId={invalidTagId}
      />

      <ExportModal
        open={dialog.kind === "export"}
        onOpenChange={(open) => !open && closeDialog()}
        target={dialog.kind === "export" ? dialog.target : ""}
        passphrase={passphrase}
        onCopy={() => copyPassphraseWithAutoClear(passphrase)}
        onRegenerate={() => {
          // 保留中のクリアタイマーは新しいパスフレーズには無関係(値を比較して
          // クリアするため無くても安全だが、無駄なタイマーを積まないための整理)。
          cancelClipboardClear();
          setPassphrase(generatePassphrase());
        }}
        onExport={async () => {
          if (dialog.kind !== "export") return;
          const { profileId } = dialog;
          const defaultPath = `${profileId === null ? "sensitivemasker_all" : dialog.target}.smx`;
          const destPath = await saveFileDialog({ defaultPath, filters: EXPORT_FILE_FILTERS });
          if (!destPath) return;
          try {
            if (profileId === null) await appState.exportAll(passphrase, destPath);
            else await appState.exportProfile(profileId, passphrase, destPath);
            toast.success("エクスポートが完了しました");
            // クリップボードの自動クリアはダイアログを閉じても継続する(コピーした
            // パスフレーズを他所に控える目的で閉じた場合もクリアされるべきため)。
            closeDialog();
          } catch {
            // 失敗の通知はappState側のtoastが行う。ダイアログは開いたままにし、
            // 別の保存先で再試行できるようにする。
          }
        }}
      />

      <ImportPassphraseDialog
        open={dialog.kind === "importPassphrase"}
        onOpenChange={(open) => !open && closeDialog()}
        fileName={dialog.kind === "importPassphrase" ? dialog.fileName : ""}
        passphrase={importPassphrase}
        onPassphraseChange={(value) => {
          setImportPassphrase(value);
          setImportPassphraseError(undefined);
        }}
        errorMessage={importPassphraseError}
        onConfirm={async () => {
          if (dialog.kind !== "importPassphrase") return;
          try {
            const preview = await appState.previewImport(dialog.sourcePath, importPassphrase);
            setDialog({ kind: "importConfirm", rows: toImportPreviewRows(preview) });
          } catch {
            setImportPassphraseError("パスフレーズが誤っているか、対応していないファイル形式です");
          }
        }}
      />

      <ImportConfirmDialog
        open={dialog.kind === "importConfirm"}
        onOpenChange={(open) => !open && closeDialog()}
        rows={dialog.kind === "importConfirm" ? dialog.rows : []}
        onConfirm={async () => {
          try {
            await appState.commitImport();
          } finally {
            // 成否に関わらずここで確認は終わる(失敗時の通知はappState側のtoastが行う。
            // 確認済みのpreviewはcommit呼び出しの成否に関わらずサーバー側で消費済みのため、
            // このダイアログを開いたままにしても同じ内容で再試行はできない)。
            closeDialog();
          }
        }}
      />
    </>
  );
}
