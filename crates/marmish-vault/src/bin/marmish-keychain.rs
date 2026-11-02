fn main() -> std::process::ExitCode {
    if std::env::args_os().len() != 1 {
        use std::io::Write;
        let _ = std::io::stdout()
            .lock()
            .write_all(b"{\"version\":1,\"ok\":false,\"error\":\"malformed\"}\n");
        return std::process::ExitCode::FAILURE;
    }
    marmish_vault::helper::run()
}
