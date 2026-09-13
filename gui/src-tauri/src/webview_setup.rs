use tauri::{Manager, Runtime};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
use windows_core::Interface;

// WebView2既定のブラウザアクセラレータキー(Ctrl+F/F3のページ内検索等)を無効化する。
// 有効なままだとWebView2自身の検索とMonaco Editor組み込みの検索/置換ウィジェットが
// 同じCtrl+F/Escapeキーを取り合い、find widgetが正しく閉じない。
pub fn disable_browser_accelerator_keys<R: Runtime>(app: &tauri::App<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.with_webview(|webview| unsafe {
        let Ok(core_webview) = webview.controller().CoreWebView2() else {
            return;
        };
        let Ok(settings) = core_webview.Settings() else {
            return;
        };
        if let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() {
            let _ = settings3.SetAreBrowserAcceleratorKeysEnabled(false);
        }
    });
}
