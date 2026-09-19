//! シェルコマンドの出力を別プロセスの標準入力へOSレベルのパイプで直接接続し、その出力を
//! タイムアウト付きで収集する汎用ロジック。MCPプロトコル固有の型には依存しない。
//!
//! pty(疑似端末)は使わず、プレーンなパイプのみを使う。Windows実装(ConPTY)で単純な
//! コマンドですら応答待ちで停止する挙動を実機で確認したため、Unix系のisatty判定による
//! stdioバッファリングの緩和(対象コマンド側の対応)よりリスクが大きいと判断した。

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub(crate) const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PipedExecutionResult {
    pub output: String,
    pub timed_out: bool,
    pub truncated: bool,
    /// 2段目のプログラムが(タイムアウトによるkillではなく)自ら非0で終了した場合の標準エラー出力。
    pub second_stage_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExecutorError {
    #[error("コマンドの起動に失敗しました: {0}")]
    SpawnFirst(std::io::Error),
    #[error("{program}の起動に失敗しました: {source}")]
    SpawnSecond { program: String, source: std::io::Error },
}

// 標準エラー出力は意図的にマージしない(設計上の`{command} | masker mask --stream`は標準
// 出力のみのパイプであり、それに合わせる)。任意のユーザー指定コマンド文字列に対して
// 文字列レベルで`2>&1`を追記すると、コマンド文字列自身が既に持つリダイレクト
// (`1>&2`等)と競合し、シェルのリダイレクト解決順序次第で出力がどこにも渡らなくなる
// ことがある(実機で確認済み)。
fn build_shell_command(command: &str) -> Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        // `command`が`&&`・パイプ等を含む場合、shはそれを実行するために自身を残したまま
        // 子プロセスを起動する(execによる自身の置き換えが効かない)。Docker上のUbuntu/dash
        // とRocky Linux/bashの両方で、単純な1コマンドでもexec置換されない場合があることを
        // 実機で確認した。新しいプロセスグループのリーダーにしておき、kill時にグループ全体
        // (負のPID宛)を対象にすることで、子孫プロセスの取り残しを防ぐ。
        c.process_group(0);
        c
    }
    #[cfg(windows)]
    {
        // `Command::arg`のWindows向けエスケープはCommandLineToArgvW規約(子プロセスが
        // 通常のargvとして解釈する前提)を仮定しているが、`cmd.exe /C`は渡された文字列を
        // 独自の構文で解釈するため、これを経由すると引用符が余分にエスケープされ、
        // ユーザー指定コマンドの引用符が壊れる(実機で確認済み)。`raw_arg`で
        // エスケープを経由させず、文字列をそのまま渡す。
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.arg("/C");
        c.raw_arg(command);
        c
    }
}

/// `child`が`deadline`までに自然終了するかを、ブロッキングな`wait()`を使わずに監視する。
/// 終了を確認できたら`true`、`deadline`に達しても終了していなければ`false`を返す。
async fn wait_until(child: &mut std::process::Child, deadline: tokio::time::Instant) -> bool {
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return true,
            Ok(None) => {}
            // 状態取得自体が失敗した場合、無限に待ち続けるよりは終了扱いにして先に進む。
            Err(_) => return true,
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// `first_child`(`build_shell_command`で起動したシェル)とその子孫プロセスを終了させる。
/// Windowsの`cmd /C "<command>"`は対象コマンドを別プロセスとして起動するため、
/// `child`(=cmd.exe自身)をkillするだけでは対象コマンドが残り続ける。Unixでも`command`が
/// `&&`・パイプ等を含む場合、shをkillするだけでは子プロセスが残り続ける。いずれもDocker上の
/// 実機(Windows、Ubuntu/dash、Rocky Linux/bash)で確認済み。
/// Windowsは`taskkill /T`で子孫ごと終了させる。Unixは、`build_shell_command`で新しい
/// プロセスグループのリーダーにしておいた`child`のPIDをそのままプロセスグループIDとして、
/// グループ全体(子孫含む)へ`kill_process_group`でSIGKILLを送る。グループへ送れなかった場合は、
/// 呼び出し側の`wait()`が対象コマンドの自然終了まで無期限にブロックしないよう、
/// 直接の子だけでも終了させる。
fn kill_first_child_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        // PATH解決に依存すると、PATH上のtaskkillより前にある別の(悪意ある、または単に
        // 別の)同名実行ファイルが誤って実行されうる。SystemRoot(Windowsが必ず設定する
        // 環境変数)から実体の絶対パスを組み立てる。取得できない場合は、既存のkill失敗時
        // (プロセスが見つからない等)と同様にベストエフォートとして諦める。
        if let Some(taskkill) = taskkill_path() {
            let _ = Command::new(taskkill).args(["/F", "/T", "/PID", &pid]).output();
        }
    }
    #[cfg(unix)]
    {
        kill_group_or_direct_child(child, kill_process_group);
    }
}

/// プロセスグループ`pgid`の全員へSIGKILLを送る(システムコール`kill(-pgid, SIGKILL)`を直接呼ぶ)。
/// 外部の`kill`コマンドを起動しないため、PATH上の`kill`の有無や実装に左右されない。
/// グループが既に無い(ESRCH)場合は、終了させる対象が無いだけなので、成功として扱う。
#[cfg(unix)]
fn kill_process_group(pgid: u32) -> std::io::Result<()> {
    let Some(pid) = signalable_process_group(pgid) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("プロセスグループID {pgid} は、シグナルの宛先にできません"),
        ));
    };
    match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(errno) => Err(errno.into()),
    }
}

/// プロセスグループID(`u32`)を、`kill(-pgid, ...)`の宛先として渡してよい`Pid`へ変換する。
/// 0(`Pid`で表せず、`kill(0, ...)`は呼び出し側自身のグループを指す)・1(`kill(-1, ...)`は、
/// 呼び出し側が送れる全プロセスを指す)・`i32`に収まらない値(負のPIDになる)は、
/// 宛先にできないため`None`にする。
#[cfg(unix)]
fn signalable_process_group(pgid: u32) -> Option<rustix::process::Pid> {
    let raw = i32::try_from(pgid).ok()?;
    rustix::process::Pid::from_raw(raw).filter(|pid| !pid.is_init())
}

/// `kill_group`(`child`が率いるプロセスグループ全体を終了させる操作)が失敗した場合に、
/// 直接の子だけでも終了させる。呼び出し側が続けて`child.wait()`するときに、対象コマンドが
/// 自然に終わるまで無期限にブロックしないようにするため。失敗は、標準エラー出力へ残す
/// (MCPの標準出力は、プロトコルのメッセージ専用)。
#[cfg(unix)]
fn kill_group_or_direct_child(
    child: &mut std::process::Child,
    kill_group: impl FnOnce(u32) -> std::io::Result<()>,
) {
    let Err(group_error) = kill_group(child.id()) else {
        return;
    };
    eprintln!(
        "masker-mcp: プロセスグループの強制終了に失敗したため、直接の子プロセスだけを終了します: {group_error}"
    );
    if let Err(error) = child.kill() {
        eprintln!("masker-mcp: 直接の子プロセスの強制終了にも失敗しました: {error}");
    }
}

#[cfg(windows)]
fn taskkill_path() -> Option<std::path::PathBuf> {
    let system_root = std::env::var("SystemRoot").ok()?;
    Some(taskkill_path_from_system_root(&system_root))
}

#[cfg(windows)]
fn taskkill_path_from_system_root(system_root: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(system_root).join("System32").join("taskkill.exe")
}

/// `second_child`(`masker mask --stream`)を終了させる。自身では子プロセスを持たないため、
/// プロセスグループ全体を対象にする特別な処理は不要。
fn kill_second_child(child: &mut std::process::Child) {
    let _ = child.kill();
}

/// `shell_command`をシェル経由で起動し、その標準出力を`second_program`(+`second_args`)の
/// 標準入力へ直接パイプする。`second_program`の標準出力を、`timeout`に達するか両プロセスの
/// 標準出力が終わるまで収集する。出力は`MAX_OUTPUT_BYTES`で上限を設け、タイムアウトとは
/// 無関係に無制限のメモリ増加を防ぐ。
pub(crate) async fn run_piped(
    shell_command: &str,
    second_program: &str,
    second_args: &[String],
    timeout: Duration,
) -> Result<PipedExecutionResult, ExecutorError> {
    run_piped_with_limit(shell_command, second_program, second_args, timeout, MAX_OUTPUT_BYTES).await
}

async fn run_piped_with_limit(
    shell_command: &str,
    second_program: &str,
    second_args: &[String],
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<PipedExecutionResult, ExecutorError> {
    let mut first_cmd = build_shell_command(shell_command);
    first_cmd.stdin(Stdio::null());
    first_cmd.stdout(Stdio::piped());
    let mut first_child = first_cmd.spawn().map_err(ExecutorError::SpawnFirst)?;
    let first_stdout = first_child.stdout.take().expect("stdoutはpipe指定済み");

    let mut second_cmd = Command::new(second_program);
    second_cmd.args(second_args);
    second_cmd.stdin(Stdio::from(first_stdout));
    second_cmd.stdout(Stdio::piped());
    second_cmd.stderr(Stdio::piped());
    let mut second_child = second_cmd.spawn().map_err(|source| ExecutorError::SpawnSecond {
        program: second_program.to_string(),
        source,
    })?;
    let mut second_stdout = second_child.stdout.take().expect("stdoutはpipe指定済み");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    let reader_thread = thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match second_stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = tokio::time::Instant::now() + timeout;
    let mut collected: Vec<u8> = Vec::new();
    let mut timed_out = false;
    let mut truncated = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(chunk)) => {
                let room = max_output_bytes.saturating_sub(collected.len());
                if room == 0 {
                    truncated = true;
                    break;
                }
                let take = room.min(chunk.len());
                collected.extend_from_slice(&chunk[..take]);
                if take < chunk.len() {
                    truncated = true;
                    break;
                }
            }
            Ok(None) => break, // 2段目の標準出力がEOFになり読み取りスレッドが終了した。
            Err(_elapsed) => {
                timed_out = true;
                break;
            }
        }
    }

    // 2段目(masker)の標準出力がEOFになっただけでは、1段目(対象コマンド)がまだ動いている
    // 可能性がある(masker起動直後のエラー終了等、2段目が1段目より先に終わるケース)。
    // ここで無条件に1段目の終了をブロッキングで待つと、1段目が終了しないコマンド
    // (tail -f等)の場合に事実上無期限にハングする。全体のdeadlineまでの残り時間で
    // 1段目の終了を監視し、それでも終わらなければタイムアウト扱いにする。
    if !timed_out && !truncated && !wait_until(&mut first_child, deadline).await {
        timed_out = true;
    }

    let second_stage_error = if timed_out || truncated {
        kill_first_child_tree(&mut first_child);
        kill_second_child(&mut second_child);
        let _ = first_child.wait();
        let _ = second_child.wait();
        let _ = reader_thread.join();
        None
    } else {
        // ここに来る時点で1段目は既にwait_untilで終了確認済みのため、以降のwait()は
        // 実行中のプロセスをブロッキングで待つものではなく、既に終了したプロセスの
        // 後始末(reap)に過ぎない。
        let _ = first_child.wait();
        let status = second_child.wait();
        let _ = reader_thread.join();
        match status {
            Ok(status) if !status.success() => {
                let mut stderr_text = String::new();
                if let Some(mut stderr) = second_child.stderr.take() {
                    let _ = stderr.read_to_string(&mut stderr_text);
                }
                Some(stderr_text)
            }
            _ => None,
        }
    };

    let output = String::from_utf8_lossy(&collected).into_owned();
    Ok(PipedExecutionResult { output, timed_out, truncated, second_stage_error })
}

#[cfg(test)]
mod tests {
    use super::*;

    // sort は改行なしの1行を渡す限りUnix/Windows双方で入力をそのまま1行返すため、
    // 「配管が正しく繋がっているか」を確認する2段目のスタンドインとして使える。
    fn sort_args() -> Vec<String> {
        Vec::new()
    }

    #[tokio::test]
    async fn pipes_first_commands_output_into_second_program() {
        let result = run_piped("echo hello-from-first", "sort", &sort_args(), Duration::from_secs(5))
            .await
            .unwrap();

        assert!(result.output.contains("hello-from-first"), "{:?}", result.output);
        assert!(!result.timed_out);
        assert!(!result.truncated);
        assert!(result.second_stage_error.is_none());
    }

    #[tokio::test]
    async fn does_not_capture_the_first_commands_stderr() {
        // 標準エラー出力は意図的に対象外(設計上のパイプは標準出力のみ)。
        #[cfg(unix)]
        let command = "echo to-stderr 1>&2";
        #[cfg(windows)]
        let command = "echo to-stderr 1>&2";

        let result = run_piped(command, "sort", &sort_args(), Duration::from_secs(5)).await.unwrap();

        assert!(!result.output.contains("to-stderr"), "{:?}", result.output);
    }

    #[tokio::test]
    async fn stops_and_reports_timeout_when_the_first_command_runs_too_long() {
        // Windowsの`timeout /t`はテスト実行環境のPATH解決次第でMSYS版に化けることがあるため、
        // ネイティブに存在するpingの応答待ちをスリープ代わりに使う。
        #[cfg(unix)]
        let command = "sleep 5 && echo should-not-appear";
        #[cfg(windows)]
        let command = "ping -n 6 127.0.0.1 >nul && echo should-not-appear";

        let result = run_piped(command, "sort", &sort_args(), Duration::from_millis(300)).await.unwrap();

        assert!(result.timed_out);
        assert!(!result.output.contains("should-not-appear"));
    }

    #[tokio::test]
    async fn stops_on_timeout_even_when_the_second_stage_exits_immediately_but_the_first_command_keeps_running() {
        // 2段目が1段目より先に終了しても(ここでは存在しないオプションで即エラー終了させる)、
        // 1段目がまだ動いている限りはdeadlineまでタイムアウトとして扱われるはず。
        #[cfg(unix)]
        let command = "sleep 5 && echo should-not-appear";
        #[cfg(windows)]
        let command = "ping -n 6 127.0.0.1 >nul && echo should-not-appear";
        let bad_args = vec!["--not-a-real-option".to_string()];

        let started = tokio::time::Instant::now();
        let result = run_piped(command, "sort", &bad_args, Duration::from_millis(300)).await.unwrap();

        assert!(result.timed_out, "{result:?}");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "1段目の自然終了(5秒)を待たず、指定したタイムアウト(300ms)近くで打ち切られるはず: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn truncates_output_once_it_reaches_the_configured_limit() {
        let result = run_piped_with_limit("echo 0123456789", "sort", &sort_args(), Duration::from_secs(5), 5)
            .await
            .unwrap();

        assert!(result.truncated, "{result:?}");
        assert!(!result.timed_out);
        assert_eq!(result.output.len(), 5, "{result:?}");
    }

    #[tokio::test]
    async fn does_not_truncate_output_within_the_configured_limit() {
        let result = run_piped_with_limit("echo hi", "sort", &sort_args(), Duration::from_secs(5), 1024)
            .await
            .unwrap();

        assert!(!result.truncated, "{result:?}");
        assert!(result.output.contains("hi"));
    }

    #[tokio::test]
    async fn reports_second_stage_error_when_it_exits_non_zero() {
        // "sort"に存在しないオプションを渡し、2段目が自ら失敗するケースを再現する。
        let bad_args = vec!["--not-a-real-option".to_string()];
        let result = run_piped("echo hello", "sort", &bad_args, Duration::from_secs(5)).await.unwrap();

        assert!(result.second_stage_error.is_some(), "{result:?}");
    }

    #[tokio::test]
    async fn fails_clearly_when_the_second_program_does_not_exist() {
        let err = run_piped("echo hello", "no-such-program-xyz", &[], Duration::from_secs(5))
            .await
            .expect_err("存在しないプログラムはエラーになるはず");

        assert!(matches!(err, ExecutorError::SpawnSecond { .. }));
    }

    #[tokio::test]
    async fn preserves_double_quotes_in_the_command_on_any_platform() {
        // Windowsではcmd.exe向けのraw_arg経由、Unixではshの引用符解釈により、
        // ユーザー指定コマンド内の二重引用符が壊れず素通りすることを確認する。
        #[cfg(unix)]
        let command = r#"echo "quoted value""#;
        #[cfg(windows)]
        let command = r#"echo "quoted value""#;

        let result = run_piped(command, "sort", &sort_args(), Duration::from_secs(5)).await.unwrap();

        assert!(result.output.contains("quoted value"), "{:?}", result.output);
    }

    #[tokio::test]
    async fn kills_the_first_commands_child_process_on_timeout_on_windows() {
        #[cfg(not(windows))]
        {
            return;
        }
        #[cfg(windows)]
        {
            // cmd.exeが起動したping.exe(孫プロセス)がタイムアウト後も残っていないことを確認する。
            let result =
                run_piped("ping -n 30 127.0.0.1 >nul", "sort", &sort_args(), Duration::from_millis(300))
                    .await
                    .unwrap();
            assert!(result.timed_out);

            tokio::time::sleep(Duration::from_millis(500)).await;
            let output = Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq PING.EXE", "/FO", "CSV"])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(!text.contains("PING.EXE"), "ping.exeが終了せず残っている: {text}");
        }
    }

    #[test]
    #[cfg(windows)]
    fn taskkill_path_is_built_under_system32_of_the_given_system_root() {
        let path = taskkill_path_from_system_root(r"C:\Windows");
        assert_eq!(path, std::path::PathBuf::from(r"C:\Windows\System32\taskkill.exe"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn kills_the_first_commands_descendant_process_on_timeout_on_unix() {
        // "sleep 1"の後にマーカーファイルを作る子プロセスをshが起動する(&&のため
        // execによる自身の置き換えは効かない)。タイムアウトでshだけkillしてもこの子が
        // 生き残るなら、マーカーファイルは(タイムアウトの再現時間を過ぎても)作られない
        // はずである、という形で検証する(psコマンド等の有無に依存しない)。
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let command = format!("sleep 1 && touch {} && sleep 30", marker.display());

        let result = run_piped(&command, "sort", &sort_args(), Duration::from_millis(300)).await.unwrap();
        assert!(result.timed_out, "{result:?}");

        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            !marker.exists(),
            "タイムアウト後もsleep 1&&touchの子プロセスが生き残り、マーカーファイルが作られた"
        );
    }

    // 対象コマンドが背景で起動した子孫(グループのリーダーではないプロセス)も、タイムアウトで落ちることを
    // 確かめる。リーダー(sh)だけを落とした場合は、この子孫が生き残り、1秒後にマーカーファイルを作る。
    #[tokio::test]
    #[cfg(unix)]
    async fn kills_a_background_descendant_of_the_first_command_on_timeout_on_unix() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let command = format!("(sleep 1 && touch {}) & wait", marker.display());

        let result = run_piped(&command, "sort", &sort_args(), Duration::from_millis(300)).await.unwrap();
        assert!(result.timed_out, "{result:?}");

        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            !marker.exists(),
            "タイムアウト後も、背景で起動した子孫プロセスが生き残り、マーカーファイルが作られた"
        );
    }

    #[cfg(unix)]
    fn wait_for_exit(child: &mut std::process::Child, within: Duration) -> Option<std::process::ExitStatus> {
        let deadline = std::time::Instant::now() + within;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return Some(status);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    // `sleep 30`を、`process_group`のグループの一員として起動する。0なら、新しいグループのリーダーになる。
    #[cfg(unix)]
    fn spawn_sleeper(process_group: i32) -> std::process::Child {
        use std::os::unix::process::CommandExt;
        Command::new("sleep").arg("30").process_group(process_group).spawn().unwrap()
    }

    // 対象のグループ(リーダーと、リーダー以外のメンバー)だけが落ち、別のグループは残ることを確かめる。
    // リーダー以外のメンバーも確かめるのは、リーダー1つだけへの送信(kill(pid))と区別するため。
    #[test]
    #[cfg(unix)]
    fn kill_process_group_kills_every_member_of_the_target_group_and_no_other_group() {
        use std::os::unix::process::ExitStatusExt;

        let mut leader = spawn_sleeper(0);
        let mut member = spawn_sleeper(leader.id() as i32);
        let mut bystander = spawn_sleeper(0);

        let kill_result = kill_process_group(leader.id());

        let leader_exit = wait_for_exit(&mut leader, Duration::from_secs(2));
        let member_exit = wait_for_exit(&mut member, Duration::from_secs(2));
        let bystander_exit = wait_for_exit(&mut bystander, Duration::from_millis(500));
        for child in [&mut leader, &mut member, &mut bystander] {
            let _ = child.kill();
            let _ = child.wait();
        }

        kill_result.expect("プロセスグループへのSIGKILLの送信に失敗した");
        assert_eq!(
            leader_exit.and_then(|status| status.signal()),
            Some(9),
            "グループのリーダーがSIGKILLで落ちなかった"
        );
        assert_eq!(
            member_exit.and_then(|status| status.signal()),
            Some(9),
            "グループのリーダー以外のメンバーがSIGKILLで落ちなかった"
        );
        assert!(bystander_exit.is_none(), "別のグループまで落ちた: {bystander_exit:?}");
    }

    // 既に存在しないグループは、終了させる対象が無いだけで、失敗ではない。i32の最大値は、実在するPIDの
    // 上限(Linuxで約420万)を超えるため、存在しないグループになる。
    #[test]
    #[cfg(unix)]
    fn kill_process_group_treats_a_group_that_does_not_exist_as_success() {
        kill_process_group(i32::MAX as u32).expect("存在しないグループは成功として扱われるはず");
    }

    #[test]
    #[cfg(unix)]
    fn kill_process_group_refuses_ids_that_cannot_address_a_single_group() {
        // 送信を試みる前に断る値だけを渡す(断られなければ、意図しないプロセスへ届きうる値)。
        let refused = kill_process_group(0).expect_err("0は断られるはず");
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
        let refused = kill_process_group(u32::MAX).expect_err("i32に収まらない値は断られるはず");
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
    }

    // 0は自分自身のグループ、1は送る権限のある全プロセス、i32に収まらない値は負のPIDを指してしまう。
    #[test]
    #[cfg(unix)]
    fn signalable_process_group_rejects_ids_that_would_not_address_a_single_group() {
        for pgid in [0, 1, i32::MAX as u32 + 1, u32::MAX] {
            assert_eq!(signalable_process_group(pgid), None, "pgid={pgid}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn signalable_process_group_accepts_ordinary_ids_unchanged() {
        for pgid in [2, 12345, i32::MAX as u32] {
            let pid = signalable_process_group(pgid).unwrap_or_else(|| panic!("pgid={pgid}が断られた"));
            assert_eq!(pid.as_raw_nonzero().get() as u32, pgid);
        }
    }

    #[test]
    #[cfg(unix)]
    fn kill_group_or_direct_child_kills_the_direct_child_when_the_group_kill_fails() {
        use std::os::unix::process::ExitStatusExt;

        let mut child = spawn_sleeper(0);
        let child_id = child.id();
        let mut requested = None;
        kill_group_or_direct_child(&mut child, |pgid| {
            requested = Some(pgid);
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });

        let exit = wait_for_exit(&mut child, Duration::from_secs(2));
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(requested, Some(child_id), "子のPIDがプロセスグループIDとして渡されなかった");
        assert_eq!(
            exit.and_then(|status| status.signal()),
            Some(9),
            "グループのkillが失敗しても、直接の子は落とされるはず"
        );
    }

    #[test]
    #[cfg(unix)]
    fn kill_group_or_direct_child_leaves_the_child_alone_when_the_group_kill_succeeds() {
        let mut child = spawn_sleeper(0);

        kill_group_or_direct_child(&mut child, |_| Ok(()));

        let exit = wait_for_exit(&mut child, Duration::from_millis(500));
        let _ = child.kill();
        let _ = child.wait();

        assert!(exit.is_none(), "グループのkillが成功したのに、直接の子が別に落とされた: {exit:?}");
    }
}
