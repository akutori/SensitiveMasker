import { useRef, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, userEvent, waitFor } from "storybook/test";
import { Button } from "@/components/ui/button";
import { CLIPBOARD_CLEAR_DELAY_SECONDS } from "@/lib/clipboard-clear-delay";
import {
  beginExport,
  beginWriting,
  completeExport,
  openExportDialog,
  regeneratePassphrase,
} from "@/lib/export-dialog-state";
import { ExportModal } from "./export-modal";

const meta = {
  component: ExportModal,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ExportModal>;

export default meta;
type Story = StoryObj<typeof meta>;

const INITIAL_PASSPHRASE = "K7m-9pQx-2Lw4-Rt8v";
const PASSPHRASE_LABEL = "生成されたパスフレーズ(自動生成):";

function randomDemoPassphrase() {
  const words = Array.from({ length: 4 }, () =>
    Math.random().toString(36).slice(2, 6)
  );
  return words.join("-");
}

// 実際のルートと同じ状態遷移の関数を使い、エクスポート実行(保存先の選択→書き込み)を0.2秒後の成功として模擬する。
function DemoTrigger(props: { target: string; label: string; openByDefault?: boolean }) {
  const [open, setOpen] = useState(props.openByDefault ?? false);
  const [session, setSession] = useState(() => openExportDialog(1, INITIAL_PASSPHRASE));
  const sessionCounter = useRef(1);

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setSession(openExportDialog(++sessionCounter.current, INITIAL_PASSPHRASE));
          setOpen(true);
        }}
      >
        {props.label}
      </Button>
      <ExportModal
        open={open}
        onOpenChange={setOpen}
        target={props.target}
        passphrase={session.passphrase}
        status={session.phase}
        clipboardBusy={false}
        onCopy={() => {
          navigator.clipboard.writeText(session.passphrase);
        }}
        onRegenerate={() => setSession((current) => regeneratePassphrase(current, randomDemoPassphrase()))}
        onExport={() => {
          const { sessionId, passphrase } = session;
          setSession((current) => beginExport(current, sessionId, passphrase));
          window.setTimeout(() => setSession((current) => beginWriting(current, sessionId)), 100);
          window.setTimeout(() => setSession((current) => completeExport(current, sessionId)), 200);
        }}
      />
    </>
  );
}

// ダイアログの外側(暗くした背景)を押す操作。
function clickOverlay() {
  const overlay = document.querySelector<HTMLElement>('[data-slot="dialog-overlay"]');
  if (!overlay) throw new Error("ダイアログの背景が見つからない");
  return userEvent.click(overlay);
}

export const SingleProfile: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "editing",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  render: () => <DemoTrigger target="SIP監視用" label="エクスポート(単一)" />,
};

export const AllProfiles: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    target: "全プロファイル",
    passphrase: INITIAL_PASSPHRASE,
    status: "editing",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  render: () => <DemoTrigger target="全プロファイル" label="エクスポート(全体)" />,
};

// 編集中は、Escapeと背景を押す操作でも閉じられる(書き出す前なので失うものがない)。
export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "editing",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  play: async ({ args }) => {
    await expect(await screen.findByRole("button", { name: "コピー" })).toBeEnabled();
    await expect(await screen.findByRole("button", { name: "再生成" })).toBeEnabled();
    await expect(await screen.findByRole("button", { name: "エクスポート" })).toBeEnabled();
    await expect(await screen.findByRole("button", { name: "キャンセル" })).toBeEnabled();
    // 読み上げの領域は、書き出す前から存在し、中身は空である(後から中身が入ることで、読み上げられる)。
    await expect(screen.getByRole("status")).toBeEmptyDOMElement();
    // コピーしたパスフレーズが消えるまでの時間を、注意文で伝える。
    await expect(
      screen.getByText(new RegExp(`${CLIPBOARD_CLEAR_DELAY_SECONDS}秒後に、自動クリアを試みます`))
    ).toBeInTheDocument();

    await userEvent.keyboard("{Escape}");
    await expect(args.onOpenChange).toHaveBeenCalledTimes(1);
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
    await clickOverlay();
    await expect(args.onOpenChange).toHaveBeenCalledTimes(2);
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
  },
};

// コピー/クリアのIPC応答待ちの間、コピー・再生成が無効化されることの確認用。
export const BusyDisablesButtons: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "editing",
    clipboardBusy: true,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  play: async () => {
    await expect(await screen.findByRole("button", { name: "コピー" })).toBeDisabled();
    await expect(await screen.findByRole("button", { name: "再生成" })).toBeDisabled();
    await expect(await screen.findByRole("button", { name: "エクスポート" })).toBeEnabled();
  },
};

// 保存先の選択中は、まだ何も書き出していないので、閉じられる(閉じると、書き出しは取り消される)。書き出す
// パスフレーズを変えさせないため、再生成とエクスポートだけが無効になる。
export const Choosing: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "choosing",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  play: async ({ args }) => {
    for (const name of ["再生成", "エクスポート"]) {
      await expect(await screen.findByRole("button", { name })).toBeDisabled();
    }
    for (const name of ["パスフレーズを表示", "コピー", "キャンセル"]) {
      await expect(await screen.findByRole("button", { name })).toBeEnabled();
    }
    await expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
    // 進み具合は、読み上げの領域(aria-live)へ出る。書き出し完了の通知の領域(status)は、空のまま。
    await expect(screen.getByText(/保存先を選択しています/)).toHaveAttribute("aria-live", "polite");
    await expect(screen.getByRole("status")).toBeEmptyDOMElement();

    await userEvent.keyboard("{Escape}");
    await expect(args.onOpenChange).toHaveBeenCalledTimes(1);
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
    await clickOverlay();
    await expect(args.onOpenChange).toHaveBeenCalledTimes(2);
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
  },
};

// 書き込み中は、書き出すパスフレーズを変えさせず、失わせないため、全ての操作が無効になり、
// 閉じる操作(×・Escape・背景を押す)も受け付けない。
export const Writing: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "writing",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  play: async ({ args }) => {
    for (const name of ["パスフレーズを表示", "コピー", "再生成", "エクスポート", "キャンセル"]) {
      await expect(await screen.findByRole("button", { name })).toBeDisabled();
    }
    await expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
    await expect(screen.getByText("書き込んでいます…")).toHaveAttribute("aria-live", "polite");

    await userEvent.keyboard("{Escape}");
    await clickOverlay();
    await expect(args.onOpenChange).not.toHaveBeenCalled();
  },
};

// 書き出し済み: パスフレーズの表示・コピーと閉じることだけができ、再生成・エクスポートは出ない
// (書き出したファイルと画面のパスフレーズが食い違うため)。表示は伏せ字から始まる。
// 唯一の表示を誤って失わないよう、Escapeと背景を押す操作では閉じず、「閉じる」と×だけで閉じる。
export const Exported: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "exported",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  play: async ({ args }) => {
    await expect(await screen.findByRole("status")).toHaveTextContent("二度と表示できません");
    await expect(screen.queryByRole("button", { name: "再生成" })).toBeNull();
    await expect(screen.queryByRole("button", { name: "エクスポート" })).toBeNull();
    await expect(screen.queryByRole("button", { name: "キャンセル" })).toBeNull();
    await expect(screen.getByRole("button", { name: "コピー" })).toBeEnabled();
    await expect(screen.getByRole("button", { name: "閉じる" })).toBeEnabled();

    const input = screen.getByLabelText(PASSPHRASE_LABEL);
    await expect(input).toHaveAttribute("type", "password");
    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");
    await expect(input).toHaveValue(INITIAL_PASSPHRASE);

    await userEvent.keyboard("{Escape}");
    await clickOverlay();
    await expect(args.onOpenChange).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    await expect(args.onOpenChange).toHaveBeenCalledTimes(1);
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
    await userEvent.click(screen.getByRole("button", { name: "閉じる" }));
    await expect(args.onOpenChange).toHaveBeenCalledTimes(2);
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
  },
};

// 実行から成功までの一連の流れ。表示していても、成功した時点で伏せ字へ戻る。
export const ExportFlow: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    status: "editing",
    clipboardBusy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  render: () => <DemoTrigger target="SIP監視用" label="エクスポート(単一)" openByDefault />,
  play: async () => {
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    await userEvent.click(await screen.findByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");

    // 押したボタンが無効になっても、フォーカスは画面の外へ出ず、入力欄へ移る
    // (キーボード操作が背景の画面へ漏れないようにするため)。
    await expect(screen.getByRole("button", { name: "パスフレーズを隠す" })).toHaveAttribute(
      "aria-controls",
      input.id
    );
    await userEvent.click(await screen.findByRole("button", { name: "エクスポート" }));
    await waitFor(() => expect(input).toHaveFocus());

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("二度と表示できません")
    );
    await expect(input).toHaveAttribute("type", "password");
    await expect(input).toHaveValue(INITIAL_PASSPHRASE);
    await expect(screen.queryByRole("button", { name: "再生成" })).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "閉じる" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  },
};
