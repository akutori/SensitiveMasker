import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { MainScreen } from "@/components/main-screen";
import { ProfileNameDialog } from "@/components/profile-name-dialog";
import { FileImportChoiceDialog } from "@/components/file-import-choice-dialog";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { MatchCountConfirmDialog, type MatchCountRow } from "@/components/match-count-confirm-dialog";
import { ImportPassphraseDialog } from "@/components/import-passphrase-dialog";
import { ImportConfirmDialog, type ImportPreviewRow } from "@/components/import-confirm-dialog";
import { useAppState, SMX_FILE_FILTERS, toImportPreviewRows } from "@/lib/app-state";
import { maskText } from "@/lib/masking-ipc";

export const Route = createFileRoute("/")({
  component: MainRoute,
});

const DEMO_LOAD_FILE_PATH = "C:\\Users\\example_user\\logs\\debug_console_output.log";
const DEMO_LOAD_FILE_CONTENT = "着信: 0000-000-000\nSIP URI: sip:bob@203.0.113.20";

type DialogState =
  | { kind: "none" }
  | { kind: "newProfile" }
  | { kind: "fileImportChoice" }
  | { kind: "overwriteConfirm" }
  | { kind: "matchCountConfirm"; rows: MatchCountRow[] }
  | { kind: "importPassphrase"; sourcePath: string; fileName: string }
  | { kind: "importConfirm"; rows: ImportPreviewRow[] };

function MainRoute() {
  const navigate = useNavigate();
  const appState = useAppState();
  const { profiles, activeProfileId } = appState;

  const [dialog, setDialog] = useState<DialogState>({ kind: "none" });
  const [draftName, setDraftName] = useState("");
  const [draftError, setDraftError] = useState<string | undefined>();
  const [passphrase, setPassphrase] = useState("");
  const [passphraseError, setPassphraseError] = useState<string | undefined>();

  const closeDialog = () => setDialog({ kind: "none" });

  const confirmNewProfileName = async () => {
    if (profiles.some((p) => p.name === draftName)) {
      setDraftError("同じ名前のプロファイルが既に存在します");
      return;
    }
    const id = await appState.createProfile(draftName);
    closeDialog();
    navigate({ to: "/rules/$profileId", params: { profileId: id } });
  };

  return (
    <>
      <MainScreen
        profiles={profiles.map((p) => ({ id: p.id, name: p.name }))}
        activeProfileId={activeProfileId}
        onActiveProfileIdChange={(id) => appState.setActiveProfileId(id)}
        onOpenProfileList={() => navigate({ to: "/profiles" })}
        onImport={async () => {
          const path = await openFileDialog({ multiple: false, filters: SMX_FILE_FILTERS });
          if (!path || Array.isArray(path)) return;
          setPassphrase("");
          setPassphraseError(undefined);
          setDialog({
            kind: "importPassphrase",
            sourcePath: path,
            fileName: path.split(/[\\/]/).pop() ?? path,
          });
        }}
        onReload={() => console.log("reload profile")}
        onNewProfile={() => {
          setDraftName("新しいプロファイル");
          setDraftError(undefined);
          setDialog({ kind: "newProfile" });
        }}
        onEditProfile={() => {
          if (activeProfileId) {
            navigate({ to: "/rules/$profileId", params: { profileId: activeProfileId } });
          }
        }}
        inputText={appState.inputText}
        onInputTextChange={appState.setInputText}
        onLoadFromFile={() => setDialog({ kind: "fileImportChoice" })}
        onMaskExecute={appState.runMask}
        onClear={appState.clearInput}
        outputText={appState.outputText}
        onSaveToFile={() => console.log("save output to file")}
        onCopyToClipboard={() => console.log("copy output to clipboard")}
        statusText={appState.statusText}
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

      <FileImportChoiceDialog
        open={dialog.kind === "fileImportChoice"}
        onOpenChange={(open) => !open && closeDialog()}
        filePath={DEMO_LOAD_FILE_PATH}
        onLoadIntoInput={() => {
          if (appState.inputText.trim().length > 0) {
            setDialog({ kind: "overwriteConfirm" });
          } else {
            appState.setInputText(DEMO_LOAD_FILE_CONTENT);
            closeDialog();
          }
        }}
        onMaskAndSaveAs={async () => {
          const activeProfile = profiles.find((p) => p.id === activeProfileId);
          if (!activeProfile) return;
          try {
            const { rules } = await appState.getProfileDetail(activeProfile.id);
            const { matchCounts } = await maskText(
              activeProfile.id,
              activeProfile.name,
              rules,
              DEMO_LOAD_FILE_CONTENT
            );
            const rows: MatchCountRow[] = rules
              .filter((r) => r.enabled)
              .map((rule) => ({
                ruleName: rule.name,
                matchCount: matchCounts.find((m) => m.ruleName === rule.name)?.count ?? 0,
              }));
            setDialog({ kind: "matchCountConfirm", rows });
          } catch (error) {
            // 失敗時は確認ダイアログを開かない(成功したように見せない)。
            console.error("mask_text failed", error);
            toast.error("マスク実行に失敗しました");
          }
        }}
      />

      <ConfirmDialog
        open={dialog.kind === "overwriteConfirm"}
        onOpenChange={(open) => !open && closeDialog()}
        title="上書き確認"
        description="入力テキスト欄に既に入力されている内容は上書きされます。よろしいですか?"
        onConfirm={() => {
          appState.setInputText(DEMO_LOAD_FILE_CONTENT);
          closeDialog();
        }}
      />

      <MatchCountConfirmDialog
        open={dialog.kind === "matchCountConfirm"}
        onOpenChange={(open) => !open && closeDialog()}
        rows={dialog.kind === "matchCountConfirm" ? dialog.rows : []}
        onConfirm={() => {
          console.log("save masked file as...");
          closeDialog();
        }}
      />

      <ImportPassphraseDialog
        open={dialog.kind === "importPassphrase"}
        onOpenChange={(open) => !open && closeDialog()}
        fileName={dialog.kind === "importPassphrase" ? dialog.fileName : ""}
        passphrase={passphrase}
        onPassphraseChange={(value) => {
          setPassphrase(value);
          setPassphraseError(undefined);
        }}
        errorMessage={passphraseError}
        onConfirm={async () => {
          if (dialog.kind !== "importPassphrase") return;
          const { sourcePath } = dialog;
          try {
            const preview = await appState.previewImport(sourcePath, passphrase);
            // await中にユーザーがダイアログを閉じた、または別のファイルで
            // インポートをやり直している場合は上書きしない。
            setDialog((current) =>
              current.kind === "importPassphrase" && current.sourcePath === sourcePath
                ? { kind: "importConfirm", rows: toImportPreviewRows(preview) }
                : current
            );
          } catch {
            setPassphraseError("パスフレーズが誤っているか、対応していないファイル形式です");
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
          } catch {
            // 失敗時のトースト表示はappState側のreportAndRethrowが行うため、
            // ここでの追加対応は不要(catchが無いとこのPromise自体がunhandledになる)。
          } finally {
            closeDialog();
          }
        }}
      />
    </>
  );
}
