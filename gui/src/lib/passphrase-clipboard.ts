// パスフレーズのコピーと、その自動クリアの制御。
//
// 書き込み・確認・クリアは全てRust側のコマンドで行う。navigator.clipboard.readText()は
// ウィンドウのフォーカスとclipboard-read権限を要求し、コピー後に他アプリへ切り替えるという
// 最も一般的な操作フローで失敗するため使わない。
//
// generationは「この呼び出しが今なお最新の操作か」の判定に一本化して使う(書き込み完了時の判定だけで
// なく、自動クリアの結果が返ってきた時点でも同じ判定に使う)。既に次のコピー/再生成が発生していれば、
// 古い呼び出しの結果は(成功・失敗を問わず)通知や状態更新の対象にしない。

import { CLIPBOARD_CLEAR_DELAY_MS } from "./clipboard-clear-delay";
import type { ClipboardClearOutcome } from "./clipboard-ipc";

export interface PassphraseClipboardDeps {
  write: (text: string) => Promise<void>;
  // クリップボードの内容がexpectedのままであればクリアする(Rust側で比較・クリアまで行う)。
  clearIfMatches: (expected: string) => Promise<ClipboardClearOutcome>;
  // コピー/クリアのIPCの応答待ちを数える(operation-counter.ts)。
  track: <T>(operation: Promise<T>) => Promise<T>;
  notify: {
    copied: () => void;
    copyFailed: () => void;
    // 「確認できなかった」だけでは「まだ残っている」とは断定できない(他の内容に既に上書きされて
    // いた場合も、読み取り自体は失敗しうるため)。断定形の通知にしない。
    notCleared: () => void;
  };
  // 自動クリアまでの時間。既定は、利用者へ示す秒数のもとになる定数。
  delayMs?: number;
}

export interface PassphraseClipboard {
  copy: (value: string) => void;
  // 再生成の前に呼ぶ。タイマーの取り消しだけでは、既にコピー済みの値がクリップボードに残り続けるため、
  // その場でクリアを試みる。
  clearNow: () => void;
}

export function createPassphraseClipboard(deps: PassphraseClipboardDeps): PassphraseClipboard {
  const delayMs = deps.delayMs ?? CLIPBOARD_CLEAR_DELAY_MS;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;
  // 直近でコピーに成功したパスフレーズ(自動クリア待ちの間だけ保持)。再生成時にその場でクリアする
  // ため、タイマーの生存とは別に、値そのものを覚えておく。
  let lastCopied: string | null = null;

  const cancelTimer = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  };

  // 一致していればクリアし、確認できなかった場合は通知する。onClearedは、クリアの結果が返った
  // (どの結果でも)ときに、それが最新の操作であれば呼ぶ。
  const attemptClear = (value: string, mine: number, onSettled?: () => void) => {
    deps
      .track(deps.clearIfMatches(value))
      .then((result) => {
        if (generation !== mine) return;
        if (result.outcome === "skipped_unable_to_verify") deps.notify.notCleared();
        onSettled?.();
      })
      .catch(() => {
        if (generation === mine) deps.notify.notCleared();
      });
  };

  return {
    copy(value) {
      cancelTimer();
      const mine = ++generation;
      deps
        .track(deps.write(value))
        .then(() => {
          if (generation !== mine) return;
          lastCopied = value;
          deps.notify.copied();
          timer = setTimeout(() => {
            timer = null;
            attemptClear(value, mine, () => {
              lastCopied = null;
            });
          }, delayMs);
        })
        .catch(() => {
          if (generation !== mine) return;
          deps.notify.copyFailed();
          // 書き込みの後段だけが失敗しても(Windowsでは、テキストの書き込みの後に、履歴・クラウド
          // 同期からの除外を設定する2段階の処理)、値は実際にはクリップボードに残ることがある。
          // 一致する場合だけ消し(何も書かれていなければ、何も起きない)、その結果は通知しない
          // (失敗は既に通知している)。
          deps.track(deps.clearIfMatches(value)).catch(() => {});
        });
    },
    clearNow() {
      cancelTimer();
      const mine = ++generation;
      const copied = lastCopied;
      if (!copied) return;
      lastCopied = null;
      attemptClear(copied, mine);
    },
  };
}
