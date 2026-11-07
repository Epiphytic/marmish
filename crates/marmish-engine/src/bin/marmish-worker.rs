fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("marmish worker failed")));
    if marmish_engine::initialize_process_limits().is_err() {
        eprintln!("marmish worker: process_limits");
        return std::process::ExitCode::FAILURE;
    }
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return std::process::ExitCode::FAILURE;
    }
    marmish_engine::worker_main(std::env::args_os().skip(1).collect())
}
