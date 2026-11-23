import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, screen, userEvent, waitFor, within } from "storybook/test";
import { Toaster } from "@/components/ui/sonner";
import { notifyZeroMatchRules } from "@/lib/zero-match-notice";
import { MainScreen, type ProfileOption } from "./main-screen";

const meta = {
  component: MainScreen,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof MainScreen>;

export default meta;
type Story = StoryObj<typeof meta>;

const PROFILES: ProfileOption[] = [
  { id: "1", name: "SIP監視用" },
  { id: "2", name: "Asterisk本番ログ" },
];

const SAMPLE_TEXT =
  "着信: 0120-000-000\nSIP URI: sip:alice@203.0.113.10\nパスワード: hunter2";

// デモ用の簡易マスク処理(実際のマスキングはRust側masking-coreがIPC経由で行う想定、
// ここでは「マスク実行」クリックでoutputTextが更新されることを示すためだけの簡略版)。
function demoMask(text: string): string {
  return text
    .replace(/0\d{2,4}-\d{2,4}-\d{3,4}/g, "__MASK_TEL_1")
    .replace(/sip:[\w.]+@[\d.]+/g, "__MASK_SIP_1")
    .replace(/hunter2/g, "[REDACTED]");
}

// 指定したルール名を、マッチ0件として、マスク実行のたびにトーストで知らせる(アプリ本体と同じ関数を使う)。
function DemoScreen(props: { initialProfiles: ProfileOption[]; zeroMatchRuleNames?: string[] }) {
  const [activeProfileId, setActiveProfileId] = useState<string | null>(
    props.initialProfiles[0]?.id ?? null
  );
  const [inputText, setInputText] = useState(SAMPLE_TEXT);
  const [outputText, setOutputText] = useState("");
  const [statusText, setStatusText] = useState("アクティブプロファイル: なし");

  const activeProfile = props.initialProfiles.find((p) => p.id === activeProfileId);

  return (
    <>
      {props.zeroMatchRuleNames && <Toaster />}
      <MainScreen
        profiles={props.initialProfiles}
        activeProfileId={activeProfileId}
        onActiveProfileIdChange={setActiveProfileId}
        onOpenProfileList={() => console.log("open profile list")}
        onImport={() => console.log("import")}
        onReload={() => console.log("reload")}
        onNewProfile={() => console.log("new profile")}
        onEditProfile={() => console.log("edit profile")}
        inputText={inputText}
        onInputTextChange={setInputText}
        onLoadFromFile={() => console.log("load from file")}
        onMaskExecute={() => {
          const masked = demoMask(inputText);
          setOutputText(masked);
          setStatusText(
            `アクティブプロファイル: ${activeProfile?.name ?? "なし"} ・ 直近のマスク実行でマッピング3件を置換`
          );
          notifyZeroMatchRules(
            (props.zeroMatchRuleNames ?? []).map((ruleName) => ({ ruleName, count: 0 }))
          );
        }}
        onClear={() => setInputText("")}
        outputText={outputText}
        onSaveToFile={() => console.log("save to file")}
        onCopyToClipboard={() => console.log("copy to clipboard")}
        statusText={statusText}
      />
    </>
  );
}

export const Default: Story = {
  args: {
    profiles: PROFILES,
    activeProfileId: PROFILES[0].id,
    onActiveProfileIdChange: () => {},
    onOpenProfileList: () => {},
    onImport: () => {},
    onReload: () => {},
    onNewProfile: () => {},
    onEditProfile: () => {},
    inputText: SAMPLE_TEXT,
    onInputTextChange: () => {},
    onLoadFromFile: () => {},
    onMaskExecute: () => {},
    onClear: () => {},
    outputText: "",
    onSaveToFile: () => {},
    onCopyToClipboard: () => {},
    statusText: `アクティブプロファイル: ${PROFILES[0].name}`,
  },
  render: () => <DemoScreen initialProfiles={PROFILES} />,
};

export const NoProfiles: Story = {
  args: {
    ...Default.args,
    profiles: [],
    activeProfileId: null,
    statusText: "アクティブプロファイル: なし",
  },
  render: () => <DemoScreen initialProfiles={[]} />,
};

const ZERO_MATCH_RULE_NAMES = ["jp_phone_number", "ipv4_address", "password_kv", "email_address"];
const ZERO_MATCH_MESSAGE = /マッチ件数が0件のルールがあります/;

// 出現の動き(画面の外からせり上がる)が終わった矩形を返す。動きの前や途中の矩形で比べると、重なりを見落とす。
// 負荷が高いと、動きが始まる前の位置が、続けて2回同じになるため、表示の準備ができて、画面内に収まっていることも要る。
async function settledRect(element: HTMLElement): Promise<DOMRect> {
  let previous: DOMRect | undefined;
  for (let i = 0; i < 50; i++) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    const current = element.getBoundingClientRect();
    const inScreen =
      element.dataset.mounted === "true" &&
      current.top >= 0 &&
      current.bottom <= window.innerHeight;
    if (
      previous &&
      inScreen &&
      current.x === previous.x &&
      current.y === previous.y &&
      current.width === previous.width &&
      current.height === previous.height
    ) {
      return current;
    }
    previous = current;
  }
  throw new Error("トーストが、画面内で位置を落ち着かせなかった");
}

function rectsOverlap(a: DOMRect, b: DOMRect): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

// マッチ0件のルールの警告は、右寄せの保存・コピーのボタンを覆わず、×で閉じられる。
export const ZeroMatchWarning: Story = {
  args: Default.args,
  render: () => (
    <DemoScreen initialProfiles={PROFILES} zeroMatchRuleNames={ZERO_MATCH_RULE_NAMES} />
  ),
  play: async () => {
    await userEvent.click(await screen.findByRole("button", { name: "マスク実行 ->" }));

    const message = await screen.findByText(ZERO_MATCH_MESSAGE);
    await expect(message).toHaveTextContent(
      "マッチ件数が0件のルールがあります(jp_phone_number、ipv4_address、password_kv、email_address)。意図した設定でない場合はルール設定を見直してください。"
    );
    const toastElement = message.closest<HTMLElement>("[data-sonner-toast]");
    if (!toastElement) throw new Error("トーストの要素が見つからない");

    const toastRect = await settledRect(toastElement);
    await expect(toastRect.left).toBeGreaterThanOrEqual(0);
    await expect(toastRect.bottom).toBeLessThanOrEqual(window.innerHeight);
    for (const name of ["ファイルに保存", "クリップボードにコピー"]) {
      const buttonRect = screen.getByRole("button", { name }).getBoundingClientRect();
      await expect(buttonRect.width, `${name}の矩形が空`).toBeGreaterThan(0);
      await expect(rectsOverlap(toastRect, buttonRect), `トーストが「${name}」と重なっている`).toBe(false);
    }

    // ×は、トーストの右上にある。
    const closeButton = within(toastElement).getByRole("button", { name: "通知を閉じる" });
    const closeRect = closeButton.getBoundingClientRect();
    await expect(closeRect.left + closeRect.width / 2, "×がトーストの右半分に無い").toBeGreaterThan(
      toastRect.left + toastRect.width / 2
    );
    await expect(closeRect.top + closeRect.height / 2, "×がトーストの上半分に無い").toBeLessThan(
      toastRect.top + toastRect.height / 2
    );

    await userEvent.click(closeButton);
    await waitFor(() => expect(toastElement).not.toBeInTheDocument());
  },
};

// 閉じる操作をしなくても、5秒ほどで自動で消える(カーソルは、押した「マスク実行」の上に残り、トーストの上には無い)。
export const ZeroMatchWarningAutoDismiss: Story = {
  args: Default.args,
  render: () => (
    <DemoScreen initialProfiles={PROFILES} zeroMatchRuleNames={ZERO_MATCH_RULE_NAMES} />
  ),
  play: async () => {
    await userEvent.click(await screen.findByRole("button", { name: "マスク実行 ->" }));

    const message = await screen.findByText(ZERO_MATCH_MESSAGE);
    await waitFor(() => expect(message).not.toBeInTheDocument(), { timeout: 8000 });
  },
};
