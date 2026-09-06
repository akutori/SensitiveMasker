//! MCPツール定義。`masking-core`/`profile-store`には依存せず、対象コマンドの出力を
//! `masker mask --stream`にパイプするだけの薄い層として実装する。

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{schemars, tool, tool_router, ErrorData as McpError};

use crate::executor::run_piped;
use crate::masker_path::resolve_masker_path;

const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
// timeout_secondsはMCP経由(LLM等)からの入力であり、極端な値(例: u64::MAX)を渡されると
// Instant + Durationの加算がオーバーフローしてpanicする(実機で確認済み)。呼び出し全体を
// 永久にハングさせないため、妥当な範囲に収める。
const MAX_TIMEOUT_SECONDS: u64 = 3600;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct RunMaskedCommandParams {
    /// 実行するシェルコマンド(例: "asterisk -rx \"core show channels\"")
    pub command: String,
    /// 収集を打ち切るまでの秒数(省略時は300秒)
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    /// 使用するプロファイル名(省略時はアクティブプロファイル)
    #[serde(default)]
    pub profile: Option<String>,
    /// 対象コマンドの出力のエンコーディング(WHATWG Encoding Standardのラベル名、例:
    /// "shift-jis")。省略時はUTF-8として扱う
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Clone)]
pub(crate) struct MaskerMcp;

#[tool_router(server_handler)]
impl MaskerMcp {
    #[tool(
        description = "指定したシェルコマンドを実行し、その標準出力をmasker(機微情報マスキングCLI)経由でマスクしてから返す。terraform、asterisk -rx、fs_cli、tail -f等、機微情報が出力に混じる可能性のあるコマンドの実行に使う。標準エラー出力は対象外。出力がUTF-8以外(Shift-JIS等)の場合はencodingで明示的に指定する(省略時はUTF-8として扱う)。"
    )]
    async fn run_masked_command(
        &self,
        Parameters(params): Parameters<RunMaskedCommandParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.command.trim().is_empty() {
            return Err(McpError::invalid_params("commandは空にできません", None));
        }
        if params.command.contains(['\n', '\r']) {
            // cmd.exe /Cは埋め込まれた改行以降を無警告で無視する(実機で確認済み)ため、
            // 一部だけが実行されたことに気づけない事故を避けるために拒否する。
            return Err(McpError::invalid_params("commandに改行を含めることはできません", None));
        }

        let masker_path = match resolve_masker_path() {
            Ok(path) => path,
            Err(e) => return Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())])),
        };

        let timeout = Duration::from_secs(resolve_timeout_seconds(params.timeout_seconds));
        let masker_args = build_masker_args(params.profile.as_deref(), params.encoding.as_deref());

        let result = match run_piped(&params.command, &masker_path.to_string_lossy(), &masker_args, timeout).await {
            Ok(result) => result,
            Err(e) => return Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())])),
        };

        if let Some(stderr) = result.second_stage_error {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "maskerの実行に失敗しました: {stderr}"
            ))]));
        }

        let mut text = result.output;
        if result.timed_out {
            text.push_str("\n\n[masker-mcp: タイムアウトにより打ち切りました]");
        }
        if result.truncated {
            text.push_str("\n\n[masker-mcp: 出力が大きすぎるため打ち切りました]");
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }
}

fn resolve_timeout_seconds(requested: Option<u64>) -> u64 {
    requested.unwrap_or(DEFAULT_TIMEOUT_SECONDS).min(MAX_TIMEOUT_SECONDS)
}

fn build_masker_args(profile: Option<&str>, encoding: Option<&str>) -> Vec<String> {
    let mut args = vec!["mask".to_string(), "--stream".to_string()];
    if let Some(profile) = profile {
        args.push("--profile".to_string());
        args.push(profile.to_string());
    }
    if let Some(encoding) = encoding {
        args.push("--encoding".to_string());
        args.push(encoding.to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_profile_or_encoding_produces_the_bare_stream_args() {
        assert_eq!(build_masker_args(None, None), vec!["mask", "--stream"]);
    }

    #[test]
    fn resolve_timeout_seconds_uses_the_default_when_omitted() {
        assert_eq!(resolve_timeout_seconds(None), DEFAULT_TIMEOUT_SECONDS);
    }

    #[test]
    fn resolve_timeout_seconds_passes_through_a_reasonable_value() {
        assert_eq!(resolve_timeout_seconds(Some(30)), 30);
    }

    #[test]
    fn resolve_timeout_seconds_clamps_extreme_values() {
        assert_eq!(resolve_timeout_seconds(Some(u64::MAX)), MAX_TIMEOUT_SECONDS);
    }

    #[test]
    fn duration_from_the_clamped_value_never_panics_even_for_extreme_input() {
        // 実機で確認された問題(u64::MAXを渡すとInstant + Durationの加算でpanicする)の
        // 回帰テスト。clampを経由すればDuration::from_secsも安全な範囲に収まる。
        let _ = Duration::from_secs(resolve_timeout_seconds(Some(u64::MAX)));
        let _ = tokio::time::Instant::now() + Duration::from_secs(resolve_timeout_seconds(Some(u64::MAX)));
    }

    #[test]
    fn profile_is_passed_through_as_a_flag() {
        assert_eq!(
            build_masker_args(Some("work"), None),
            vec!["mask", "--stream", "--profile", "work"]
        );
    }

    #[test]
    fn encoding_is_passed_through_as_a_flag() {
        assert_eq!(
            build_masker_args(None, Some("shift-jis")),
            vec!["mask", "--stream", "--encoding", "shift-jis"]
        );
    }

    #[test]
    fn profile_and_encoding_can_both_be_passed_through_together() {
        assert_eq!(
            build_masker_args(Some("work"), Some("shift-jis")),
            vec!["mask", "--stream", "--profile", "work", "--encoding", "shift-jis"]
        );
    }
}
