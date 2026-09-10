import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { patternTypeLabel, modeLabel } from "@/components/rule-edit-screen";
import type { ImportRuleDto } from "@/lib/profile-ipc";

export interface ImportPreviewRow {
  profileName: string;
  result: string;
  rules: ImportRuleDto[];
}

export interface ImportConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  rows: ImportPreviewRow[];
  onConfirm: () => void;
}

function modeAndValue(rule: ImportRuleDto) {
  const value = rule.mode === "fixed" ? rule.fixed_value : rule.prefix;
  return `${modeLabel(rule.mode)}: ${value}`;
}

// 横スクロールが必要になった場合でも、左側(ルール名・状態・種別・モード/値)は
// スクロール前に見える位置に置き、パターンだけを右端(最も可変長で長くなりうる列)
// にする。無効ルールに気付けることがSMX-1対応の目的そのものであるため、
// 「状態」列はスクロールしないと見えない位置に置かない。
function RuleTable({ rules }: { rules: ImportRuleDto[] }) {
  if (rules.length === 0) {
    return <p className="px-3 py-2 text-sm text-muted-foreground">ルールがありません。</p>;
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>ルール名</TableHead>
          <TableHead>状態</TableHead>
          <TableHead>種別</TableHead>
          <TableHead>モード / 値</TableHead>
          <TableHead>パターン</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rules.map((rule) => (
          <TableRow key={rule.name} className={rule.enabled ? undefined : "opacity-50"}>
            <TableCell className="whitespace-nowrap">{rule.name}</TableCell>
            <TableCell className="whitespace-nowrap">{rule.enabled ? "有効" : "無効"}</TableCell>
            <TableCell className="whitespace-nowrap">{patternTypeLabel(rule.pattern_type)}</TableCell>
            <TableCell className="max-w-40 whitespace-nowrap overflow-hidden text-ellipsis font-mono text-xs">
              {modeAndValue(rule)}
            </TableCell>
            <TableCell className="max-w-72 break-all font-mono text-xs">{rule.pattern}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

export function ImportConfirmDialog({
  open,
  onOpenChange,
  rows,
  onConfirm,
}: ImportConfirmDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      {/* AlertDialogContentの既定サイズ指定(data-[size=default]:sm:max-w-sm)は
          属性セレクタを含み通常のsm:max-w-*より詳細度が高く上書きされないため、
          important修飾子で明示的に勝たせる。 */}
      <AlertDialogContent className="sm:!max-w-4xl">
        <AlertDialogHeader>
          <AlertDialogTitle>インポート内容の確認</AlertDialogTitle>
          <AlertDialogDescription>
            取り込まれるルールの内容を確認してから、インポートを実行するか選択してください。
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="max-h-[28rem] divide-y overflow-y-auto rounded-md border">
          {rows.map((row) => (
            <div key={row.profileName} className="p-3">
              <div className="mb-2 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
                <span className="font-medium">{row.profileName}</span>
                <span className="text-sm text-muted-foreground">{row.result}</span>
              </div>
              <div className="overflow-hidden rounded-md border">
                <RuleTable rules={row.rules} />
              </div>
            </div>
          ))}
        </div>

        <AlertDialogFooter>
          <AlertDialogAction onClick={onConfirm}>インポート実行</AlertDialogAction>
          <AlertDialogCancel>キャンセル</AlertDialogCancel>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
