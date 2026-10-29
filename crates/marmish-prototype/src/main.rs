use std::os::fd::FromRawFd;

fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("marmish bridge failed")));
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return std::process::ExitCode::FAILURE;
    }
    if std::env::args_os().len() != 1 {
        return std::process::ExitCode::FAILURE;
    }
    // Own the unbuffered descriptor; std Stdin retains a global buffer.
    let input = unsafe { std::fs::File::from_raw_fd(0) };
    match marmish_prototype::serve(input, std::io::stdout().lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}
