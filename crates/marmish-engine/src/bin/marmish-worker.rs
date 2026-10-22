fn main() -> std::process::ExitCode {
    marmish_engine::worker_main(std::env::args_os().skip(1).collect())
}
