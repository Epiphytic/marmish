//! Dedicated engine worker: no daemon, Tokio runtime or network initialization.
fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("marmish worker failed")));
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return std::process::ExitCode::FAILURE;
    }
    marmish_engine::worker_main(std::env::args_os().skip(1).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_or_malformed_handoff_is_refused() {
        for arguments in [vec![], vec!["/unused".into(), "not-a-job-id".into()]] {
            assert_eq!(
                marmish_engine::worker_main(arguments),
                std::process::ExitCode::FAILURE
            );
        }
    }
}
