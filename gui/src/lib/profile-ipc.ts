import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RuleListItem } from "@/components/rule-edit-screen";
import { recordPendingImportIdForE2e } from "./e2e-pending-import";
import { fromRuleDto, toRuleDto, type RuleProfileDto } from "./masking-ipc";

export interface ProfileSummaryDto {
  id: number;
  name: string;
  rule_count: number;
  enabled_rule_count: number;
  is_favorite: boolean;
  is_active: boolean;
  updated_at: string;
  tags: string[];
}

export interface ProfileDetail {
  name: string;
  description: string;
  rules: RuleListItem[];
}

function toProfileDto(name: string, description: string, rules: RuleListItem[]): RuleProfileDto {
  return { profile_name: name, description: description || null, rules: rules.map(toRuleDto) };
}

export async function isStoreInitialized(): Promise<boolean> {
  return invoke<boolean>("is_store_initialized");
}

// 2回目以降の起動用。ディスク上は初期化済みでも、プロセスを跨いで保持されない
// メモリ上のストア状態を実体化するために、起動のたびに一度呼ぶ必要がある。
export async function openStore(): Promise<void> {
  await invoke("open_store");
}

export async function initializeStore(): Promise<void> {
  await invoke("initialize_store");
}

export async function listProfiles(): Promise<ProfileSummaryDto[]> {
  return invoke<ProfileSummaryDto[]>("list_profiles");
}

export async function getProfile(name: string): Promise<ProfileDetail> {
  const dto = await invoke<RuleProfileDto>("get_profile", { name });
  return { name: dto.profile_name, description: dto.description ?? "", rules: dto.rules.map(fromRuleDto) };
}

export async function createProfile(
  name: string,
  description: string,
  rules: RuleListItem[]
): Promise<number> {
  return invoke<number>("create_profile", { profile: toProfileDto(name, description, rules) });
}

export async function updateProfile(
  oldName: string,
  name: string,
  description: string,
  rules: RuleListItem[]
): Promise<void> {
  await invoke("update_profile", { oldName, profile: toProfileDto(name, description, rules) });
}

export async function deleteProfile(name: string): Promise<void> {
  await invoke("delete_profile", { name });
}

export async function setActiveProfile(name: string): Promise<void> {
  await invoke("set_active_profile", { name });
}

export async function setFavorite(name: string, isFavorite: boolean): Promise<void> {
  await invoke("set_favorite", { name, isFavorite });
}

export async function listTags(): Promise<string[]> {
  return invoke<string[]>("list_tags");
}

export async function createTag(name: string): Promise<void> {
  await invoke("create_tag", { name });
}

export async function renameTag(oldName: string, newName: string): Promise<void> {
  await invoke("rename_tag", { oldName, newName });
}

export async function deleteTag(name: string): Promise<void> {
  await invoke("delete_tag", { name });
}

export async function setProfileTags(profileName: string, tags: string[]): Promise<void> {
  await invoke("set_profile_tags", { profileName, tags });
}

export async function exportProfileToFile(
  name: string,
  passphrase: string,
  destPath: string
): Promise<void> {
  await invoke("export_profile_to_file", { name, passphrase, destPath });
}

export async function exportAllToFile(passphrase: string, destPath: string): Promise<void> {
  await invoke("export_all_to_file", { passphrase, destPath });
}

// インポート確認画面でルールの中身を表示するための型。
// 「構文的に有効だが実データの書式と食い違う」細工されたルールに、確定前に
// 気付けるようにするための情報であり、確定前に必ず提示する。
export interface ImportRuleDto {
  name: string;
  pattern_type: "literal" | "regex";
  pattern: string;
  mode: "fixed" | "sequential";
  fixed_value: string | null;
  prefix: string | null;
  enabled: boolean;
}

// Rust側のExportImportErrorに対応する型。invalid_inputはパスフレーズによる復号を
// 試みる前の事前検証(パス形式・拡張子・サイズ・保存先)で弾かれたことを示すため、
// 呼び出し元はこれをパスフレーズ入力エラーとして表示してはならない。
export interface ExportImportError {
  kind: "invalid_input" | "failed";
  message: string;
}

export function isExportImportError(error: unknown): error is ExportImportError {
  if (typeof error !== "object" || error === null) return false;
  const { kind, message } = error as Record<string, unknown>;
  return (kind === "invalid_input" || kind === "failed") && typeof message === "string";
}

export interface ImportEntryDto {
  original_name: string;
  resolved_name: string;
  renamed: boolean;
  rules: ImportRuleDto[];
  tags: string[];
}

export type ImportPreviewDto =
  | { kind: "single"; name: string; rules: ImportRuleDto[]; tags: string[] }
  | { kind: "all"; will_activate_profile_name: string | null; entries: ImportEntryDto[] };

// previewImportの結果。pendingIdは、復号済みの内容をRust側に保留した、保留の識別子で、確定
// (commitPendingImport)と破棄(clearPendingImport)は、この識別子で、その保留だけを指す。
export interface ImportPreviewResult {
  pendingId: number;
  preview: ImportPreviewDto;
}

// 保留の識別子は、Rust側のu64。JavaScriptの数値として正確に扱える、0以上の安全な整数だけを有効とする。
function isPendingId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

// Rustのclear_pending_importは、識別子が無い・nullのとき、全ての保留(他の画面が始めた復号の保留も)を破棄する。
// undefinedはキーごと落ち、NaN・Infinityはnullに直列化されるため、無効な識別子のままinvokeを呼ぶと、その呼び出しに
// なってしまう。そのため、確定・破棄は、識別子を検証し、無効なら、invokeを呼ばずに拒否する。
function assertPendingId(value: unknown): asserts value is number {
  if (!isPendingId(value)) throw new TypeError(`invalid pending import id: ${String(value)}`);
}

export async function previewImport(sourcePath: string, passphrase: string): Promise<ImportPreviewResult> {
  // 応答の形は、型引数で断定できるだけで、Rust側との取り決めがずれると、識別子が欠ける。識別子の無い保留を
  // 画面へ渡さないよう、実行時に検証する。
  const response = await invoke<{ pending_id: unknown; preview: ImportPreviewDto } | null | undefined>(
    "preview_import",
    { sourcePath, passphrase }
  );
  const pendingId = response?.pending_id;
  if (!response || !isPendingId(pendingId)) {
    throw new Error(`preview_import returned an invalid pending_id: ${String(pendingId)}`);
  }
  recordPendingImportIdForE2e(pendingId);
  return { pendingId, preview: response.preview };
}

export interface CommitImportResultDto {
  activated_profile_name: string | null;
}

// 保留の識別子で指定した保留だけを確定する。その保留が無い(破棄済み・確定済み・保持数の上限で捨てられた)場合は失敗する。
export async function commitPendingImport(pendingId: number): Promise<CommitImportResultDto> {
  assertPendingId(pendingId);
  return invoke<CommitImportResultDto>("commit_pending_import", { pendingId });
}

// 保留の識別子で指定した保留だけを破棄する(他の保留は消さない)。preview_importが復号した平文をプロセス内に
// 残さないためと、破棄した後にcommitPendingImportを呼んでも確定しないようにするため、確認画面の取り消し・
// 画面を離れるとき・閉じられた画面へ遅れて届いた復号結果の破棄・確認されないまま残った前の保留の破棄で呼ぶ。
// 識別子を省略して全ての保留を消す呼び方(E2Eの後片付け用)は、意図せず他の保留を消さないよう、ここには設けない。
export async function clearPendingImport(pendingId: number): Promise<void> {
  assertPendingId(pendingId);
  await invoke("clear_pending_import", { pendingId });
}

// フォーカスの有無に関わらず、変更を検知したウィンドウ側で最新状態を取り直すためのイベント。
// 今は単一ウィンドウだが、将来の別ウィンドウ化でそのまま使う想定(PoCで検証済み)。
export function onProfilesChanged(handler: () => void): Promise<() => void> {
  return listen("profiles-changed", handler);
}

export function onTagsChanged(handler: () => void): Promise<() => void> {
  return listen("tags-changed", handler);
}

export interface ActiveProfileRulesWeakenedPayload {
  profileName: string;
  weakenedRuleNames: string[];
}

// update_profileの呼び出し元(通常の編集操作、または直接IPCを叩く経路の両方を問わず)が
// アクティブプロファイルの既存の有効ルールを無効化・削除、またはマスク挙動を左右する
// 内容(pattern等)を書き換えた場合にRust側から届く。マスクルールが無言で無力化される
// のに気付く簡易な手がかりとして使う(SensitiveMasker側の設計、改ざん耐性のある記録
// ではなく即時の通知であることが重要)。
export function onActiveProfileRulesWeakened(
  handler: (payload: ActiveProfileRulesWeakenedPayload) => void
): Promise<() => void> {
  return listen<ActiveProfileRulesWeakenedPayload>("active-profile-rules-weakened", (event) =>
    handler(event.payload)
  );
}
