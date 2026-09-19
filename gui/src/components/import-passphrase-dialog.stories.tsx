import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, screen, userEvent, waitFor, within } from "storybook/test";
import { Button } from "@/components/ui/button";
import { ImportPassphraseDialog } from "./import-passphrase-dialog";

const meta = {
  component: ImportPassphraseDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ImportPassphraseDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const FILE_NAME = "profiles_export_20260901.smx";
const PASSPHRASE_LABEL = "パスフレーズ:";
const ERROR_MESSAGE =
  "パスフレーズが誤っているか、対応していないファイル形式です";

function DemoTrigger(props: { initialError?: string }) {
  const [open, setOpen] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [errorMessage, setErrorMessage] = useState(props.initialError);

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setPassphrase("");
          setErrorMessage(props.initialError);
          setOpen(true);
        }}
      >
        インポート
      </Button>
      <ImportPassphraseDialog
        open={open}
        onOpenChange={setOpen}
        fileName={FILE_NAME}
        passphrase={passphrase}
        onPassphraseChange={(value) => {
          setPassphrase(value);
          setErrorMessage(undefined);
        }}
        errorMessage={errorMessage}
        onConfirm={() => {
          if (passphrase === "correct") {
            setOpen(false);
          } else {
            setErrorMessage(ERROR_MESSAGE);
          }
        }}
      />
    </>
  );
}

export const Default: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
};

export const WithError: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    errorMessage: ERROR_MESSAGE,
    onConfirm: () => {},
  },
  render: () => <DemoTrigger initialError={ERROR_MESSAGE} />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "wrong-passphrase",
    onPassphraseChange: () => {},
    errorMessage: ERROR_MESSAGE,
    onConfirm: () => {},
  },
  // 入力欄は伏せ字から始まり、表示切替で入力内容を確認できる(貼り付けに混ざった空白などを
  // 目で確かめられるようにするため)。
  play: async () => {
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    await expect(input).toHaveAttribute("type", "password");
    // 入力があり、復号していなければ、OKを押せる。
    await expect(screen.getByRole("button", { name: "OK" })).toBeEnabled();
    await expect(input).not.toHaveAttribute("readonly");
    // 状態の領域は、復号していない間は、書き出す前から置いてあって、中身は空である。
    await expect(screen.getByRole("status")).toBeEmptyDOMElement();
    // 伏せ字でも表示でも、入力したパスフレーズが自動補完の履歴や綴り確認の対象にならない。
    await expect(input).toHaveAttribute("autocomplete", "off");
    await expect(input).toHaveAttribute("spellcheck", "false");

    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");
    await expect(input).toHaveAttribute("autocomplete", "off");
    await expect(input).toHaveAttribute("spellcheck", "false");
    await expect(input).toHaveValue("wrong-passphrase");

    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを隠す" }));
    await expect(input).toHaveAttribute("type", "password");
  },
};

// 手入力の途中で「表示」をマウスで押した後も、入力欄にフォーカスとキャレット位置が戻り、続けて入力できる
// (ボタンにフォーカスが残ると、続けて打った文字が入力されない)。キーボードで押したときは、
// 繰り返し切り替えられるよう、フォーカスをボタンに残す。
export const RevealKeepsTyping: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(await canvas.findByRole("button", { name: "インポート" }));
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    await userEvent.type(input, "ab");

    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");
    await expect(input).toHaveFocus();
    await userEvent.keyboard("cd");
    await expect(input).toHaveValue("abcd");

    const hideButton = screen.getByRole("button", { name: "パスフレーズを隠す" });
    hideButton.focus();
    await userEvent.keyboard("{Enter}");
    await expect(input).toHaveAttribute("type", "password");
    await expect(screen.getByRole("button", { name: "パスフレーズを表示" })).toHaveFocus();
  },
};

// 復号している間は、OKを押せず、入力も変えられず、その理由を文言で示す。復号を待つ間も、閉じることはできる。
export const Busy: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "dummy-passphrase-0001",
    onPassphraseChange: () => {},
    onConfirm: () => {},
    busy: true,
  },
  play: async () => {
    await expect(await screen.findByRole("button", { name: "OK" })).toBeDisabled();
    await expect(screen.getByLabelText(PASSPHRASE_LABEL)).toHaveAttribute("readonly");
    await expect(screen.getByRole("status")).toHaveTextContent("復号しています");
    await expect(screen.getByRole("button", { name: "キャンセル" })).toBeEnabled();
  },
};

// OKを押して復号が始まると、押したボタンが無効になってフォーカスを失い、Tabで背景の画面へ出られてしまうため、
// フォーカスを入力欄へ移す。復号が終わると、OKと入力が元に戻り、状態の文言は消える。
function BusyDemo() {
  const [busy, setBusy] = useState(false);
  const [passphrase, setPassphrase] = useState("dummy-passphrase-0001");
  return (
    <ImportPassphraseDialog
      open
      onOpenChange={() => {}}
      fileName={FILE_NAME}
      passphrase={passphrase}
      onPassphraseChange={setPassphrase}
      busy={busy}
      onConfirm={() => {
        setBusy(true);
        window.setTimeout(() => setBusy(false), 600);
      }}
    />
  );
}

export const BusyMovesFocus: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "dummy-passphrase-0001",
    onPassphraseChange: () => {},
    onConfirm: () => {},
  },
  render: () => <BusyDemo />,
  play: async () => {
    // 高さの検証は、DialogFooterが縦に積まれる幅(640px未満)でだけ意味がある(横並びになる幅では、行の固定を
    // 外しても、高さは変わらない)。幅は、vite.config.tsで固定している。
    await expect(window.innerWidth, "この検証は、幅640px未満で行う").toBeLessThan(640);
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    const ok = screen.getByRole("button", { name: "OK" });
    const dialog = screen.getByRole("dialog");
    // 開くときの拡大の動きの影響を受けない、レイアウト上の高さで比べる。
    const heightBefore = dialog.offsetHeight;
    await expect(screen.getByRole("status")).toBeEmptyDOMElement();

    await userEvent.click(ok);
    await waitFor(() => expect(ok).toBeDisabled());
    await expect(input).toHaveFocus();
    await expect(screen.getByRole("status")).toHaveTextContent("復号しています");
    // 文言が出ても、画面の高さは変わらない(画面は中央寄せのため、高さが変わると、内容が動いて見える)。
    await expect(dialog.offsetHeight).toBe(heightBefore);

    await waitFor(() => expect(ok).toBeEnabled());
    await expect(input).not.toHaveAttribute("readonly");
    await expect(screen.getByRole("status")).toBeEmptyDOMElement();
    await expect(dialog.offsetHeight).toBe(heightBefore);
  },
};

// 開き直すたびに、表示していても伏せ字から始まる(前回の表示状態を引き継がない)。
export const RevealResetsWhenReopened: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(await canvas.findByRole("button", { name: "インポート" }));
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");

    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await userEvent.click(await canvas.findByRole("button", { name: "インポート" }));
    await expect(await screen.findByLabelText(PASSPHRASE_LABEL)).toHaveAttribute("type", "password");
  },
};
