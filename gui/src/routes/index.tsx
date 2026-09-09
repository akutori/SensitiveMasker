import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { MainScreen } from "@/components/main-screen";
import { ProfileNameDialog } from "@/components/profile-name-dialog";
import { FileImportChoiceDialog } from "@/components/file-import-choice-dialog";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { MatchCountConfirmDialog, type MatchCountRow } from "@/components/match-count-confirm-dialog";
import { ImportPassphraseDialog } from "@/components/import-passphrase-dialog";
import { ImportConfirmDialog, type ImportPreviewRow } from "@/components/import-confirm-dialog";
import { useAppState, resolveUniqueName } from "@/lib/app-state";
import { maskText } from "@/lib/masking-ipc";

export const Route = createFileRoute("/")({
  component: MainRoute,
});

const DEMO_IMPORT_FILE_NAME = "sip_profile_export.smexport";
const DEMO_IMPORT_PROFILE_NAME = "SIP監視用(インポート)";
const DEMO_LOAD_FILE_PATH = "C:\\Users\\example_user\\logs\\debug_console_output.log";
const DEMO_LOAD_FILE_CONTENT = "着信: 0000-000-000\nSIP URI: sip:bob@203.0.113.20";

type DialogState =
  | { kind: "none" }
  | { kind: "newProfile" }
  | { kind: "fileImportChoice" }
  | { kind: "overwriteConfirm" }
  | { kind: "matchCountConfirm"; rows: MatchCountRow[] }
  | { kind: "importPassphrase" }
  | { kind: "importConfirm"; rows: ImportPreviewRow[]; resolvedProfileName: string };

function MainRoute() {
  const navigate = useNavigate();
  const appState = useAppState();
  const { profiles, activeProfileId, rulesByProfileId } = appState;

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
        onImport={() => {
          setPassphrase("");
          setPassphraseError(undefined);
          setDialog({ kind: "importPassphrase" });
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
          const rules = rulesByProfileId[activeProfile.id] ?? [];
          try {
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
        fileName={DEMO_IMPORT_FILE_NAME}
        passphrase={passphrase}
        onPassphraseChange={(value) => {
          setPassphrase(value);
          setPassphraseError(undefined);
        }}
        errorMessage={passphraseError}
        onConfirm={() => {
          if (passphrase.trim().length === 0) {
            setPassphraseError("パスフレーズが正しくありません");
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
