import { Button } from "@/components/ui/button";

export interface InitialSetupScreenProps {
  onStart: () => void;
}

export function InitialSetupScreen({ onStart }: InitialSetupScreenProps) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-muted p-8">
      <div className="w-full max-w-2xl rounded-lg border bg-background p-10">
        <h1 className="text-xl font-bold">SensitiveMaskerへようこそ</h1>
        <p className="mt-4 text-sm">
          初めての起動です。マスキングルールを保存するための、暗号化されたローカルデータベースを作成します。
        </p>
        <div className="mt-6 rounded-md border bg-muted p-4 text-sm">
          保存先: OS標準のアプリデータフォルダ(Windowsの場合はAppDataフォルダ配下)。復号用の鍵はデータベースとは別ファイルに分離し、所有ユーザーのみアクセスできるよう保護されます。
        </div>
        <div className="mt-8 flex justify-end">
          <Button onClick={onStart}>始める</Button>
        </div>
      </div>
    </div>
  );
}
