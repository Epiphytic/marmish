use std::process::ExitCode;

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
fn main() -> ExitCode {
    marmish_native::registered_qualification_entry()
}

#[cfg(not(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
)))]
fn main() -> ExitCode {
    eprintln!("marmish qualification: unavailable");
    ExitCode::FAILURE
}
