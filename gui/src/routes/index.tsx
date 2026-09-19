import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";
import { MainScreen } from "@/components/main-screen";
import { ProfileNameDialog } from "@/components/profile-name-dialog";
import { FileImportChoiceDialog } from "@/components/file-import-choice-dialog";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { MatchCountConfirmDialog, type MatchCountRow } from "@/components/match-count-confirm-dialog";
import { ImportPassphraseDialog } from "@/components/import-passphrase-dialog";
import { ImportConfirmDialog, type ImportPreviewRow } from "@/components/import-confirm-dialog";
import { useAppState, SMX_FILE_FILTERS, toImportPreviewRows } from "@/lib/app-state";
import { createImportConfirmHandlers } from "@/lib/import-confirm-handlers";
import { createImportPassphraseHandlers } from "@/lib/import-passphrase-handlers";
import { isExportImportError } from "@/lib/profile-ipc";
import { openFileDialog, saveFileDialog } from "@/lib/file-dialog";
import { maskText } from "@/lib/masking-ipc";
import { readTextFile, writeTextFile } from "@/lib/text-file-ipc";

export const Route = createFileRoute("/")({
  component: MainRoute,
});

// 元のファイル名から既定の保存先ファイル名を作る(拡張子の手前に_maskedを挿む。
// 拡張子が無い場合は.txtを付与する)。
function deriveMaskedFileName(sourcePath: string): string {
  const baseName = sourcePath.split(/[\\/]/).pop() ?? "output.txt";
  const dotIndex = baseName.lastIndexOf(".");
  return dotIndex > 0
    ? `${baseName.slice(0, dotIndex)}_masked${baseName.slice(dotIndex)}`
    : `${baseName}_masked.txt`;
}

type DialogState =
  | { kind: "none" }
  | { kind: "newProfile" }
  | { kind: "fileImportChoice"; sourcePath: string; content: string }
  | { kind: "overwriteConfirm"; content: string }
  | { kind: "matchCountConfirm"; rows: MatchCountRow[]; maskedText: string; sourcePath: string }
  | { kind: "importPassphrase"; session: number; sourcePath: string; fileName: string }
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
  // 「直接マスクして別ファイルに保存」の非同期処理(getProfileDetail+maskText)が
  // 完了する前に、ユーザーが確認ダイアログを閉じる・別のファイルを読み込み直す等で
  // 先に進んでしまった場合、古い呼び出しの結果で新しい状態を上書きしないためのgeneration
  // カウンタ(profiles.tsxのcopyGenerationと同じ方針)。sourcePathの値比較だけでは、
  // 同じパスのファイルを読み込み直した場合(内容が変わっていても)に古い結果を
  // 誤って受理してしまうため、値ではなく「これが最新の呼び出しか」で判定する。
  const maskAndSaveGeneration = useRef(0);

  // パスフレーズ入力画面で、復号している間だけtrue(理由はimport-passphrase-handlers.ts)。
  const importDecrypting = useRef(false);
  // この画面が所有する保留(復号の結果として、Rust側に保留された内容)の識別子。確認画面の確定・破棄・離脱は、
  // この識別子の保留だけを対象にする(他の画面が始めた復号の保留には触れない)。無ければnull。
  const ownedPendingImportId = useRef<number | null>(null);
  const [importBusy, setImportBusy] = useState(false);
  // 非同期の完了時に、最新の画面の状態を読むための写し(クロージャは、操作した時点の古い状態を掴む)。
  const dialogRef = useRef(dialog);
  useLayoutEffect(() => {
    dialogRef.current = dialog;
  });
  // パスフレーズ入力画面を開くたびに増やす識別子。同じファイルを開き直しても、別の画面として区別する
  // (復号の結果を、OKを押した時の画面にだけ返すため。理由はimport-passphrase-handlers.ts)。
  const importSessionCounter = useRef(0);
  // この画面が破棄されていないか(破棄された後に届いた復号の結果は、見る人が居ない)。
  const mounted = useRef(true);

  const closeDialog = () => setDialog({ kind: "none" });

  const importConfirmHandlers = createImportConfirmHandlers(
    {
      isOpen: () => dialog.kind === "importConfirm",
      commit: (pendingId) => appState.commitImport(pendingId),
      discardPending: (pendingId) => {
        void appState.clearPendingImport(pendingId);
      },
      close: closeDialog,
      closeIfStillOpen: () =>
        setDialog((current) => (current.kind === "importConfirm" ? { kind: "none" } : current)),
    },
    ownedPendingImportId
  );

  const importPassphraseHandlers = createImportPassphraseHandlers(
    {
      target: () =>
        dialog.kind === "importPassphrase"
          ? { session: dialog.session, sourcePath: dialog.sourcePath, passphrase: passphrase }
          : null,
      preview: (sourcePath, passphrase) => appState.previewImport(sourcePath, passphrase),
      isStillOpen: (session) => {
        const shown = dialogRef.current;
        return mounted.current && shown.kind === "importPassphrase" && shown.session === session;
      },
      showConfirm: (result) => {
        setDialog({ kind: "importConfirm", rows: toImportPreviewRows(result.preview) });
        // 復号は完了済みでこの先パスフレーズ自体は不要になるため、state上に残さない。
        setPassphrase("");
      },
      showError: (error) => {
        // Rust側は復号失敗・フォーマット不一致・規模超過・名前重複等を原因ごとに
        // 別々の具体的なメッセージとして返すため、1つの汎用文言に一本化しない
        // (一本化すると、正しいパスフレーズでも「誤っている」という誤案内になり、
        // 正当なバックアップファイルを誤って破棄しかねない)。ExportImportError以外の
        // 想定外の例外(Tauri IPC自体の失敗等)の場合のみ汎用文言にフォールバックする。
        setPassphraseError(
          isExportImportError(error)
            ? error.message
            : "パスフレーズが誤っているか、対応していないファイル形式です"
        );
      },
      discardPending: (result) => appState.clearPendingImport(result.pendingId),
      onBusyChange: setImportBusy,
    },
    importDecrypting,
    ownedPendingImportId
  );

  // この画面を離れる(破棄される)と、復号済みの内容(Rust側の保留)を確認する人が居なくなるため、この画面が
  // 所有する保留を破棄する(破棄しない場合の理由はimport-confirm-handlers.ts)。復号している最中に離れた場合は、
  // 結果が届いた時に、isStillOpenがfalseになって、その結果の保留が破棄される。
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      importConfirmHandlers.onLeave();
    };
  }, []);

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
          // ファイルの選択を待つ間に別の画面が開かれていたら、置き換えない(その画面の内容や、確認待ちの
          // 復号済みの内容を、失うため)。
          if (dialogRef.current.kind !== "none") return;
          setPassphrase("");
          setPassphraseError(undefined);
          setDialog({
            kind: "importPassphrase",
            session: ++importSessionCounter.current,
            sourcePath: path,
            fileName: path.split(/[\\/]/).pop() ?? path,
          });
        }}
        onReload={async () => {
          try {
            await appState.reloadProfiles();
          } catch {
            // 失敗の通知はappState側のtoastが行う。
          }
        }}
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
        onLoadFromFile={async () => {
          const path = await openFileDialog({ multiple: false });
          if (!path || Array.isArray(path)) return;
          try {
            const { text, hadInvalidUtf8 } = await readTextFile(path);
            // ファイルの読み込み中に別の画面が開かれていたら、置き換えない(その画面の内容や、確認待ちの
            // 復号済みの内容を、失うため)。読み込んだ内容を使わないので、読み込んだことの通知も出さない。
            if (dialogRef.current.kind !== "none") return;
            if (hadInvalidUtf8) {
              toast.warning("ファイルの一部に不正なバイト列があったため、置き換えて読み込みました");
            }
            // 新しい読み込みは、進行中の(古い)「直接マスクして別ファイルに保存」を
            // 無効化する(同じパスの再読み込みでも内容が変わっている可能性があるため)。
            maskAndSaveGeneration.current += 1;
            setDialog({ kind: "fileImportChoice", sourcePath: path, content: text });
          } catch (error) {
            console.error("read_text_file failed", error);
            toast.error("ファイルを読み込めませんでした");
          }
        }}
        onMaskExecute={appState.runMask}
        onClear={appState.clearInput}
        outputText={appState.outputText}
        onSaveToFile={async () => {
          const destPath = await saveFileDialog({ defaultPath: "masked.txt" });
          if (!destPath) return;
          try {
            await appState.saveOutputToFile(destPath);
            toast.success("ファイルに保存しました");
          } catch {
            // 失敗の通知はappState側のtoastが行う。
          }
        }}
        onCopyToClipboard={async () => {
          try {
            await appState.copyOutputToClipboard();
            toast.success("クリップボードにコピーしました");
          } catch {
            // 失敗の通知はappState側のtoastが行う。
          }
        }}
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
        onOpenChange={(open) => {
          if (open) return;
          // 進行中の(古い)「直接マスクして別ファイルに保存」が、閉じた後に
          // 無言で確認ダイアログを開き直すことがないよう無効化する。
          maskAndSaveGeneration.current += 1;
          closeDialog();
        }}
        filePath={dialog.kind === "fileImportChoice" ? dialog.sourcePath : ""}
        onLoadIntoInput={() => {
          if (dialog.kind !== "fileImportChoice") return;
          const { content } = dialog;
          // このダイアログのもう一方のボタン(直接マスクして別ファイルに保存)が
          // 進行中の場合、そちらを無効化する(setDialogを介さずこの分岐へ抜けるため
          // onOpenChangeのガードを経由しない。これが無いと、保留中のマスク結果が
          // この後開く上書き確認/マッチ件数確認ダイアログを無言で置き換えてしまう)。
          maskAndSaveGeneration.current += 1;
          if (appState.inputText.trim().length > 0) {
            setDialog({ kind: "overwriteConfirm", content });
          } else {
            appState.setInputText(content);
            closeDialog();
          }
        }}
        onMaskAndSaveAs={async () => {
          if (dialog.kind !== "fileImportChoice") return;
          const { sourcePath, content } = dialog;
          const activeProfile = profiles.find((p) => p.id === activeProfileId);
          if (!activeProfile) return;
          // このスナップショット以降に新しい読み込み・キャンセル・別のマスク実行が
          // 起きていないかを、値の一致(sourcePath等)ではなく「これが最新の呼び出しか」
          // で判定する(profiles.tsxのcopyGenerationと同じ方針。同じパスを読み込み
          // 直した場合、値だけの比較では内容が変わっていても古い結果を通してしまう)。
          const generation = ++maskAndSaveGeneration.current;
          try {
            const { rules } = await appState.getProfileDetail(activeProfile.id);
            const { text: maskedText, matchCounts } = await maskText(
              activeProfile.id,
              activeProfile.name,
              rules,
              content
            );
            if (maskAndSaveGeneration.current !== generation) return;
            const rows: MatchCountRow[] = rules
              .filter((r) => r.enabled)
              .map((rule) => ({
                ruleName: rule.name,
                matchCount: matchCounts.find((m) => m.ruleName === rule.name)?.count ?? 0,
              }));
            setDialog({ kind: "matchCountConfirm", rows, maskedText, sourcePath });
          } catch (error) {
            if (maskAndSaveGeneration.current !== generation) return;
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
          if (dialog.kind !== "overwriteConfirm") return;
          appState.setInputText(dialog.content);
          closeDialog();
        }}
      />

      <MatchCountConfirmDialog
        open={dialog.kind === "matchCountConfirm"}
        onOpenChange={(open) => !open && closeDialog()}
        rows={dialog.kind === "matchCountConfirm" ? dialog.rows : []}
        onConfirm={async () => {
          if (dialog.kind !== "matchCountConfirm") return;
          const { maskedText, sourcePath } = dialog;
          const destPath = await saveFileDialog({ defaultPath: deriveMaskedFileName(sourcePath) });
          if (!destPath) return;
          try {
            await writeTextFile(destPath, maskedText);
            toast.success("マスク結果をファイルに保存しました");
            closeDialog();
          } catch (error) {
            // write_text_fileの失敗はRust側が既に安全な具体的文言を返す
            // (例:「アプリのデータフォルダには保存できません」)ため、固定文言に
            // 置き換えずそのまま表示する(app-state.tsxのreportFileErrorAndRethrowと同じ理由)。
            const message = typeof error === "string" ? error : "ファイルへの保存に失敗しました";
            console.error(message, error);
            toast.error(message);
          }
        }}
      />

      <ImportPassphraseDialog
        open={dialog.kind === "importPassphrase"}
        onOpenChange={(open) => {
          if (open) return;
          closeDialog();
          setPassphrase("");
        }}
        fileName={dialog.kind === "importPassphrase" ? dialog.fileName : ""}
        passphrase={passphrase}
        onPassphraseChange={(value) => {
          setPassphrase(value);
          setPassphraseError(undefined);
        }}
        errorMessage={passphraseError}
        onConfirm={importPassphraseHandlers.onConfirm}
        busy={importBusy}
      />

      <ImportConfirmDialog
        open={dialog.kind === "importConfirm"}
        onOpenChange={importConfirmHandlers.onOpenChange}
        rows={dialog.kind === "importConfirm" ? dialog.rows : []}
        onConfirm={importConfirmHandlers.onConfirm}
      />
    </>
  );
}
