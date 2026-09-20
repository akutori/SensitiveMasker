import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createFileRoute, useBlocker, useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";
import { ProfileManagementScreen, type SortOption } from "@/components/profile-management-screen";
import { ProfileNameDialog } from "@/components/profile-name-dialog";
import { TemplateSelectDialog, DEFAULT_TEMPLATES } from "@/components/template-select-dialog";
import { TagManagementDialog } from "@/components/tag-management-dialog";
import { ExportModal } from "@/components/export-modal";
import { ExportMethodDialog, type ExportMethod } from "@/components/export-method-dialog";
import { ExportKeyFileModal, type ExportKeyFilePhase } from "@/components/export-key-file-modal";
import { ImportPassphraseDialog } from "@/components/import-passphrase-dialog";
import { ImportKeyFileDialog } from "@/components/import-key-file-dialog";
import { ImportConfirmDialog, type ImportPreviewRow } from "@/components/import-confirm-dialog";
import { EnvImportSelectDialog } from "@/components/env-import-select-dialog";
import type { RuleListItem } from "@/components/rule-edit-screen";
import { useAppState, SMX_FILE_FILTERS, toImportPreviewRows } from "@/lib/app-state";
import { isExportImportError } from "@/lib/profile-ipc";
import { openFileDialog, saveFileDialog } from "@/lib/file-dialog";
import { actionOnHiddenToTray } from "@/lib/hidden-to-tray";
import { KEY_FILE_FILTERS, keyFileNameOf } from "@/lib/key-file";
import { useImportKeyFile } from "@/lib/use-import-key-file";
import { suppressContextMenu } from "@/lib/suppress-context-menu";
import { useOnHiddenToTray } from "@/lib/use-on-hidden-to-tray";
import {
  abortExport,
  beginExport,
  beginWriting,
  canRegenerate,
  canStartExport,
  completeExport,
  isPassphraseAtRisk,
  openExportDialog,
  regeneratePassphrase,
  type ExportDialogState,
} from "@/lib/export-dialog-state";
import { createOperationCounter } from "@/lib/operation-counter";
import { createPassphraseClipboard } from "@/lib/passphrase-clipboard";
import { createImportConfirmHandlers } from "@/lib/import-confirm-handlers";
import { createImportPassphraseHandlers } from "@/lib/import-passphrase-handlers";
import { writeClipboardText, clearClipboardIfMatches } from "@/lib/clipboard-ipc";
import { CLIPBOARD_CLEAR_DELAY_SECONDS } from "@/lib/clipboard-clear-delay";
import { readTextFile } from "@/lib/text-file-ipc";
import { previewEnvImport, type EnvCandidate } from "@/lib/env-import-ipc";

export const Route = createFileRoute("/profiles")({
  component: ProfilesRoute,
});

const SORT_OPTIONS: SortOption[] = [
  { value: "updated_desc", label: "更新日時が新しい順" },
  { value: "updated_asc", label: "更新日時が古い順" },
  { value: "name_asc", label: "名前順" },
];

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

const blockAlways = () => true;

function generatePassphrase(): string {
  return crypto.randomUUID().replace(/-/g, "").slice(0, 20);
}

// 選択されたenv変数を、そのままLiteral一致+固定置換のルールへ変換する。
// 値が実際のシークレット文字列そのもののため、正規表現ではなくリテラル一致で
// 確実に検出する(masking-coreのRuleモデルに新しい概念を追加せずに済む)。
function envCandidatesToRules(candidates: EnvCandidate[]): Omit<RuleListItem, "id">[] {
  return candidates.map((c) => ({
    name: c.key,
    patternType: "literal",
    pattern: c.value,
    mode: "fixed",
    fixedValue: `[MASKED_${c.key}]`,
    prefix: "",
    enabled: true,
    description: "",
  }));
}

type DialogState =
  | { kind: "none" }
  | { kind: "newProfile" }
  | { kind: "templateSelect" }
  | { kind: "profileNameFromTemplate"; templateValue: string }
  | { kind: "tagManagement" }
  | { kind: "exportMethod"; target: string; profileId: string | null }
  | { kind: "exportKeyFile"; target: string; profileId: string | null; session: number; phase: ExportKeyFilePhase }
  | { kind: "export"; target: string; profileId: string | null; session: ExportDialogState }
  | { kind: "importPassphrase"; session: number; sourcePath: string; fileName: string }
  | { kind: "importKeyFile"; session: number; sourcePath: string; fileName: string }
  | { kind: "importConfirm"; rows: ImportPreviewRow[]; passphraseTrimmed: boolean }
  | { kind: "envImportSelect"; candidates: EnvCandidate[] }
  | { kind: "envImportName"; selectedRules: Omit<RuleListItem, "id">[] };

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
  // エクスポート画面を開いた回の番号(開くたびに増やす。export-dialog-state.tsのsessionId)。
  const exportSessionCounter = useRef(0);
  // エクスポートの進行状況(編集中→実行中→成功後)の、同期的に読める正。React 19は離散イベントの
  // 更新を再描画するまで反映しないため、同じ瞬間に届いた複数の操作(再生成の直後の実行、実行の
  // 二重押下など)が古い描画の状態を見て、画面に出ているものと違うパスフレーズで書き出したり、
  // 二重に実行したりしてしまう。遷移はこちらへ先に適用し、画面の状態(dialog)へ写す。
  const exportSessionRef = useRef<ExportDialogState | null>(null);
  // ウィンドウがトレイへ格納されるたびに進める。書き出し済みのエクスポート画面が、パスフレーズの表示を伏せ字へ戻す合図。
  const [concealSignal, setConcealSignal] = useState(0);
  // エクスポートの方法の選択(鍵ファイル・パスフレーズ)。エクスポートを始めるたびに、鍵ファイルへ戻す。
  const [exportMethod, setExportMethod] = useState<ExportMethod>("key_file");
  // 鍵ファイル方式のエクスポート画面を開いた回の番号(開くたびに増やす)。閉じて開き直した後に届く、古い操作の
  // 結果と、区別するため。
  const exportKeyFileSessionCounter = useRef(0);
  // パスフレーズ入力画面で、復号している間だけtrue(理由はimport-passphrase-handlers.ts)。
  const importDecrypting = useRef(false);
  // この画面が所有する保留(復号の結果として、Rust側に保留された内容)の識別子。確認画面の確定・破棄・離脱は、
  // その保留だけを対象にする(他の画面が始めた復号の保留には触れない)。無ければnull。
  const ownedPendingImportId = useRef<number | null>(null);
  const [importBusy, setImportBusy] = useState(false);
  // 非同期の完了時に、最新の画面の状態を読むための写し(クロージャは、操作した時点の古い状態を掴む)。
  const dialogRef = useRef(dialog);
  useLayoutEffect(() => {
    dialogRef.current = dialog;
  });
  // パスフレーズ入力画面を開いた回の番号(開くたびに増やす)。同じファイルを開き直しても、別の画面として区別する
  // (復号の結果を、OKを押した時の画面にだけ返すため。理由はimport-passphrase-handlers.ts)。
  const importSessionCounter = useRef(0);
  // この画面が破棄されていないか(破棄された後に届いた復号の結果は、見る人が居ない)。
  const mounted = useRef(true);
  const [importPassphrase, setImportPassphrase] = useState("");
  const [importPassphraseError, setImportPassphraseError] = useState<string | undefined>();
  // コピー/クリアのIPC応答待ちの間は、コピー・再生成を受け付けない(理由はoperation-counter.ts)。
  // ボタンの無効化に使うstateは、件数の変化の写しである。
  const [clipboardBusy, setClipboardBusy] = useState(false);
  const [clipboardOperations] = useState(() => createOperationCounter(setClipboardBusy));
  // パスフレーズのコピーと、その自動クリアの制御(理由と仕様はpassphrase-clipboard.ts)。
  const [passphraseClipboard] = useState(() =>
    createPassphraseClipboard({
      write: writeClipboardText,
      clearIfMatches: clearClipboardIfMatches,
      track: clipboardOperations.track,
      notify: {
        copied: () =>
          toast.success(
            `パスフレーズをコピーしました(${CLIPBOARD_CLEAR_DELAY_SECONDS}秒後に自動クリアを試みます)`
          ),
        copyFailed: () => toast.error("クリップボードへのコピーに失敗しました"),
        notCleared: () =>
          toast.warning(
            "クリップボードの内容を確認できませんでした。パスフレーズが残っている場合は手動でクリアしてください"
          ),
      },
    })
  );

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
          ? { session: dialog.session, sourcePath: dialog.sourcePath, passphrase: importPassphrase }
          : null,
      preview: (sourcePath, passphrase) => appState.previewImport(sourcePath, passphrase),
      isStillOpen: (session) => {
        const shown = dialogRef.current;
        return mounted.current && shown.kind === "importPassphrase" && shown.session === session;
      },
      showConfirm: (result) => {
        setDialog({
          kind: "importConfirm",
          rows: toImportPreviewRows(result.preview),
          passphraseTrimmed: result.passphraseTrimmed,
        });
        // 復号は完了済みでこの先パスフレーズ自体は不要になるため、state上に残さない。
        setImportPassphrase("");
      },
      showError: (error) => {
        // Rust側は復号失敗・フォーマット不一致・規模超過・名前重複等を原因ごとに
        // 別々の具体的なメッセージとして返すため、1つの汎用文言に一本化しない
        // (一本化すると、正しいパスフレーズでも「誤っている」という誤案内になり、
        // 正当なバックアップファイルを誤って破棄しかねない)。ExportImportError以外の
        // 想定外の例外(Tauri IPC自体の失敗等)の場合のみ汎用文言にフォールバックする
        // (原因を辿れるよう、ログには残す)。
        if (!isExportImportError(error)) console.error("preview_import failed", error);
        setImportPassphraseError(
          isExportImportError(error)
            ? error.message
            : "パスフレーズが誤っているか、対応していないファイル形式です"
        );
      },
      discardPending: (pendingId) => appState.clearPendingImport(pendingId),
      onBusyChange: setImportBusy,
    },
    importDecrypting,
    ownedPendingImportId
  );

  // 鍵ファイル方式の取り込み。パスフレーズ入力と同じ流れ(復号している間の再入の防止・閉じられた画面の結果の破棄・
  // 保留の所有の記録)を使う。流れの中で、入力の値として渡すのは、鍵ファイルのパス(鍵の中身は、画面へ渡さない)。
  const importKeyFile = useImportKeyFile(dialog.kind === "importKeyFile");
  const importKeyFileHandlers = createImportPassphraseHandlers(
    {
      target: () =>
        dialog.kind === "importKeyFile" && importKeyFile.keyFilePath !== null
          ? { session: dialog.session, sourcePath: dialog.sourcePath, passphrase: importKeyFile.keyFilePath }
          : null,
      preview: (sourcePath, keyPath) => appState.previewImportWithKeyFile(sourcePath, keyPath),
      isStillOpen: (session) => {
        const shown = dialogRef.current;
        return mounted.current && shown.kind === "importKeyFile" && shown.session === session;
      },
      showConfirm: (result) => {
        setDialog({
          kind: "importConfirm",
          rows: toImportPreviewRows(result.preview),
          passphraseTrimmed: false,
        });
        importKeyFile.reset();
      },
      showError: (error) => {
        // Rust側は、鍵ファイルの形式・別の鍵ファイル・データの破損などを、原因ごとに、具体的な文言で返す。
        if (!isExportImportError(error)) console.error("preview_import_with_key_file failed", error);
        importKeyFile.setError(isExportImportError(error) ? error.message : "鍵ファイルで復号できませんでした");
      },
      discardPending: (pendingId) => appState.clearPendingImport(pendingId),
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
      // エクスポートの進行状況の正も手放す(保存先を選ぶ間に離れた場合に、その選択の結果で、書き込まないため)。
      exportSessionRef.current = null;
    };
  }, []);

  // ウィンドウがトレイへ格納されたとき(Rust側が知らせる)。格納したまま長時間置かれると、パスフレーズ・復号済みの内容・
  // .envの値を持つ画面が、その間、画面と状態に残り続けるため、画面ごとの扱い(actionOnHiddenToTray)に従って、閉じる・
  // 伏せ字へ戻す。エクスポートの局面は、同期的に読める正(exportSessionRef)から読む(再描画より前に進んでいることがある)。
  useOnHiddenToTray(() => {
    const shown = dialogRef.current;
    const action = actionOnHiddenToTray(shown.kind, exportSessionRef.current?.phase);
    if (action === "conceal") setConcealSignal((current) => current + 1);
    if (action !== "close") return;
    switch (shown.kind) {
      case "export":
        exportSessionRef.current = null;
        closeDialog();
        break;
      case "importPassphrase":
        closeDialog();
        setImportPassphrase("");
        break;
      case "importConfirm":
        importConfirmHandlers.onOpenChange(false);
        break;
      default:
        closeDialog();
    }
  });

  // 書き出し中・書き出し済みの間だけtrue(パスフレーズを失わせてはならない局面)。
  const passphraseAtRisk = dialog.kind === "export" && isPassphraseAtRisk(dialog.session.phase);

  // 書き出し中・書き出し済みの画面は、履歴の移動(マウスの戻るボタンなど)でこの画面ごと消えると、
  // パスフレーズを失うため、移動を止める(Escapeや背景の操作を受け付けないのと同じ理由)。
  useBlocker({
    shouldBlockFn: blockAlways,
    disabled: !passphraseAtRisk,
    enableBeforeUnload: false,
  });

  // 同じ理由で、右クリックのメニュー(メニューキー・Shift+F10を含む)も出さない。WebView2の既定のメニューは有効で、
  // ページを操作する項目を含みうるため。
  useEffect(() => {
    if (!passphraseAtRisk) return;
    return suppressContextMenu(document);
  }, [passphraseAtRisk]);

  // エクスポートを開く。パスフレーズはここで生成し、進行状況の正(exportSessionRef)にも置く。
  const openExportSession = (target: string, profileId: string | null) => {
    const session = openExportDialog(++exportSessionCounter.current, generatePassphrase());
    exportSessionRef.current = session;
    setDialog({ kind: "export", target, profileId, session });
  };

  // 画面に出ているエクスポートの、最新の状態。古い描画からの操作(再描画の前に届いたものや、
  // 閉じて開き直す前の画面のもの)は、今の画面のものではないため、nullにする。
  const currentExportSession = (): ExportDialogState | null => {
    const session = exportSessionRef.current;
    return dialog.kind === "export" && session?.sessionId === dialog.session.sessionId
      ? session
      : null;
  };

  // 進行状況の遷移を、正へ先に適用し、画面の状態へ写す。閉じて開き直した後に届いた古い実行の
  // 結果は、開いた回の番号が異なるため、新しい画面へ反映しない。
  const transitionExportSession = (update: (session: ExportDialogState) => ExportDialogState) => {
    const current = exportSessionRef.current;
    if (!current) return;
    const next = update(current);
    exportSessionRef.current = next;
    setDialog((shown) =>
      shown.kind === "export" && shown.session.sessionId === next.sessionId
        ? { ...shown, session: next }
        : shown
    );
  };

  const exportToFile = async () => {
    const session = currentExportSession();
    if (dialog.kind !== "export" || !session || !canStartExport(session)) return;
    const { profileId } = dialog;
    // 書き出したファイルを復号できるのは、実行を押した時点のパスフレーズだけである。
    const { sessionId, passphrase: exportedPassphrase } = session;
    transitionExportSession((current) => beginExport(current, sessionId, exportedPassphrase));
    try {
      // プロファイル名を既定ファイル名に使うと、暗号文の外側(ファイル名・最近使った
      // ファイルの履歴)に平文メタデータとして残ってしまうため、汎用名にする。
      const defaultPath = `${profileId === null ? "sensitivemasker_all" : "sensitivemasker_export"}.smx`;
      // 保存先の選択そのものが失敗した場合は、appState側のtoastを通らないため、ここで通知する。
      const destPath = await saveFileDialog({ defaultPath, filters: SMX_FILE_FILTERS }).catch(
        (error) => {
          console.error("save dialog failed", error);
          toast.error("保存先を選択できませんでした");
          return null;
        }
      );
      if (!destPath) {
        transitionExportSession((current) => abortExport(current, sessionId));
        return;
      }
      // 保存先を選ぶ間は、画面を閉じられる。閉じられていた(画面を閉じた・別の画面へ移った・閉じて開き直した)
      // 場合は、その画面のパスフレーズを、誰も見ないまま書き出してはならないため、書き込まない。
      transitionExportSession((current) => beginWriting(current, sessionId));
      const writingSession = exportSessionRef.current;
      if (writingSession?.sessionId !== sessionId || writingSession.phase !== "writing") return;
      if (profileId === null) await appState.exportAll(exportedPassphrase, destPath);
      else await appState.exportProfile(profileId, exportedPassphrase, destPath);
      toast.success("エクスポートが完了しました");
      transitionExportSession((current) => completeExport(current, sessionId));
    } catch {
      // エクスポート自体の失敗の通知は、appState側のtoastが行う。ダイアログは開いたままにし、
      // 別の保存先で再試行できるようにする。
      transitionExportSession((current) => abortExport(current, sessionId));
    }
  };

  // エクスポートを始める: まず、復号の方法(鍵ファイル・パスフレーズ)を選ぶ。鍵ファイルが、初期の選択。
  const openExportMethodChoice = (target: string, profileId: string | null) => {
    setExportMethod("key_file");
    setDialog({ kind: "exportMethod", target, profileId });
  };

  // 方法の選択から、次へ進む。パスフレーズなら、パスフレーズを生成して、確認・コピーできる画面へ。鍵ファイルなら、鍵ファイルを
  // 保存する画面へ。
  const continueExport = () => {
    const shown = dialogRef.current;
    if (shown.kind !== "exportMethod") return;
    if (exportMethod === "passphrase") {
      openExportSession(shown.target, shown.profileId);
      return;
    }
    setDialog({
      kind: "exportKeyFile",
      target: shown.target,
      profileId: shown.profileId,
      session: ++exportKeyFileSessionCounter.current,
      phase: "idle",
    });
  };

  // 鍵ファイル方式のエクスポート。鍵ファイルの保存先、エクスポートするファイルの保存先の順に選び、両方決まってから、Rustが、鍵を
  // 生成して、2つのファイルを書く(どちらかの選択を取り消すと、何も書かない。鍵は、画面へ渡さない)。保存先を選ぶ間は、
  // まだ何も書き出していないため、画面を閉じられる。閉じられていた(閉じた・別の画面へ移った)場合は、その選択の結果では、
  // 書き込まない。
  const exportWithKeyFile = async () => {
    const shown = dialogRef.current;
    if (shown.kind !== "exportKeyFile" || shown.phase !== "idle") return;
    const { session, profileId } = shown;
    const stillOpen = () => {
      const current = dialogRef.current;
      return mounted.current && current.kind === "exportKeyFile" && current.session === session;
    };
    const setPhase = (phase: ExportKeyFilePhase) =>
      setDialog((current) =>
        current.kind === "exportKeyFile" && current.session === session ? { ...current, phase } : current
      );
    // 保存先の選択そのものが失敗した場合は、appState側のtoastを通らないため、ここで通知する。
    const chooseDestination = (
      defaultPath: string,
      filters: typeof SMX_FILE_FILTERS,
      purpose: "data" | "key"
    ) =>
      saveFileDialog({ defaultPath, filters }, purpose).catch((error) => {
        console.error("save dialog failed", error);
        toast.error("保存先を選択できませんでした");
        return null;
      });

    try {
      // プロファイル名を既定ファイル名に使わない理由は、exportToFileと同じ。
      const baseName = profileId === null ? "sensitivemasker_all" : "sensitivemasker_export";
      setPhase("choosingKeyFile");
      const keyDestPath = await chooseDestination(`${baseName}.smxkey`, KEY_FILE_FILTERS, "key");
      if (!keyDestPath || !stillOpen()) {
        if (stillOpen()) setPhase("idle");
        return;
      }
      setPhase("choosingExportFile");
      const destPath = await chooseDestination(`${baseName}.smx`, SMX_FILE_FILTERS, "data");
      if (!destPath || !stillOpen()) {
        if (stillOpen()) setPhase("idle");
        return;
      }
      setPhase("writing");
      const result =
        profileId === null
          ? await appState.exportAllWithKeyFile(destPath, keyDestPath)
          : await appState.exportProfileWithKeyFile(profileId, destPath, keyDestPath);
      toast.success("エクスポートが完了しました。鍵ファイルは、エクスポートしたファイルとは別の場所に、保管してください");
      if (!result.keyFileRestricted) {
        toast.warning("鍵ファイルを、自分だけが読める権限にできませんでした。保存先を確認してください");
      }
      setDialog((current) =>
        current.kind === "exportKeyFile" && current.session === session ? { kind: "none" } : current
      );
    } catch {
      // 失敗の通知は、appState側のtoastが行う。画面は開いたままにし、別の保存先で、やり直せるようにする。
      if (stillOpen()) setPhase("idle");
    }
  };

  const confirmNewProfileName = async () => {
    if (profiles.some((p) => p.name === draftName)) {
      setDraftError("同じ名前のプロファイルが既に存在します");
      return;
    }
    const id =
      dialog.kind === "envImportName"
        ? await appState.createProfileFromRules(draftName, dialog.selectedRules)
        : await appState.createProfile(
            draftName,
            dialog.kind === "profileNameFromTemplate" ? dialog.templateValue : undefined
          );
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
        onExportAll={() => openExportMethodChoice("全プロファイル", null)}
        onImport={async () => {
          const path = await openFileDialog({ multiple: false, filters: SMX_FILE_FILTERS });
          if (!path || Array.isArray(path)) return;
          // ファイルの選択を待つ間に別の画面が開かれていたら、置き換えない(書き出し中・書き出し済みの
          // エクスポート画面ならパスフレーズを、確認待ちのインポート画面なら復号済みの内容を、失うため)。
          if (dialogRef.current.kind !== "none") return;
          // ファイルの先頭の、平文のヘッダーから、復号の方式(パスフレーズ・鍵ファイル)を判別する。失敗の通知は、
          // appState側のtoastが行う。
          const method = await appState.detectImportMethod(path).catch(() => null);
          if (method === null) return;
          // 判別を待つ間に別の画面が開かれていたら、同じ理由で、置き換えない。
          if (dialogRef.current.kind !== "none") return;
          const session = ++importSessionCounter.current;
          const fileName = path.split(/[\\/]/).pop() ?? path;
          if (method === "key_file") {
            importKeyFile.reset();
            setDialog({ kind: "importKeyFile", session, sourcePath: path, fileName });
            return;
          }
          setImportPassphrase("");
          setImportPassphraseError(undefined);
          setDialog({ kind: "importPassphrase", session, sourcePath: path, fileName });
        }}
        onEnvImport={async () => {
          // 拡張子フィルタは付けない(index.tsxの「ファイルから」と同じ理由: ".env"は
          // Path::extension()上「拡張子なし」扱いになり、拡張子フィルタと相性が悪い)。
          const path = await openFileDialog({ multiple: false });
          if (!path || Array.isArray(path)) return;
          try {
            const { text, hadInvalidUtf8 } = await readTextFile(path);
            const candidates = await previewEnvImport(text);
            // ファイルの読み込み中に別の画面が開かれていたら、置き換えない(書き出し中・書き出し済みの
            // エクスポート画面ならパスフレーズを、確認待ちのインポート画面なら復号済みの内容を、失うため)。
            // 読み込んだ内容を使わないので、読み込んだことの通知も出さない。
            if (dialogRef.current.kind !== "none") return;
            if (hadInvalidUtf8) {
              toast.warning("ファイルの一部に不正なバイト列があったため、置き換えて読み込みました");
            }
            setDialog({ kind: "envImportSelect", candidates });
          } catch (error) {
            console.error("preview_env_import failed", error);
            toast.error("ファイルを読み込めませんでした");
          }
        }}
        onToggleFavorite={(id) => appState.toggleFavorite(id)}
        onRowClick={(id) => appState.setActiveProfileId(id)}
        onEditProfile={(id) => navigate({ to: "/rules/$profileId", params: { profileId: id } })}
        onDuplicateProfile={(id, newName) => appState.duplicateProfile(id, newName)}
        onExportProfile={(id) => {
          openExportMethodChoice(profiles.find((p) => p.id === id)?.name ?? "", id);
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
        onConfirm={() => confirmNewProfileName()}
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

      <ExportMethodDialog
        open={dialog.kind === "exportMethod"}
        onOpenChange={(open) => !open && closeDialog()}
        target={dialog.kind === "exportMethod" ? dialog.target : ""}
        method={exportMethod}
        onMethodChange={setExportMethod}
        onNext={continueExport}
      />

      <ExportKeyFileModal
        open={dialog.kind === "exportKeyFile"}
        onOpenChange={(open) => !open && closeDialog()}
        target={dialog.kind === "exportKeyFile" ? dialog.target : ""}
        phase={dialog.kind === "exportKeyFile" ? dialog.phase : "idle"}
        onExport={exportWithKeyFile}
      />

      <ExportModal
        open={dialog.kind === "export"}
        // 閉じるとパスフレーズは、画面の状態と、進行状況の正(exportSessionRef)の両方から手放す
        // (以後、この画面から参照できなくなり、閉じた後に届いた古い描画からの操作も、書き出さない。
        // JSの文字列はメモリ上で消去できないため、消えるのは参照だけである)。クリップボードの
        // 自動クリアは画面を閉じても継続する(コピーしたパスフレーズを他所に控える目的で
        // 閉じた場合も、クリアされるべきため)。実行中・成功後に閉じる操作は、ExportModalが
        // 受け付けない。
        onOpenChange={(open) => {
          if (open) return;
          exportSessionRef.current = null;
          closeDialog();
        }}
        target={dialog.kind === "export" ? dialog.target : ""}
        passphrase={dialog.kind === "export" ? dialog.session.passphrase : ""}
        status={dialog.kind === "export" ? dialog.session.phase : "editing"}
        clipboardBusy={clipboardBusy}
        onCopy={() => {
          // ボタンの無効化が再描画で反映されるより前に届いたクリックも防ぐため、同期的に判定する。
          if (clipboardOperations.isBusy()) return;
          // 閉じる途中(フェードアウト中)に届いたクリックで、空文字をコピーしないため。
          const session = currentExportSession();
          if (!session) return;
          passphraseClipboard.copy(session.passphrase);
        }}
        onRegenerate={() => {
          if (clipboardOperations.isBusy()) return;
          const session = currentExportSession();
          if (!session || !canRegenerate(session)) return;
          // 旧パスフレーズが既にコピーされていた場合、タイマーの取り消しだけでは
          // クリップボードに残り続けるため、その場でクリアを試みる。
          passphraseClipboard.clearNow();
          transitionExportSession((current) => regeneratePassphrase(current, generatePassphrase()));
        }}
        onExport={exportToFile}
        concealSignal={concealSignal}
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
        onConfirm={importPassphraseHandlers.onConfirm}
        busy={importBusy}
      />

      <ImportKeyFileDialog
        open={dialog.kind === "importKeyFile"}
        onOpenChange={(open) => {
          if (open) return;
          closeDialog();
          importKeyFile.reset();
        }}
        fileName={dialog.kind === "importKeyFile" ? dialog.fileName : ""}
        keyFileName={importKeyFile.keyFilePath === null ? null : keyFileNameOf(importKeyFile.keyFilePath)}
        dragActive={importKeyFile.dragActive}
        onSelectKeyFile={importKeyFile.selectKeyFile}
        errorMessage={importKeyFile.error}
        onConfirm={importKeyFileHandlers.onConfirm}
        busy={importBusy}
      />

      <ImportConfirmDialog
        open={dialog.kind === "importConfirm"}
        onOpenChange={importConfirmHandlers.onOpenChange}
        rows={dialog.kind === "importConfirm" ? dialog.rows : []}
        passphraseTrimmed={dialog.kind === "importConfirm" ? dialog.passphraseTrimmed : false}
        onConfirm={importConfirmHandlers.onConfirm}
      />

      <EnvImportSelectDialog
        open={dialog.kind === "envImportSelect"}
        onOpenChange={(open) => !open && closeDialog()}
        candidates={dialog.kind === "envImportSelect" ? dialog.candidates : []}
        onConfirm={(selected) => {
          setDraftName(".envから作成");
          setDraftError(undefined);
          setDialog({ kind: "envImportName", selectedRules: envCandidatesToRules(selected) });
        }}
      />

      <ProfileNameDialog
        open={dialog.kind === "envImportName"}
        onOpenChange={(open) => !open && closeDialog()}
        name={draftName}
        onNameChange={(name) => {
          setDraftName(name);
          setDraftError(undefined);
        }}
        errorMessage={draftError}
        onConfirm={() => confirmNewProfileName()}
      />
    </>
  );
}
