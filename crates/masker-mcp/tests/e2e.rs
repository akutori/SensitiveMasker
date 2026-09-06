//! `masker-mcp`バイナリを実際にサブプロセスとして起動し、MCPプロトコル経由でツールを呼び出す
//! 統合テスト。rmcp自身のクライアント機能(`TokioChildProcess`)を使う(公式SDKが示す検証方法)。
//!
//! 実機のmasker初期化状態(プロファイルの有無)には依存しない。ここで検証するのはMCPの
//! プロトコル層(ツール一覧に載るか、呼び出しがハングせずに整形されたレスポンスを返すか)
//! であり、マスキング結果の正しさそのものはmasking-core/masker crateの既存テストが担う。

use rmcp::model::CallToolRequestParams;
use rmcp::service::ServiceExt;
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

fn masker_mcp_bin() -> &'static str {
    env!("CARGO_BIN_EXE_masker-mcp")
}

#[tokio::test]
async fn lists_the_run_masked_command_tool() {
    let client = ().serve(TokioChildProcess::new(Command::new(masker_mcp_bin())).unwrap()).await.unwrap();

    let tools = client.list_tools(Default::default()).await.unwrap();

    assert!(tools.tools.iter().any(|t| t.name == "run_masked_command"), "{tools:?}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn rejects_an_empty_command_as_invalid_params() {
    let client = ().serve(TokioChildProcess::new(Command::new(masker_mcp_bin())).unwrap()).await.unwrap();

    let result = client
        .call_tool(
            CallToolRequestParams::new("run_masked_command")
                .with_arguments(serde_json::json!({ "command": "" }).as_object().unwrap().clone()),
        )
        .await;

    assert!(result.is_err(), "空のcommandはプロトコルエラーになるはず: {result:?}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn run_masked_command_returns_a_well_formed_response() {
    let client = ().serve(TokioChildProcess::new(Command::new(masker_mcp_bin())).unwrap()).await.unwrap();

    let result = client
        .call_tool(
            CallToolRequestParams::new("run_masked_command")
                .with_arguments(serde_json::json!({ "command": "echo hello" }).as_object().unwrap().clone()),
        )
        .await
        .unwrap();

    // maskerが未初期化ならtool-level error、初期化済みなら成功レスポンスになる。
    // 実機の状態によらず、プロトコル層が正しく機能して何らかのcontentを返すことを確認する。
    assert!(!result.content.is_empty(), "{result:?}");

    client.cancel().await.unwrap();
}
