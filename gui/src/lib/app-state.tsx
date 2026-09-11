import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import type { RuleListItem } from "@/components/rule-edit-screen";
import type { ImportPreviewRow } from "@/components/import-confirm-dialog";
import { DEMO_SAMPLE_TEXT, PROFILE_TEMPLATE_RULES } from "./demo-seed-data";
import { maskText, clearMappings as ipcClearMappings } from "./masking-ipc";
import {
  clearPendingImport as ipcClearPendingImport,
  commitPendingImport as ipcCommitPendingImport,
  createProfile as ipcCreateProfile,
  createTag as ipcCreateTag,
  deleteProfile as ipcDeleteProfile,
  deleteTag as ipcDeleteTag,
  exportAllToFile as ipcExportAllToFile,
  exportProfileToFile as ipcExportProfileToFile,
  getProfile as ipcGetProfile,
  initializeStore,
  isStoreInitialized,
  listProfiles,
  listTags,
  onActiveProfileRulesWeakened,
  onProfilesChanged,
  onTagsChanged,
  openStore,
  previewImport as ipcPreviewImport,
  renameTag as ipcRenameTag,
  setActiveProfile as ipcSetActiveProfile,
  setFavorite as ipcSetFavorite,
  setProfileTags as ipcSetProfileTags,
  updateProfile as ipcUpdateProfile,
  isExportImportError,
  type ImportPreviewDto,
  type ProfileDetail,
} from "./profile-ipc";

export interface Profile {
  id: string;
  name: string;
  isActive: boolean;
  isFavorite: boolean;
  updatedAt: string;
  ruleCount: number;
  enabledRuleCount: number;
  tags: string[];
}

export interface Tag {
  id: string;
  name: string;
}

// インポート対象ファイルのネイティブダイアログ用フィルタ。エクスポート保存
// ダイアログとも共用する(拡張子は常に.smx)。
export const SMX_FILE_FILTERS = [{ name: "SensitiveMasker Export", extensions: ["smx"] }];

// タグの無警告追加(監査指摘)・アクティブ化の無警告発生(同)の両方に確定前に
// 気付けるよう、件数・有無をresult文言に含める。
function tagsSuffix(tags: string[]): string {
  return tags.length > 0 ? `(タグ: ${tags.join("、")})` : "";
}

export function toImportPreviewRows(preview: ImportPreviewDto): ImportPreviewRow[] {
  if (preview.kind === "single") {
    return [
      {
        profileName: preview.name,
        result: `新規プロファイルとして追加されます${tagsSuffix(preview.tags)}`,
        rules: preview.rules,
      },
    ];
  }
  return preview.entries.map((entry) => {
    const base = entry.renamed
      ? `名前が重複するため「${entry.resolved_name}」として追加されます`
      : "新規プロファイルとして追加されます";
    const activated =
      preview.will_activate_profile_name === entry.resolved_name ? "、アクティブになります" : "";
    return {
      profileName: entry.original_name,
      result: `${base}${activated}${tagsSuffix(entry.tags)}`,
      rules: entry.rules,
    };
  });
}

export interface AppStateValue {
  initialized: boolean | null;
  start: () => Promise<void>;

  profiles: Profile[];
  activeProfileId: string | null;
  setActiveProfileId: (id: string) => Promise<void>;
  createProfile: (name: string, templateValue?: string) => Promise<string>;
  duplicateProfile: (id: string, newName: string) => Promise<string>;
  deleteProfile: (id: string) => Promise<void>;
  toggleFavorite: (id: string) => Promise<void>;
  getProfileDetail: (id: string) => Promise<ProfileDetail>;
  updateProfile: (
    id: string,
    // tagsを省略した場合はタグ自体の更新を送らない(呼び出し元の画面でタグ欄が
    // 未操作の場合に、読み込み時点のスナップショットで他画面での並行した
    // タグ変更を無警告に上書きしてしまうのを防ぐため)。
    meta: { name: string; description: string; rules: RuleListItem[]; tags?: string[] }
  ) => Promise<void>;
  setProfileTags: (id: string, tags: string[]) => Promise<void>;
  exportProfile: (id: string, passphrase: string, destPath: string) => Promise<void>;
  exportAll: (passphrase: string, destPath: string) => Promise<void>;
  previewImport: (sourcePath: string, passphrase: string) => Promise<ImportPreviewDto>;
  commitImport: () => Promise<void>;
  clearPendingImport: () => Promise<void>;

  tags: Tag[];
  createTag: (name: string) => Promise<void>;
  renameTag: (id: string, newName: string) => Promise<void>;
  deleteTag: (id: string) => Promise<void>;

  inputText: string;
  setInputText: (text: string) => void;
  outputText: string;
  statusText: string;
  runMask: () => void;
  clearInput: () => void;
}

const AppStateContext = createContext<AppStateValue | null>(null);

function withRuleIds(rules: Omit<RuleListItem, "id">[]): RuleListItem[] {
  return rules.map((rule) => ({ ...rule, id: crypto.randomUUID() }));
}

// 失敗を握り潰さず必ずユーザーに見える形にするための共通ラッパー。呼び出し元が
// 「失敗時は何もしない(画面遷移しない等)」を判断できるよう、表示後にrethrowする。
async function reportAndRethrow<T>(message: string, action: () => Promise<T>): Promise<T> {
  try {
    return await action();
  } catch (error) {
    console.error(message, error);
    toast.error(message);
    throw error;
  }
}

// reportAndRethrowのエクスポート専用版。保存先パス・拡張子・サイズ等の事前検証で
// 弾かれた場合(kind: "invalid_input")は、原因を隠す固定文言ではなく実際の理由を
// 表示する(例:「アプリのデータフォルダには保存できません」がここで初めてユーザーに
// 届く。これが無いとパスフレーズとは無関係な失敗が「エクスポートに失敗しました」と
// しか表示されず原因を特定できない)。
async function reportExportErrorAndRethrow<T>(action: () => Promise<T>): Promise<T> {
  try {
    return await action();
  } catch (error) {
    const message =
      isExportImportError(error) && error.kind === "invalid_input" ? error.message : "エクスポートに失敗しました";
    console.error(message, error);
    toast.error(message);
    throw error;
  }
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [initialized, setInitializedState] = useState<boolean | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [inputText, setInputText] = useState(DEMO_SAMPLE_TEXT);
  const [outputText, setOutputText] = useState("");
  const [statusText, setStatusText] = useState("アクティブプロファイル: なし");

  // 同一プロファイルへのタグ更新が並行して呼ばれた場合、IPC応答の順序保証が
  // 無いため後発が先に完了して先発に上書きされうる。idごとに前回の完了を
  // 待ってから次を実行することで、発行順=反映順を保証する。
  const tagUpdateQueues = useRef(new Map<string, Promise<void>>());

  // キュー待ちで実行が遅延した時点の最新profilesを参照するためのref。
  // (通常のクロージャはsetProfileTags呼び出し時点のprofilesを掴んだままになり、
  // 待機中に別画面でのリネームが完了していても検知できないため。)
  const profilesRef = useRef(profiles);
  useEffect(() => {
    profilesRef.current = profiles;
  }, [profiles]);

  const refreshProfiles = async () => {
    const summaries = await listProfiles();
    setProfiles(
      summaries.map((s) => ({
        id: String(s.id),
        name: s.name,
        isActive: s.is_active,
        isFavorite: s.is_favorite,
        updatedAt: s.updated_at,
        ruleCount: s.rule_count,
        enabledRuleCount: s.enabled_rule_count,
        tags: s.tags,
      }))
    );
  };

  const refreshTags = async () => {
    const names = await listTags();
    setTags(names.map((name) => ({ id: name, name })));
  };

  // 起動時に一度だけ、既に初期化済み(鍵/DBが既存)かを確認する。初回セットアップ画面は
  // 「未初期化と確認できた場合」だけ表示し、2回目以降の起動では出さない。
  // 既に初期化済みの場合でも、ProfileStoreState自体はプロセス起動ごとに空になるため、
  // openStoreで明示的に実体化してからでないとlist_profiles等が失敗する。
  useEffect(() => {
    isStoreInitialized()
      .then(async (yes) => {
        if (yes) {
          await openStore();
          await Promise.all([refreshProfiles(), refreshTags()]);
        }
        setInitializedState(yes);
      })
      .catch((error) => {
        console.error("is_store_initialized failed", error);
        toast.error("初期化状態の確認に失敗しました");
        setInitializedState(false);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 別ウィンドウ(将来分含む)での変更をこのウィンドウにも反映するためのイベント購読。
  // フォーカスの有無に関係なく届く(PoCで検証済みの方式)。
  useEffect(() => {
    if (!initialized) return;
    const unlistenProfiles = onProfilesChanged(() => {
      refreshProfiles();
    });
    const unlistenTags = onTagsChanged(() => {
      refreshTags();
    });
    // アクティブプロファイルの既存の有効ルールが無言で無力化された場合の簡易な手がかり。
    // 正規の編集操作でも表示される(改ざん耐性のある記録ではなく、その場で見える
    // 通知であることが目的のため)。
    const unlistenRulesWeakened = onActiveProfileRulesWeakened(({ profileName, weakenedRuleNames }) => {
      // 一括インポート等で多数のルールが一度に無力化された場合でもトーストが
      // 読めないほど長くならないよう、表示件数に上限を設ける(最大500件想定)。
      const MAX_NAMES_IN_TOAST = 5;
      const shown = weakenedRuleNames.slice(0, MAX_NAMES_IN_TOAST).join("、");
      const rest = weakenedRuleNames.length - MAX_NAMES_IN_TOAST;
      const label = rest > 0 ? `${shown} 他${rest}件` : shown;
      toast.warning(`プロファイル「${profileName}」のルールが変更されました(${label})`);
    });
    return () => {
      unlistenProfiles.then((f) => f());
      unlistenTags.then((f) => f());
      unlistenRulesWeakened.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialized]);

  const activeProfileId = profiles.find((p) => p.isActive)?.id ?? null;
  const findNameById = (id: string): string | undefined => profiles.find((p) => p.id === id)?.name;

  const value = useMemo<AppStateValue>(
    () => ({
      initialized,
      start: () =>
        reportAndRethrow("初期化に失敗しました", async () => {
          await initializeStore();
          await Promise.all([refreshProfiles(), refreshTags()]);
          setInitializedState(true);
        }),

      profiles,
      activeProfileId,
      setActiveProfileId: (id) =>
        reportAndRethrow("プロファイルの切り替えに失敗しました", async () => {
          const name = findNameById(id);
          if (!name) throw new Error(`profile not found: ${id}`);
          await ipcSetActiveProfile(name);
          await refreshProfiles();
        }),
      createProfile: (name, templateValue) =>
        reportAndRethrow("プロファイルの作成に失敗しました", async () => {
          const seedRules = templateValue ? PROFILE_TEMPLATE_RULES[templateValue] : undefined;
          const id = await ipcCreateProfile(name, "", seedRules ? withRuleIds(seedRules) : []);
          // profile-storeの既定は「アクティブが未設定の場合のみ」自動アクティブ化するため、
          // GUI固有の「新規作成分は常にアクティブにする」挙動はここで明示的に行う。
          await ipcSetActiveProfile(name);
          await refreshProfiles();
          return String(id);
        }),
      duplicateProfile: (id, newName) =>
        reportAndRethrow("プロファイルの複製に失敗しました", async () => {
          const source = profiles.find((p) => p.id === id);
          if (!source) throw new Error(`profile not found: ${id}`);
          const detail = await ipcGetProfile(source.name);
          const newId = await ipcCreateProfile(newName, detail.description, detail.rules);
          if (source.tags.length > 0) await ipcSetProfileTags(newName, source.tags);
          await refreshProfiles();
          return String(newId);
        }),
      deleteProfile: (id) =>
        reportAndRethrow("プロファイルの削除に失敗しました", async () => {
          const name = findNameById(id);
          if (!name) throw new Error(`profile not found: ${id}`);
          await ipcDeleteProfile(name);
          await refreshProfiles();
        }),
      toggleFavorite: (id) =>
        reportAndRethrow("お気に入りの更新に失敗しました", async () => {
          const target = profiles.find((p) => p.id === id);
          if (!target) throw new Error(`profile not found: ${id}`);
          await ipcSetFavorite(target.name, !target.isFavorite);
          await refreshProfiles();
        }),
      getProfileDetail: (id) =>
        reportAndRethrow("プロファイルの読み込みに失敗しました", async () => {
          const name = findNameById(id);
          if (!name) throw new Error(`profile not found: ${id}`);
          return ipcGetProfile(name);
        }),
      updateProfile: (id, meta) =>
        reportAndRethrow("プロファイルの保存に失敗しました", async () => {
          const oldName = findNameById(id);
          if (!oldName) throw new Error(`profile not found: ${id}`);
          await ipcUpdateProfile(oldName, meta.name, meta.description, meta.rules);
          // refreshProfilesの成否に関わらずfindNameById/profilesRefが常に新しい
          // 名前を解決できるようにする(そうしないと、この後refreshProfilesや
          // タグ更新が失敗した場合に、再試行時点で既に存在しない旧名を使い続けて
          // 永久に失敗する)。
          setProfiles((prev) => prev.map((p) => (p.id === id ? { ...p, name: meta.name } : p)));
          if (meta.tags !== undefined) {
            // idからの再解決はしない: meta.nameを直接使う(再解決するとリネーム前の
            // 名前を掴んだままの古いクロージャを参照してしまう、というのが元々の
            // 不具合だった)。
            await ipcSetProfileTags(meta.name, meta.tags);
          }
          await refreshProfiles();
        }),
      setProfileTags: (id, tags) =>
        reportAndRethrow("タグの更新に失敗しました", () => {
          const previous = tagUpdateQueues.current.get(id) ?? Promise.resolve();
          const next = previous.catch(() => {}).then(async () => {
            // 実行直前に解決する: キュー待ちの間にルール編集画面側の保存で
            // 同じプロファイルが改名されている可能性があるため、発行時点の
            // クロージャ(findNameById)ではなくprofilesRef経由で実行時点の
            // 最新の名前を使う。
            const name = profilesRef.current.find((p) => p.id === id)?.name;
            if (!name) throw new Error(`profile not found: ${id}`);
            await ipcSetProfileTags(name, tags);
            await refreshProfiles();
          });
          tagUpdateQueues.current.set(id, next);
          return next;
        }),
      exportProfile: (id, passphrase, destPath) =>
        reportExportErrorAndRethrow(async () => {
          const name = findNameById(id);
          if (!name) throw new Error(`profile not found: ${id}`);
          await ipcExportProfileToFile(name, passphrase, destPath);
        }),
      exportAll: (passphrase, destPath) =>
        reportExportErrorAndRethrow(async () => {
          await ipcExportAllToFile(passphrase, destPath);
        }),
      // ここは意図的にreportAndRethrow(汎用トースト)を使わない: 誤ったパスフレーズは
      // 想定内の入力ミスであり、モックアップ通り呼び出し元(パスフレーズ入力欄)で
      // インラインエラーとして表示する。
      previewImport: (sourcePath, passphrase) => ipcPreviewImport(sourcePath, passphrase),
      commitImport: () =>
        reportAndRethrow("インポートに失敗しました", async () => {
          const { activated_profile_name } = await ipcCommitPendingImport();
          await Promise.all([refreshProfiles(), refreshTags()]);
          // アクティブ未設定だった場合、無言でインポート内容がアクティブ化されうる
          // (監査指摘対応)。プレビュー画面でも事前に示されるが、実際に確定した
          // 結果としてここでも改めて知らせる。
          if (activated_profile_name) {
            toast.info(`プロファイル「${activated_profile_name}」がアクティブになりました`);
          }
        }),
      // キャンセル操作からのみ呼ばれる想定(commit成功/失敗時はRust側で既に消費済み)。
      // 失敗しても致命的ではない(プロセス内メモリの後始末のみ)ためトーストは出さない。
      clearPendingImport: () => ipcClearPendingImport().catch(() => {}),

      tags,
      createTag: (name) =>
        reportAndRethrow("タグの作成に失敗しました", async () => {
          await ipcCreateTag(name);
          await refreshTags();
        }),
      renameTag: (id, newName) =>
        // Tag.idはprofile-store側にidが無いためタグ名そのものを使っている。
        reportAndRethrow("タグ名の変更に失敗しました", async () => {
          await ipcRenameTag(id, newName);
          await Promise.all([refreshTags(), refreshProfiles()]);
        }),
      deleteTag: (id) =>
        reportAndRethrow("タグの削除に失敗しました", async () => {
          await ipcDeleteTag(id);
          await Promise.all([refreshTags(), refreshProfiles()]);
        }),

      inputText,
      setInputText,
      outputText,
      statusText,
      runMask: async () => {
        const activeProfile = profiles.find((p) => p.isActive);
        if (!activeProfile) return;
        try {
          const detail = await ipcGetProfile(activeProfile.name);
          const { text, matchCounts } = await maskText(
            activeProfile.id,
            activeProfile.name,
            detail.rules,
            inputText
          );
          const totalMatches = matchCounts.reduce((sum, m) => sum + m.count, 0);
          setOutputText(text);
          setStatusText(
            `アクティブプロファイル: ${activeProfile.name} ・ 直近のマスク実行でマッピング${totalMatches}件を置換`
          );
        } catch (error) {
          console.error("mask_text failed", error);
          toast.error("マスク実行に失敗しました");
          setStatusText(`アクティブプロファイル: ${activeProfile.name} ・ マスク実行に失敗しました`);
        }
      },
      clearInput: () => {
        setInputText("");
        // マスク実行のたびに蓄積する対応表(実在の機微情報を保持)を、クリア操作に
        // 合わせて破棄する。失敗しても致命的ではないため(プロセス内メモリの
        // 後始末のみ)トーストは出さない。
        const activeProfile = profiles.find((p) => p.isActive);
        if (activeProfile) ipcClearMappings(activeProfile.id).catch(() => {});
      },
    }),
    [initialized, profiles, activeProfileId, tags, inputText, outputText, statusText]
  );

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}

export function useAppState(): AppStateValue {
  const ctx = useContext(AppStateContext);
  if (!ctx) throw new Error("useAppState must be used within AppStateProvider");
  return ctx;
}
