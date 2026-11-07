fn main() -> std::process::ExitCode {
    if marmish_engine::initialize_process_limits().is_err() {
        eprintln!("marmish: process_limits");
        return std::process::ExitCode::FAILURE;
    }
    marmish_native::cli::main(true)
}
