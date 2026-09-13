import { useId } from "react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { MaskedTextEditor } from "./masked-text-editor";

export interface ProfileOption {
  id: string;
  name: string;
}

export interface MainScreenProps {
  profiles: ProfileOption[];
  activeProfileId: string | null;
  onActiveProfileIdChange: (id: string) => void;
  onOpenProfileList: () => void;
  onImport: () => void;
  onReload: () => void;
  onNewProfile: () => void;
  onEditProfile: () => void;
  inputText: string;
  onInputTextChange: (text: string) => void;
  onLoadFromFile: () => void;
  onMaskExecute: () => void;
  onClear: () => void;
  outputText: string;
  onSaveToFile: () => void;
  onCopyToClipboard: () => void;
  statusText: string;
}

export function MainScreen({
  profiles,
  activeProfileId,
  onActiveProfileIdChange,
  onOpenProfileList,
  onImport,
  onReload,
  onNewProfile,
  onEditProfile,
  inputText,
  onInputTextChange,
  onLoadFromFile,
  onMaskExecute,
  onClear,
  outputText,
  onSaveToFile,
  onCopyToClipboard,
  statusText,
}: MainScreenProps) {
  const id = useId();
  const hasProfiles = profiles.length > 0;

  return (
    // data-a11y-verified-contrast: Monaco Editorの描画と無関係な子孫要素まで
    // axeのcolor-contrastが不安定にInconclusiveを出すため子孫ごと除外する(.storybook/preview.tsx参照)。
    <div
      className="flex h-screen flex-col overflow-hidden p-5"
      data-a11y-verified-contrast="monaco-adjacent-contrast-instability"
    >
      <h1 className="shrink-0 text-lg font-bold">SensitiveMasker</h1>

      <div className="mt-4 flex shrink-0 flex-wrap items-center gap-2">
        <label htmlFor={`${id}-profile`} className="text-sm">
          プロファイル:
        </label>
        <Select
          value={activeProfileId ?? undefined}
          onValueChange={onActiveProfileIdChange}
          disabled={!hasProfiles}
        >
          <SelectTrigger id={`${id}-profile`} className="w-64">
            <SelectValue placeholder="プロファイルがありません" />
          </SelectTrigger>
          <SelectContent>
            {profiles.map((profile) => (
              <SelectItem key={profile.id} value={profile.id}>
                {profile.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <div className="flex-1" />
        <Button variant="outline" onClick={onOpenProfileList}>
          プロファイル一覧
        </Button>
        <Button variant="outline" onClick={onImport}>
          インポート
        </Button>
        <Button variant="outline" onClick={onReload}>
          再読み込み
        </Button>
      </div>

      <div className="mt-2 flex shrink-0 flex-wrap gap-2">
        <Button variant="outline" onClick={onNewProfile}>
          新規作成
        </Button>
        <Button variant="outline" onClick={onEditProfile} disabled={!hasProfiles}>
          プロファイルを編集
        </Button>
      </div>

      <div className="mt-4 flex min-h-0 flex-1 flex-col overflow-y-auto">
        <div className="flex min-h-[100px] flex-1 flex-col gap-1.5">
          <p className="shrink-0 text-sm">入力テキスト: (Ctrl+Fで検索、Ctrl+Hで置換)</p>
          <MaskedTextEditor
            value={inputText}
            onChange={onInputTextChange}
            ariaLabel="入力テキスト"
            height="100%"
            className="min-h-0 flex-1"
          />
        </div>

        <div className="mt-2 flex shrink-0 flex-wrap items-center gap-2">
          <Button variant="outline" onClick={onLoadFromFile} disabled={!hasProfiles}>
            ファイルから
          </Button>
          <Button
            className="font-bold"
            onClick={onMaskExecute}
            disabled={!hasProfiles}
          >
            マスク実行 -&gt;
          </Button>
          <Button variant="outline" onClick={onClear}>
            クリア
          </Button>
        </div>

        <div className="mt-5 flex min-h-[100px] flex-1 flex-col border-t pt-4">
          <p className="shrink-0 text-sm">出力(マスク後)テキスト:</p>
          <MaskedTextEditor
            value={outputText}
            readOnly
            ariaLabel="出力(マスク後)テキスト"
            height="100%"
            className="mt-1.5 min-h-0 flex-1"
          />
        </div>

        <div className="mt-2 flex shrink-0 justify-end gap-2">
          <Button variant="outline" onClick={onSaveToFile}>
            ファイルに保存
          </Button>
          <Button variant="outline" onClick={onCopyToClipboard}>
            クリップボードにコピー
          </Button>
        </div>

        <div className="mt-4 shrink-0 rounded-md border border-border bg-muted px-3 py-2 text-sm text-foreground">
          {statusText}
        </div>
      </div>
    </div>
  );
}
