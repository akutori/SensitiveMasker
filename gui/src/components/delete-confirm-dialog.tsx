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

export interface DeleteConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  targetType: string;
  targetName: string;
  onConfirm: () => void;
}

export function DeleteConfirmDialog({
  open,
  onOpenChange,
  targetType,
  targetName,
  onConfirm,
}: DeleteConfirmDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>削除の確認</AlertDialogTitle>
          <AlertDialogDescription>
            {targetType}「{targetName}」を削除しますか?
          </AlertDialogDescription>
        </AlertDialogHeader>
        <p className="text-sm font-medium text-foreground">この操作は取り消せません。</p>
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
