import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ProfileManagementScreen, type SortOption } from "@/components/profile-management-screen";
import { ProfileNameDialog } from "@/components/profile-name-dialog";
import { TemplateSelectDialog, DEFAULT_TEMPLATES } from "@/components/template-select-dialog";
import { TagManagementDialog } from "@/components/tag-management-dialog";
import { ExportModal } from "@/components/export-modal";
import { ImportPassphraseDialog } from "@/components/import-passphrase-dialog";
import { ImportConfirmDialog, type ImportPreviewRow } from "@/components/import-confirm-dialog";
import { useAppState, resolveUniqueName } from "@/lib/app-state";

export const Route = createFileRoute("/profiles")({
  component: ProfilesRoute,
});

const SORT_OPTIONS: SortOption[] = [
  { value: "updated_desc", label: "更新日時が新しい順" },
  { value: "updated_asc", label: "更新日時が古い順" },
  { value: "name_asc", label: "名前順" },
];

const DEMO_IMPORT_FILE_NAME = "sip_profile_export.smexport";
const DEMO_IMPORT_PROFILE_NAME = "SIP監視用(インポート)";

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

function generateDemoPassphrase(): string {
  return crypto.randomUUID().replace(/-/g, "").slice(0, 20);
}

type DialogState =
  | { kind: "none" }
  | { kind: "newProfile" }
  | { kind: "templateSelect" }
  | { kind: "profileNameFromTemplate"; templateValue: string }
  | { kind: "tagManagement" }
  | { kind: "export"; target: string }
  | { kind: "importPassphrase" }
  | { kind: "importConfirm"; rows: ImportPreviewRow[]; resolvedProfileName: string };

function ProfilesRoute() {
  const navigate = useNavigate();
  const appState = useAppState();
  const { profiles, tags, rulesByProfileId } = appState;

  const [searchQuery, setSearchQuery] = useState("");
  const [sortValue, setSortValue] = useState(SORT_OPTIONS[0].value);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [selectedTags, setSelectedTags] = useState<string[]>([]);

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

  const closeDialog = () => setDialog({ kind: "none" });

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
          ruleCount: (rulesByProfileId[p.id] ?? []).length,
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
          setPassphrase(generateDemoPassphrase());
          setDialog({ kind: "export", target: "全プロファイル" });
        }}
        onImport={() => {
          setImportPassphrase("");
          setImportPassphraseError(undefined);
          setDialog({ kind: "importPassphrase" });
        }}
        onToggleFavorite={(id) => appState.toggleFavorite(id)}
        onRowClick={(id) => appState.setActiveProfileId(id)}
        onEditProfile={(id) => navigate({ to: "/rules/$profileId", params: { profileId: id } })}
        onDuplicateProfile={(id, newName) => appState.duplicateProfile(id, newName)}
        onExportProfile={(id) => {
          const target = profiles.find((p) => p.id === id)?.name ?? "";
          setPassphrase(generateDemoPassphrase());
          setDialog({ kind: "export", target });
        }}
        onDeleteProfile={(id) => appState.deleteProfile(id)}
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
        onCopy={() => {
          navigator.clipboard.writeText(passphrase).catch(() => {});
        }}
        onRegenerate={() => setPassphrase(generateDemoPassphrase())}
        onExport={() => {
          console.log("export with passphrase", passphrase);
          closeDialog();
        }}
      />

      <ImportPassphraseDialog
        open={dialog.kind === "importPassphrase"}
        onOpenChange={(open) => !open && closeDialog()}
        fileName={DEMO_IMPORT_FILE_NAME}
        passphrase={importPassphrase}
        onPassphraseChange={(value) => {
          setImportPassphrase(value);
          setImportPassphraseError(undefined);
        }}
        errorMessage={importPassphraseError}
        onConfirm={() => {
          if (importPassphrase.trim().length === 0) {
            setImportPassphraseError("パスフレーズが正しくありません");
            return;
          }
          const resolvedProfileName = resolveUniqueName(
            DEMO_IMPORT_PROFILE_NAME,
            profiles.map((p) => p.name)
          );
          setDialog({
            kind: "importConfirm",
            resolvedProfileName,
            rows: [
              {
                profileName: DEMO_IMPORT_PROFILE_NAME,
                result:
                  resolvedProfileName === DEMO_IMPORT_PROFILE_NAME
                    ? "新規プロファイルとして追加されます"
                    : `名前が重複するため「${resolvedProfileName}」として追加されます`,
              },
            ],
          });
        }}
      />

      <ImportConfirmDialog
        open={dialog.kind === "importConfirm"}
        onOpenChange={(open) => !open && closeDialog()}
        rows={dialog.kind === "importConfirm" ? dialog.rows : []}
        onConfirm={async () => {
          if (dialog.kind === "importConfirm") {
            await appState.createProfile(dialog.resolvedProfileName);
          }
          closeDialog();
        }}
      />
    </>
  );
}
