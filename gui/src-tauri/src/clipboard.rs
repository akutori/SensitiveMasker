use std::sync::Mutex;

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

const CLIPBOARD_ERROR: &str = "クリップボードを操作できませんでした";

/// clear_clipboard_if_matchesの結果。フロントエンドはこれを見て、ユーザーに手動での
/// クリアを促すかどうかを判断する(Cleared/SkippedContentChangedは追加の対応不要、
/// SkippedUnableToVerifyのみ警告が必要)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ClipboardClearOutcome {
    Cleared,
    SkippedContentChanged,
    SkippedUnableToVerify,
}

/// write_clipboard_textで書き込まれ、まだ「クリア済み」または「既に別内容に
/// 変わったことを確認済み」になっていない値を保持する。アプリ終了時にここへ値が
/// 残っていれば、JS側のsetTimeout(プロセス終了と共に消える)に代わって終了直前に
/// 一度だけクリアを試みるために使う(clear_pending_on_exit)。
///
/// 同じMutexを書き込み・クリア両コマンドの本体全体(読み取り・比較・書き込みの間)で
/// 保持することで、この2つの呼び出しがOSクリップボードへの読み書きの間で競合しない
/// ようにする(片方が古いタイマー由来でも、新しい方の書き込みを後から消してしまう
/// ことがなくなる)。ここではawaitを挟まないためstd::sync::Mutexで足りる。
#[derive(Default)]
pub struct ClipboardState(Mutex<Option<String>>);

/// 現在のクリップボードの内容(取得できなかった場合はNone)と、コピー時点の値を
/// 比較し、クリアしてよいかを判定する(純粋関数。OSのクリップボードに触れずに
/// テストできるよう読み取り自体とは分離する)。
///
/// Noneは「別の内容だった」ではなく「確認できなかった」ことを表す。取得失敗には
/// クリップボードが空/テキスト以外の内容である正常なケースと、他プロセスによる
/// 占有などの異常なケースの両方が含まれプラグインのエラー型からは区別できないため、
/// 安全側に倒していずれもクリアしない。
fn decide_outcome(current: Option<&str>, expected: &str) -> ClipboardClearOutcome {
    match current {
        Some(text) if text == expected => ClipboardClearOutcome::Cleared,
        Some(_) => ClipboardClearOutcome::SkippedContentChanged,
        None => ClipboardClearOutcome::SkippedUnableToVerify,
    }
}

fn write_clipboard_text_impl<R: Runtime>(
    app: &AppHandle<R>,
    state: &ClipboardState,
    text: String,
) -> Result<(), String> {
    let mut pending = state.0.lock().unwrap_or_else(|e| e.into_inner());
    // 書き込み試行前にpendingへ反映する。Windows版のarboard呼び出しはテキスト書き込み後に
    // 履歴/クラウド除外フォーマットの設定を行う2段階の処理で、後段だけ失敗してもErrが
    // 返るため、成功後に反映する書き方だとテキストは実際にクリップボードへ残っているのに
    // pendingが空のままになり得る。decide_outcomeは実際のクリップボード内容と比較してから
    // 判定するため、書き込みが完全に失敗した場合にpendingだけ残っても誤ってクリアされない。
    *pending = Some(text.clone());
    write_to_os_clipboard(app, &text)
}

/// Windowsでは書き込みと同時にSetExtWindows(exclude_from_history/exclude_from_cloud)で
/// クリップボード履歴・クラウド同期からの除外を行うため、tauri-plugin-clipboard-manager
/// (内部でarboardをラップ)を経由せずarboardを直接呼ぶ。このWindows専用拡張traitはプラグイン
/// からは呼べないため。読み取り・クリア、および非Windowsでの書き込みは対象外(プラグイン経由のまま)。
#[cfg(windows)]
fn write_to_os_clipboard<R: Runtime>(_app: &AppHandle<R>, text: &str) -> Result<(), String> {
    use arboard::SetExtWindows;

    let mut clipboard = arboard::Clipboard::new().map_err(|_| CLIPBOARD_ERROR.to_string())?;
    clipboard
        .set()
        .exclude_from_cloud()
        .exclude_from_history()
        .text(text)
        .map_err(|_| CLIPBOARD_ERROR.to_string())
}

#[cfg(not(windows))]
fn write_to_os_clipboard<R: Runtime>(app: &AppHandle<R>, text: &str) -> Result<(), String> {
    app.clipboard().write_text(text.to_string()).map_err(|_| CLIPBOARD_ERROR.to_string())
}

/// clear_clipboard_if_matchesの後、pending追跡をクリアしてよいかの判定(純粋関数)。
///
/// SkippedUnableToVerify(確認できなかった)の間は追跡を残す。まだクリップボード上に
/// 残っている可能性を捨てないことで、後続の再試行(終了時のclear_pending_on_exit等)に
/// 機会を残すため。それ以外(クリア済み、または既に無関係な内容と確認できた)は、この
/// 値についての追跡はもう不要。ただし追跡中の値が既に別の(より新しい)コピーに置き
/// 換わっている場合、それはこの呼び出しとは無関係なので触らない。
fn should_forget_pending(outcome: ClipboardClearOutcome, pending: Option<&str>, expected: &str) -> bool {
    outcome != ClipboardClearOutcome::SkippedUnableToVerify && pending == Some(expected)
}

fn clear_clipboard_if_matches_impl<R: Runtime>(
    app: &AppHandle<R>,
    state: &ClipboardState,
    expected: &str,
) -> Result<ClipboardClearOutcome, String> {
    let mut pending = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let current = app.clipboard().read_text().ok();
    let outcome = decide_outcome(current.as_deref(), expected);
    if outcome == ClipboardClearOutcome::Cleared {
        app.clipboard().write_text("").map_err(|_| CLIPBOARD_ERROR.to_string())?;
    }
    if should_forget_pending(outcome, pending.as_deref(), expected) {
        *pending = None;
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn write_clipboard_text(
    app: tauri::AppHandle,
    state: tauri::State<'_, ClipboardState>,
    text: String,
) -> Result<(), String> {
    write_clipboard_text_impl(&app, &state, text)
}

/// 現在のクリップボードの内容がexpectedのままであればクリアする。読み取り・比較・
/// クリアをRust側で完結させることで、navigator.clipboard.readText()が要求する
/// ウィンドウフォーカス・clipboard-read権限に依存しない(フォーカスが外れている状態が
/// コピー後最も一般的な操作フローであるため)。
#[tauri::command]
pub async fn clear_clipboard_if_matches(
    app: tauri::AppHandle,
    state: tauri::State<'_, ClipboardState>,
    expected: String,
) -> Result<ClipboardClearOutcome, String> {
    clear_clipboard_if_matches_impl(&app, &state, &expected)
}

/// トレイメニューの「終了」からの終了直前に呼ぶ。JS側のsetTimeoutはプロセス終了と
/// 共に消えるため、これが無いと保留中の自動クリアが実行されないまま終了し、
/// パスフレーズがクリップボードに残り続ける(コピー後すぐにアプリを終了する、という
/// ごく普通の操作フローで発生しうる)。追跡している値があれば、終了直前に一度だけ
/// クリアを試みる(失敗しても終了自体は妨げない)。
pub fn clear_pending_on_exit<R: Runtime>(app: &AppHandle<R>, state: &ClipboardState) {
    let pending = state.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(expected) = pending {
        let _ = clear_clipboard_if_matches_impl(app, state, &expected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_outcome_clears_when_current_content_still_matches() {
        assert_eq!(decide_outcome(Some("secret"), "secret"), ClipboardClearOutcome::Cleared);
    }

    #[test]
    fn decide_outcome_skips_when_content_has_changed() {
        assert_eq!(
            decide_outcome(Some("something-else"), "secret"),
            ClipboardClearOutcome::SkippedContentChanged
        );
    }

    #[test]
    fn decide_outcome_skips_when_current_content_cannot_be_read() {
        assert_eq!(decide_outcome(None, "secret"), ClipboardClearOutcome::SkippedUnableToVerify);
    }

    #[test]
    fn should_forget_pending_after_a_successful_clear() {
        assert!(should_forget_pending(ClipboardClearOutcome::Cleared, Some("secret"), "secret"));
    }

    #[test]
    fn should_forget_pending_once_content_is_confirmed_changed() {
        assert!(should_forget_pending(ClipboardClearOutcome::SkippedContentChanged, Some("secret"), "secret"));
    }

    #[test]
    fn should_not_forget_pending_when_it_could_not_be_verified() {
        // 再試行(終了時のclear_pending_on_exit等)の機会を残すため、確認できなかった
        // 場合は追跡を消してはならない。
        assert!(!should_forget_pending(ClipboardClearOutcome::SkippedUnableToVerify, Some("secret"), "secret"));
    }

    #[test]
    fn should_not_forget_pending_that_belongs_to_a_newer_copy() {
        // pendingが既に別の(より新しい)値に置き換わっている場合、今回の呼び出しは
        // その新しい値とは無関係なので触ってはならない。
        assert!(!should_forget_pending(ClipboardClearOutcome::Cleared, Some("newer-secret"), "secret"));
    }
}
