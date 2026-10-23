#[path = "../../marmish-engine/tests/support/executable.rs"]
mod executable;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

fn python() -> Vec<u8> {
    let configured = std::env::var_os("MARMISH_TEST_PYTHON").map(std::path::PathBuf::from);
    let executable = configured
        .or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|p| p.join("python3"))
                .find(|p| p.is_file())
        })
        .expect("native tests require Python 3");
    std::fs::canonicalize(executable)
        .unwrap()
        .as_os_str()
        .as_bytes()
        .to_vec()
}

const PRINCIPAL: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ENDPOINT: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const INVOCATION: &str = "33333333333333333333333333333333";

struct Pipe {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    directory: tempfile::TempDir,
    worker: executable::InstalledWorker,
}
impl Pipe {
    fn new() -> Self {
        let directory = executable::private_tempdir();
        let worker = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("marmish-worker");
        assert!(
            worker.is_file(),
            "build reviewed worker first: cargo build -p marmish-engine --bin marmish-worker"
        );
        let worker = executable::stage_worker(&worker);
        let mut child = Command::new(env!("CARGO_BIN_EXE_marmish-engine-bridge"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            directory,
            worker,
        }
    }
    fn restart(&mut self) {
        self.input.take();
        assert!(self.child.wait().unwrap().success());
        let mut child = Command::new(env!("CARGO_BIN_EXE_marmish-engine-bridge"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        self.input = child.stdin.take();
        self.output = BufReader::new(child.stdout.take().unwrap());
        self.child = child;
    }
    fn call(&mut self, mut request: Value) -> Value {
        request["id"] = json!("44444444444444444444444444444444");
        serde_json::to_writer(self.input.as_mut().unwrap(), &request).unwrap();
        self.input.as_mut().unwrap().write_all(b"\n").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], request["id"]);
        response
    }
    fn init(&mut self, shell: bool) {
        self.init_policy(shell, Value::Null);
    }
    fn init_policy(&mut self, shell: bool, policy: Value) {
        let request = json!({"op":"init","version":1,"state_root":self.directory.path().join("state"),"worker_executable":self.worker.path,"server":"5555555555555555555555555555555555555555555555555555555555555555","allowed_principals":[PRINCIPAL],"cwd":self.directory.path(),"allow_shell_command":shell,"command_policy":policy});
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let response = self.call(request.clone());
            if response["ok"] == true {
                assert_eq!(response["result"], json!({"version":1}));
                break;
            }
            assert!(
                Instant::now() < until
                    && matches!(
                        response["error"]["code"].as_str(),
                        Some("busy" | "already_open")
                    ),
                "{response}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn auth(&mut self, mut request: Value) -> Value {
        request["principal"] = json!(PRINCIPAL);
        request["endpoint"] = json!(ENDPOINT);
        let until = Instant::now() + Duration::from_secs(6);
        loop {
            let response = self.call(request.clone());
            if response["error"]["code"] != "busy" {
                return response;
            }
            assert!(Instant::now() < until, "busy deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn token(&mut self) -> Value {
        let result = self.auth(json!({"op":"token"}));
        assert_eq!(result["ok"], true, "{result}");
        result["result"]["token"].clone()
    }
    fn submit(&mut self, invocation: &str, token: &Value, argv: &[&[u8]]) -> Value {
        self.auth(json!({"op":"submit","invocation":invocation,"token":token,"command":{"mode":"exec","argv":argv.iter().map(|a|STANDARD.encode(a)).collect::<Vec<_>>()}}))
    }
    fn finished(&mut self, invocation: &str) -> Value {
        let until = Instant::now() + Duration::from_secs(12);
        loop {
            let result = self.auth(json!({"op":"status","invocation":invocation}));
            assert_eq!(result["ok"], true, "{result}");
            if result["result"]["state"] == "exited" {
                return result["result"].clone();
            }
            assert!(Instant::now() < until, "not exited: {result}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn read(&mut self, invocation: &str, stream: &str) -> Vec<u8> {
        let response=self.auth(json!({"op":"read_output","invocation":invocation,"stream":stream,"cursor":0,"limit":4096}));
        assert_eq!(response["ok"], true, "{response}");
        STANDARD
            .decode(response["result"]["bytes"].as_str().unwrap())
            .unwrap()
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn actual_binary_preserves_argv_and_deduplicates_invocations() {
    let mut pipe = Pipe::new();
    pipe.init(false);
    let token = pipe.token();
    let script=b"import sys;open('once','ab').write(b'x');sys.stdout.buffer.write(b'\\0'.join(x.encode() for x in sys.argv[1:]));sys.stderr.buffer.write(b'err\\0\\xff');sys.exit(7)";
    let special = b"space unicode \xe2\x98\x83 ' \"\n --leading $HOME `touch forbidden`";
    let python = python();
    let args = [python.as_slice(), b"-c", script, special];
    let response = pipe.submit(INVOCATION, &token, &args);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["disposition"], "accepted");
    let completed = pipe.finished(INVOCATION);
    assert_eq!(completed["outcome"], json!({"kind":"code","value":7}));
    assert_eq!(pipe.read(INVOCATION, "stdout"), special);
    assert_eq!(pipe.read(INVOCATION, "stderr"), b"err\0\xff");
    assert!(!pipe.directory.path().join("forbidden").exists());
    let repeat = pipe.submit(INVOCATION, &token, &args);
    assert_eq!(repeat["result"]["disposition"], "existing", "{repeat}");
    let conflict = pipe.submit(INVOCATION, &token, &[b"/bin/true"]);
    assert_eq!(conflict["error"]["code"], "invocation_conflict");
    assert_eq!(
        std::fs::read(pipe.directory.path().join("once")).unwrap(),
        b"x"
    );
    let wrong=pipe.call(json!({"op":"status","principal":"6666666666666666666666666666666666666666666666666666666666666666","endpoint":ENDPOINT,"invocation":INVOCATION}));
    assert_eq!(wrong["error"]["code"], "unauthorized");
    let cross_group=pipe.call(json!({"op":"status","principal":PRINCIPAL,"endpoint":"7777777777777777777777777777777777777777777777777777777777777777","invocation":INVOCATION}));
    assert_eq!(cross_group["ok"], true);
}

#[test]
fn actual_binary_handles_binary_stdin_eof_and_bounds() {
    let mut pipe = Pipe::new();
    pipe.init(false);
    let token = pipe.token();
    let python = python();
    let response=pipe.submit(INVOCATION,&token,&[python.as_slice(),b"-c",b"import sys;d=sys.stdin.buffer.read();sys.stdout.buffer.write(d);sys.stderr.buffer.write(d[::-1])"]);
    assert_eq!(response["ok"], true, "{response}");
    let begin = pipe.auth(json!({"op":"begin_input","invocation":INVOCATION}));
    assert_eq!(begin["ok"], true, "{begin}");
    let generation = begin["result"]["generation"].clone();
    let bytes: Vec<u8> = (0..=255).cycle().take(4096).collect();
    let write = json!({"op":"write_stdin","invocation":INVOCATION,"generation":generation,"offset":0,"bytes":STANDARD.encode(&bytes),"fresh_ms":60000});
    let receipt = pipe.auth(write.clone());
    assert_eq!(receipt["result"]["accepted"], 4096, "{receipt}");
    let repeat = pipe.auth(write.clone());
    assert_eq!(repeat["result"]["duplicate"], true, "{repeat}");
    let mut too_big = write;
    too_big["bytes"] = json!(STANDARD.encode(vec![0; 4097]));
    assert_eq!(pipe.auth(too_big)["error"]["code"], "invalid_request");
    let eof=pipe.auth(json!({"op":"stdin_eof","invocation":INVOCATION,"generation":generation,"final_offset":4096}));
    assert_eq!(eof["ok"], true, "{eof}");
    pipe.finished(INVOCATION);
    assert_eq!(pipe.read(INVOCATION, "stdout"), bytes);
    assert_eq!(
        pipe.read(INVOCATION, "stderr"),
        bytes.iter().rev().copied().collect::<Vec<_>>()
    );
    assert_eq!(pipe.auth(json!({"op":"read_output","invocation":INVOCATION,"stream":"stdout","cursor":0,"limit":4097}))["error"]["code"],"invalid_request");
}

#[test]
fn explicit_shell_mode_requires_policy() {
    for allowed in [false, true] {
        let mut pipe = Pipe::new();
        pipe.init(allowed);
        let token = pipe.token();
        let response=pipe.auth(json!({"op":"submit","invocation":INVOCATION,"token":token,"command":{"mode":"shell","command":STANDARD.encode(b"printf shell-mode")}}));
        if allowed {
            assert_eq!(response["ok"], true, "{response}");
            pipe.finished(INVOCATION);
            assert_eq!(pipe.read(INVOCATION, "stdout"), b"shell-mode");
        } else {
            assert_eq!(response["error"]["code"], "invalid_request");
        }
    }
}

#[test]
fn framing_is_bounded_and_diagnostics_are_sanitized() {
    for bytes in [
        b"{secret-malformed}\n".to_vec(),
        vec![b'x'; marmish_prototype::MAX_LINE + 1],
        b"{\"secret\":\"unfinished\"}".to_vec(),
    ] {
        let mut output = Vec::new();
        marmish_prototype::serve(std::io::Cursor::new(bytes), &mut output).unwrap();
        assert!(output.len() < 150);
        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(response["ok"], false);
        assert!(!String::from_utf8(output).unwrap().contains("secret"));
    }
    let input=b"{\"id\":\"44444444444444444444444444444444\",\"op\":\"token\",\"principal\":\"bad\",\"endpoint\":\"bad\",\"unexpected\":1}\n";
    let mut output = Vec::new();
    marmish_prototype::serve(std::io::Cursor::new(input), &mut output).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap()["error"]["code"],
        "invalid_request"
    );
}

#[test]
fn detached_worker_survives_frontend_restart_and_cancel_is_queryable() {
    let mut pipe = Pipe::new();
    pipe.init(false);
    let token = pipe.token();
    let python = python();
    let response = pipe.submit(
        INVOCATION,
        &token,
        &[
            python.as_slice(),
            b"-c",
            b"import time,sys;time.sleep(1);sys.stdout.write('survived')",
        ],
    );
    assert_eq!(response["ok"], true, "{response}");
    pipe.restart();
    pipe.init(false);
    let completed = pipe.finished(INVOCATION);
    assert_eq!(completed["outcome"], json!({"kind":"code","value":0}));
    assert_eq!(pipe.read(INVOCATION, "stdout"), b"survived");
    // A fast completed command closes stdin before a relay-roundtrip caller can
    // begin input. Preserve this error so nonempty input cannot be silently lost.
    assert_eq!(
        pipe.auth(json!({"op":"begin_input","invocation":INVOCATION}))["error"]["code"],
        "input_closed"
    );
    let next = "88888888888888888888888888888888";
    let token = pipe.token();
    let response = pipe.submit(next, &token, &[b"/bin/sleep", b"30"]);
    assert_eq!(response["ok"], true, "{response}");
    let cancel = pipe.auth(json!({"op":"cancel","invocation":next}));
    assert_eq!(cancel["result"]["cancel_requested"], true, "{cancel}");
    let completed = pipe.finished(next);
    assert_eq!(completed["outcome"]["kind"], "signal");
    let maintenance = pipe.call(json!({"op":"maintain","limit":128}));
    assert_eq!(maintenance["ok"], true, "{maintenance}");
    assert!(maintenance["result"]["records_removed"].is_u64());
    assert_eq!(
        pipe.call(json!({"op":"maintain","limit":0}))["error"]["code"],
        "invalid_request"
    );
}

#[test]
fn policy_bridge_enforces_cumulative_layers_without_consuming_token() {
    let mut pipe = Pipe::new();
    let exe = std::fs::canonicalize("/usr/bin/printf").unwrap();
    let allowed_exe = std::fs::canonicalize("/bin/echo").unwrap();
    pipe.init_policy(
        true,
        json!({
            "version": 1,
            "defaults": {"allow_exec": ["/**"], "deny_exec": [exe]},
            "keys": [
                {"principal": PRINCIPAL, "restrictions": {"allow_exec": [exe, allowed_exe]}},
                {"principal": PRINCIPAL, "restrictions": {"allow_exec": ["/**"]}}
            ]
        }),
    );
    let token = pipe.token();
    let denied = pipe.submit(
        INVOCATION,
        &token,
        &[exe.as_os_str().as_bytes(), b"forbidden"],
    );
    assert_eq!(denied["error"]["code"], "command_denied", "{denied}");
    assert_eq!(
        pipe.auth(json!({"op":"status","invocation":INVOCATION}))["error"]["code"],
        "not_found"
    );
    let denied = pipe.auth(json!({"op":"submit","invocation":INVOCATION,"token":token,"command":{"mode":"shell","command":STANDARD.encode(b"printf forbidden")}}));
    assert_eq!(denied["error"]["code"], "command_denied", "{denied}");
    // Denial did not create a job or consume the launch token. An allowed
    // real subprocess now succeeds with that same token and invocation ID.
    let accepted = pipe.submit(
        INVOCATION,
        &token,
        &[allowed_exe.as_os_str().as_bytes(), b"allowed"],
    );
    assert_eq!(accepted["ok"], true, "{accepted}");
    pipe.finished(INVOCATION);
    assert_eq!(pipe.read(INVOCATION, "stdout"), b"allowed\n");
}

#[test]
fn cleanup_fences_state_and_disables_normal_bridge_operations() {
    let mut pipe = Pipe::new();
    pipe.init(false);
    let response = pipe.call(json!({"op":"cleanup_ready"}));
    assert_eq!(response["result"], json!({"ready":true}), "{response}");
    assert_eq!(
        pipe.call(json!({"op":"cleanup_ready"}))["result"],
        json!({"ready":true})
    );
    assert_eq!(
        pipe.auth(json!({"op":"token"}))["error"]["code"],
        "invalid_request"
    );
    std::fs::remove_dir_all(pipe.directory.path().join("state")).unwrap();
    assert!(!pipe.directory.path().join("state").exists());
    pipe.restart();
    pipe.init(false);
    assert_eq!(pipe.auth(json!({"op":"token"}))["ok"], true);
}
