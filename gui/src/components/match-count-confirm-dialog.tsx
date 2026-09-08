import { cn } from "cn";
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

export interface MatchCountRow {
  ruleName: string;
  matchCount: number;
}

export interface MatchCountConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  rows: MatchCountRow[];
  onConfirm: () => void;
}

export function MatchCountConfirmDialog({
  open,
  onOpenChange,
  rows,
  onConfirm,
}: MatchCountConfirmDialogProps) {
  const zeroCountRules = rows.filter((row) => row.matchCount === 0);

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>直接変換マッチ件数確認</AlertDialogTitle>
          <AlertDialogDescription>
            マスク処理が完了しました(マスク後の内容は表示されません)。ルールごとのマッチ件数を確認し、保存を続行するか選択してください。
          </AlertDialogDescription>
        </AlertDialogHeader>

        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>ルール名</TableHead>
              <TableHead>マッチ件数</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.ruleName}>
                <TableCell className={cn(row.matchCount === 0 && "font-bold")}>
                  {row.ruleName}
                </TableCell>
                <TableCell className={cn(row.matchCount === 0 && "font-bold")}>
                  {row.matchCount}件
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>

        {zeroCountRules.length > 0 && (
          <p className="text-sm font-bold">
            注意: マッチ件数が0件のルールがあります(
            {zeroCountRules.map((row) => row.ruleName).join("、")}
            )。意図した設定でない場合は「いいえ」を選択し、ルール設定を見直してください。
          </p>
        )}

        <p className="text-sm">この内容で保存を続行しますか?</p>

        <AlertDialogFooter>
          <AlertDialogAction variant="outline" onClick={onConfirm}>
            はい
          </AlertDialogAction>
          <AlertDialogCancel variant="outline">いいえ</AlertDialogCancel>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
