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
import { useAppState, SMX_FILE_FILTERS, toImportPreviewRows } from "@/lib/app-state";
import { isExportImportError } from "@/lib/profile-ipc";
import { writeClipboardText, clearClipboardIfMatches } from "@/lib/clipboard-ipc";

export const Route = createFileRoute("/profiles")({
  component: ProfilesRoute,
});

const SORT_OPTIONS: SortOption[] = [
  { value: "updated_desc", label: "更新日時が新しい順" },
  { value: "updated_asc", label: "更新日時が古い順" },
  { value: "name_asc", label: "名前順" },
];

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
  // 直近でコピーに成功したパスフレーズ(自動クリア待ちの間だけ保持)。再生成時に
  // その場でクリアするため、タイマーの生存とは別に値そのものを覚えておく。
  const lastCopiedPassphrase = useRef<string | null>(null);
  // コピー処理の完了(Rustへの書き込み確認)を待つ間に再生成された場合、後から
  // 解決した古い呼び出しがタイマー・状態を上書きしないようにするための世代カウンタ。
  const copyGeneration = useRef(0);

  const closeDialog = () => setDialog({ kind: "none" });

  const cancelClipboardClear = () => {
    if (clipboardClearTimer.current) {
      clearTimeout(clipboardClearTimer.current);
      clipboardClearTimer.current = null;
    }
  };

  // 「確認できなかった」だけでは「まだ残っている」とは断定できない(他の内容に既に
  // 上書きされていた場合も読み取り自体は失敗しうるため)。断定形の警告にしない。
  const warnClipboardNotClearedAutomatically = () =>
    toast.warning("クリップボードの内容を確認できませんでした。パスフレーズが残っている場合は手動でクリアしてください");

  // 書き込み・確認・クリアは全てRust側のコマンドで行う。navigator.clipboard.readText()は
  // ウィンドウのフォーカスとclipboard-read権限を要求し、コピー後に他アプリへ切り替える
  // という最も一般的な操作フローで失敗するため使わない。
  //
  // copyGenerationは「この呼び出しが今なお最新の操作か」の判定に一本化して使う
  // (書き込み完了時の判定だけでなく、30秒後のクリア結果が返ってきた時点でも同じ
  // 判定に使う)。既に次のコピー/再生成が発生していれば、古い呼び出しの結果は
  // (成功・失敗を問わず)警告や状態更新の対象にしない。
  const copyPassphraseWithAutoClear = (value: string) => {
    cancelClipboardClear();
    const generation = ++copyGeneration.current;
    writeClipboardText(value)
      .then(() => {
        if (copyGeneration.current !== generation) return;
        lastCopiedPassphrase.current = value;
        toast.success("パスフレーズをコピーしました");
        clipboardClearTimer.current = setTimeout(() => {
          clipboardClearTimer.current = null;
          clearClipboardIfMatches(value)
            .then((result) => {
              if (copyGeneration.current !== generation) return;
              if (result.outcome === "skipped_unable_to_verify") {
                warnClipboardNotClearedAutomatically();
              }
              lastCopiedPassphrase.current = null;
            })
            .catch(() => {
              if (copyGeneration.current === generation) warnClipboardNotClearedAutomatically();
            });
        }, CLIPBOARD_CLEAR_DELAY_MS);
      })
      .catch(() => {
        if (copyGeneration.current !== generation) return;
        toast.error("クリップボードへのコピーに失敗しました");
      });
  };

  // タイマーの取り消しだけでは、既にコピー済みの値はクリップボードに残り続ける
  // ため、再生成時はその場でクリアを試みる。
  const clearCopiedPassphraseNow = () => {
    cancelClipboardClear();
    const generation = ++copyGeneration.current;
    const copied = lastCopiedPassphrase.current;
    if (!copied) return;
    lastCopiedPassphrase.current = null;
    clearClipboardIfMatches(copied)
      .then((result) => {
        if (copyGeneration.current !== generation) return;
        if (result.outcome === "skipped_unable_to_verify") warnClipboardNotClearedAutomatically();
      })
      .catch(() => {
        if (copyGeneration.current === generation) warnClipboardNotClearedAutomatically();
      });
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
          enabledRuleCount: p.enabledRuleCount,
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
          const path = await openFileDialog({ multiple: false, filters: SMX_FILE_FILTERS });
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
        onOpenChange={(open) => {
          if (open) return;
          closeDialog();
          // パスフレーズをReact state上に残さない(画面録画・共有のアーカイブや
          // メモリダンプからの事後的な読み取りを避けるため)。
          setPassphrase("");
        }}
        target={dialog.kind === "export" ? dialog.target : ""}
        passphrase={passphrase}
        onCopy={() => copyPassphraseWithAutoClear(passphrase)}
        onRegenerate={() => {
          // 旧パスフレーズが既にコピーされていた場合、タイマーの取り消しだけでは
          // クリップボードに残り続けるため、その場でクリアを試みる。
          clearCopiedPassphraseNow();
          setPassphrase(generatePassphrase());
        }}
        onExport={async () => {
          if (dialog.kind !== "export") return;
          const { profileId } = dialog;
          // プロファイル名を既定ファイル名に使うと、暗号文の外側(ファイル名・最近使った
          // ファイルの履歴)に平文メタデータとして残ってしまうため、汎用名にする。
          const defaultPath = `${profileId === null ? "sensitivemasker_all" : "sensitivemasker_export"}.smx`;
          const destPath = await saveFileDialog({ defaultPath, filters: SMX_FILE_FILTERS });
          if (!destPath) return;
          try {
            if (profileId === null) await appState.exportAll(passphrase, destPath);
            else await appState.exportProfile(profileId, passphrase, destPath);
            toast.success("エクスポートが完了しました");
            // クリップボードの自動クリアはダイアログを閉じても継続する(コピーした
            // パスフレーズを他所に控える目的で閉じた場合もクリアされるべきため)。
            closeDialog();
            setPassphrase("");
          } catch {
            // 失敗の通知はappState側のtoastが行う。ダイアログは開いたままにし、
            // 別の保存先で再試行できるようにする。
          }
        }}
      />

      <ImportPassphraseDialog
        open={dialog.kind === "importPassphrase"}
        onOpenChange={(open) => {
          if (open) return;
          closeDialog();
          setImportPassphrase("");
        }}
        fileName={dialog.kind === "importPassphrase" ? dialog.fileName : ""}
        passphrase={importPassphrase}
        onPassphraseChange={(value) => {
          setImportPassphrase(value);
          setImportPassphraseError(undefined);
        }}
        errorMessage={importPassphraseError}
        onConfirm={async () => {
          if (dialog.kind !== "importPassphrase") return;
          const { sourcePath } = dialog;
          try {
            const preview = await appState.previewImport(sourcePath, importPassphrase);
            // await中にユーザーがダイアログを閉じた、または別のファイルで
            // インポートをやり直している場合は上書きしない。
            setDialog((current) =>
              current.kind === "importPassphrase" && current.sourcePath === sourcePath
                ? { kind: "importConfirm", rows: toImportPreviewRows(preview) }
                : current
            );
            // 復号は完了済みでこの先パスフレーズ自体は不要になるため、state上に残さない。
            setImportPassphrase("");
          } catch (error) {
            // Rust側は復号失敗・フォーマット不一致・規模超過・名前重複等を原因ごとに
            // 別々の具体的なメッセージとして返すため、1つの汎用文言に一本化しない
            // (一本化すると、正しいパスフレーズでも「誤っている」という誤案内になり、
            // 正当なバックアップファイルを誤って破棄しかねない)。ExportImportError以外の
            // 想定外の例外(Tauri IPC自体の失敗等)の場合のみ汎用文言にフォールバックする。
            setImportPassphraseError(
              isExportImportError(error)
                ? error.message
                : "パスフレーズが誤っているか、対応していないファイル形式です"
            );
          }
        }}
      />

      <ImportConfirmDialog
        open={dialog.kind === "importConfirm"}
        onOpenChange={(open) => {
          if (open) return;
          closeDialog();
          // commit成功/失敗時はRust側で既に消費済みだが、キャンセル時はここで
          // 明示的に破棄しない限り復号済みの平文が残り続けるため。
          appState.clearPendingImport();
        }}
        rows={dialog.kind === "importConfirm" ? dialog.rows : []}
        onConfirm={async () => {
          try {
            await appState.commitImport();
          } catch {
            // 失敗時のトースト表示はappState側のreportAndRethrowが行うため、
            // ここでの追加対応は不要(catchが無いとこのPromise自体がunhandledになる)。
          } finally {
            // 成否に関わらずここで確認は終わる。確認済みのpreviewはcommit呼び出しの
            // 成否に関わらずサーバー側で消費済みのため、このダイアログを開いたままに
            // しても同じ内容で再試行はできない。
            closeDialog();
          }
        }}
      />
    </>
  );
}
