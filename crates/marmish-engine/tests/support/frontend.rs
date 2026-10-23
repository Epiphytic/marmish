//! Feature-only public-API frontend fixture. Never built into default releases.
use marmish_engine::{
    CommandRequest, EndpointId, Engine, EngineError, EngineLimits, EnginePolicy, ExecuteToken,
    InvocationId, InvocationState, PrincipalId, ServerId, Stream,
};
use std::{
    fs,
    io::{self, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

const ID: InvocationId = InvocationId([7; 16]);

fn retry<T>(mut operation: impl FnMut() -> marmish_engine::Result<T>) -> Result<T, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match operation() {
            Err(EngineError::Busy) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            result => return result.map_err(|error| error.to_string()),
        }
    }
}

fn fixture(root: &Path, scenario: &str) -> Result<CommandRequest, String> {
    let script = match scenario {
        "quick" | "expiry" => {
            "printf 'run\\n' >> \"$1\"; printf stdout; printf stderr >&2; exit 17"
        }
        "input" => "printf 'run\\n' >> \"$1\"; exec /bin/cat",
        "output" => {
            "printf 'run\\n' >> \"$1\"; /usr/bin/head -c 131072 /dev/zero; printf stderr >&2"
        }
        "slow" => "printf 'run\\n' >> \"$1\"; /bin/sleep 2; printf survived",
        _ => return Err("unknown fixed fixture".into()),
    };
    Ok(CommandRequest::Exec {
        argv: vec![
            b"/bin/sh".to_vec(),
            b"-c".to_vec(),
            script.as_bytes().to_vec(),
            b"marmish-test-fixture".to_vec(),
            root.join("executions").as_os_str().as_bytes().to_vec(),
        ],
    })
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("expected state-root worker action fixture".into());
    }
    let root = PathBuf::from(&args[0]);
    let worker = PathBuf::from(&args[1]);
    let action = args[2].to_str().ok_or("invalid action")?;
    let scenario = args[3].to_str().ok_or("invalid fixture")?;
    let request = fixture(&root, scenario)?;
    let uid = fs::metadata(&root)
        .map_err(|_| "fixture root missing")?
        .uid();
    let policy = EnginePolicy {
        state_root: root.clone(),
        worker_executable: worker,
        server: ServerId([3; 32]),
        allowed_principals: vec![PrincipalId([1; 32])],
        allow_shell_command: false,
        command_policy: None,
        cwd: Some(root.clone()),
        limits: EngineLimits {
            cancel_grace: Duration::from_millis(100),
            drain_grace: Duration::from_millis(100),
            output_retention: marmish_core::Retention::from_seconds(Some(
                if scenario == "expiry" { 1 } else { 86400 },
            ))
            .map_err(|_| "invalid fixed retention")?,
            ..EngineLimits::default()
        },
    };
    let mut engine = Engine::open(policy).map_err(|e| e.to_string())?;
    if action == "initialize" {
        println!("INITIALIZED");
        return Ok(());
    }
    if action == "maintain" {
        retry(|| engine.maintain(32))?;
        println!("MAINTAINED");
        return Ok(());
    }
    let ctx = engine
        .authorize(
            PrincipalId([1; 32]),
            EndpointId([2; 32]),
            ServerId([3; 32]),
            uid,
        )
        .map_err(|e| e.to_string())?;
    if matches!(action, "submit" | "hold" | "retry" | "input") {
        let token = if action == "retry" {
            let bytes =
                fs::read(root.join("fixture-token")).map_err(|_| "missing fixture token")?;
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| "bad fixture token")?;
            ExecuteToken::from_bytes(bytes)
        } else {
            let token = engine
                .issue_execute_token(&ctx)
                .map_err(|e| e.to_string())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join("fixture-token"))
                .map_err(|_| "cannot save fixture token")?;
            file.write_all(&token.expose_bytes())
                .map_err(|_| "cannot save fixture token")?;
            token
        };
        let receipt = retry(|| engine.submit(&ctx, &token, ID, &request))?;
        println!("SUBMISSION:{:?}", receipt.disposition);
    } else if !matches!(action, "status" | "feed" | "cancel") {
        return Err("unknown fixed action".into());
    }
    println!("READY");
    io::stdout().flush().map_err(|_| "stdout closed")?;
    if action == "hold" {
        thread::sleep(Duration::from_secs(5));
    }
    if action == "cancel" {
        retry(|| engine.cancel(&ctx, ID))?;
    }
    if matches!(action, "input" | "feed") {
        let input = retry(|| engine.begin_input(&ctx, ID))?;
        let generation = input.generation.ok_or("no generation")?;
        retry(|| {
            engine.write_stdin(
                &ctx,
                ID,
                generation,
                0,
                b"fixed-input",
                Duration::from_secs(5),
            )
        })?;
        retry(|| engine.stdin_eof(&ctx, ID, generation, 11))?;
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let status = match engine.status(&ctx, ID) {
            Ok(status) => status,
            Err(EngineError::Busy) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        if matches!(
            status.state,
            InvocationState::Exited | InvocationState::Lost | InvocationState::UnknownLaunch
        ) {
            println!("STATE:{:?}", status.state);
            println!("OUTCOME:{:?}", status.outcome);
            println!("INPUT:{:?}", status.input.health);
            for (name, stream) in [("STDOUT", Stream::Stdout), ("STDERR", Stream::Stderr)] {
                let output = loop {
                    match engine.read_output(&ctx, ID, stream, 0, 16384) {
                        Ok(output) => break output,
                        Err(EngineError::Busy) if Instant::now() < deadline => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(error) => return Err(error.to_string()),
                    }
                };
                println!("{name}_BYTES:{}", output.bytes.len());
                println!("{name}_RETAINED:{}", output.status.retained);
                println!("{name}_PRODUCED:{}", output.status.produced);
                println!("{name}_CLOSED:{}", output.status.closed);
                println!("{name}_GAPS:{:?}", output.status.gaps);
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("fixture deadline exceeded".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fixture: {message}");
            ExitCode::FAILURE
        }
    }
}
