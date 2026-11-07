//! Dedicated engine worker: no daemon, Tokio runtime or network initialization.
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
    if marmish_native::validate_executable_layout().is_err() {
        eprintln!("marmish: worker_installation");
        return std::process::ExitCode::FAILURE;
    }
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments == [std::ffi::OsString::from("release-info")] {
        return match marmish_native::release_info() {
            Ok(info) => {
                println!("{info}");
                std::process::ExitCode::SUCCESS
            }
            Err(_) => std::process::ExitCode::FAILURE,
        };
    }
    marmish_engine::worker_main(arguments)
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
