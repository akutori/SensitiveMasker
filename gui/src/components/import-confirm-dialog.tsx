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

export interface ImportPreviewRow {
  profileName: string;
  result: string;
}

export interface ImportConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  rows: ImportPreviewRow[];
  onConfirm: () => void;
}

export function ImportConfirmDialog({
  open,
  onOpenChange,
  rows,
  onConfirm,
}: ImportConfirmDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent className="sm:max-w-xl">
        <AlertDialogHeader>
          <AlertDialogTitle>インポート内容の確認</AlertDialogTitle>
          <AlertDialogDescription className="sr-only">
            インポートするプロファイルの一覧と適用結果を確認し、インポートを実行するか選択してください。
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="max-h-64 overflow-y-auto rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>プロファイル名</TableHead>
                <TableHead>結果</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.profileName}>
                  <TableCell>{row.profileName}</TableCell>
                  <TableCell className="whitespace-normal">{row.result}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>

        <AlertDialogFooter>
          <AlertDialogAction onClick={onConfirm}>インポート実行</AlertDialogAction>
          <AlertDialogCancel>キャンセル</AlertDialogCancel>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
