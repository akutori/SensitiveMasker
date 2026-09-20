use std::sync::Mutex;

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;
use zeroize::Zeroizing;

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
///
/// 保持する値はパスフレーズのため、dropのときにメモリを消去する型(`Zeroizing`)で持つ。
#[derive(Default)]
pub struct ClipboardState(Mutex<Option<Zeroizing<String>>>);

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
    // IPCで受け取った文字列は、そのまま、追跡している値として持つ(複製を作らない)。
    let text = Zeroizing::new(text);
    let mut pending = state.0.lock().unwrap_or_else(|e| e.into_inner());
    // 書き込み試行前にpendingへ反映する。Windows版のarboard呼び出しはテキスト書き込み後に
    // 履歴/クラウド除外フォーマットの設定を行う2段階の処理で、後段だけ失敗してもErrが
    // 返るため、成功後に反映する書き方だとテキストは実際にクリップボードへ残っているのに
    // pendingが空のままになり得る。decide_outcomeは実際のクリップボード内容と比較してから
    // 判定するため、書き込みが完全に失敗した場合にpendingだけ残っても誤ってクリアされない。
    let stored = pending.insert(text);
    write_to_os_clipboard(app, stored.as_str())
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
    // 読み取った内容は、パスフレーズそのものでありうるため、比較の後に消去する。
    let current = app.clipboard().read_text().ok().map(Zeroizing::new);
    let outcome = decide_outcome(current.as_ref().map(|text| text.as_str()), expected);
    if outcome == ClipboardClearOutcome::Cleared {
        app.clipboard().write_text("").map_err(|_| CLIPBOARD_ERROR.to_string())?;
    }
    if should_forget_pending(outcome, pending.as_ref().map(|text| text.as_str()), expected) {
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

/// マスク済みテキスト等、秘匿情報ではない値をクリップボードへ書き込む。
/// write_clipboard_text/ClipboardStateのpending追跡(終了時の自動クリア対象)には
/// 含めない。これを共有すると、メイン画面で「クリップボードにコピー」した後トレイの
/// 「終了」から抜けた場合、clear_pending_on_exitがまだ一致している値を秘匿情報の
/// 消し忘れと誤認し、無警告でクリップボードを空にしてしまう(実際に確認済みの不具合)。
/// tray.rsのmask_clipboardが元々プラグインを直接呼びこの追跡を経由しないのと
/// 同じ理由でこの関数を共有する。
///
/// pending追跡には加わらないが、Mutex自体はwrite_clipboard_text_impl/
/// clear_clipboard_if_matches_implと共有してロックする。これが無いと、
/// 「secretコピー→自動クリアタイマーがread→(ここでuntracked書き込みが割り込む)→
/// タイマーがwrite("")」という並びで、たった今書いたマスク結果を誤って消しうる
/// (read-compare-writeの間に割り込まれるTOCTOU)。同じMutexを保持することで、
/// この一連の操作全体を他のクリップボード操作と直列化する。
pub(crate) fn write_untracked<R: Runtime>(
    app: &AppHandle<R>,
    state: &ClipboardState,
    text: &str,
) -> Result<(), String> {
    let _guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    app.clipboard().write_text(text.to_string()).map_err(|_| CLIPBOARD_ERROR.to_string())
}

#[tauri::command]
pub async fn write_clipboard_text_untracked(
    app: tauri::AppHandle,
    state: tauri::State<'_, ClipboardState>,
    text: String,
) -> Result<(), String> {
    write_untracked(&app, &state, &text)
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
    let expected = Zeroizing::new(expected);
    clear_clipboard_if_matches_impl(&app, &state, expected.as_str())
}

/// トレイメニューの「終了」からの終了直前に呼ぶ。JS側のsetTimeoutはプロセス終了と
/// 共に消えるため、これが無いと保留中の自動クリアが実行されないまま終了し、
/// パスフレーズがクリップボードに残り続ける(コピー後すぐにアプリを終了する、という
/// ごく普通の操作フローで発生しうる)。追跡している値があれば、終了直前に一度だけ
/// クリアを試みる(失敗しても終了自体は妨げない)。
pub fn clear_pending_on_exit<R: Runtime>(app: &AppHandle<R>, state: &ClipboardState) {
    let pending = state.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(expected) = pending {
        let _ = clear_clipboard_if_matches_impl(app, state, expected.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_secret_is_held_in_a_type_that_is_erased_on_drop() {
        // 追跡している値は、dropのときに消去する型で持つ(Stringなどへ替えると、この関数がコンパイルできなくなる)。
        fn pending_of(state: &ClipboardState) -> &Mutex<Option<Zeroizing<String>>> {
            &state.0
        }
        let state = ClipboardState::default();
        assert!(pending_of(&state).lock().unwrap().is_none());
    }

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
