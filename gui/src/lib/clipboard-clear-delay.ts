// コピーしたパスフレーズをクリップボードから自動クリアするまでの時間。
// タイマー(profiles.tsx)と、利用者へ「何秒後か」を伝える表示(通知・注意文)が
// 同じ値を参照し、食い違わないようにするため、1か所で定義する。

// コピー後この時間が経過したら、クリップボードの中身がまだこのパスフレーズの
// ままであることを確認した上でクリアする(モックアップ6の要件)。
export const CLIPBOARD_CLEAR_DELAY_MS = 30_000;

export const CLIPBOARD_CLEAR_DELAY_SECONDS = CLIPBOARD_CLEAR_DELAY_MS / 1000;
