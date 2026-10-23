//! Native process-interface inspection for confidentiality/descriptor tests.
use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub fn interfaces(pid: u32) -> Vec<u8> {
    let mut result = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    result.extend(std::fs::read(format!("/proc/{pid}/environ")).unwrap());
    result
}
#[cfg(target_os = "macos")]
pub fn interfaces(pid: u32) -> Vec<u8> {
    // Includes executable, argv and environment; no parsing is needed to scan
    // the complete byte interface for both raw and hexadecimal key canaries.
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut bytes = vec![0u8; 1_048_576];
    let mut length = bytes.len();
    assert_eq!(
        unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                bytes.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        },
        0
    );
    assert!(length > 4 && length <= bytes.len());
    bytes.truncate(length);
    bytes
}
#[cfg(target_os = "linux")]
pub fn descriptors(pid: u32) -> Vec<PathBuf> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|entry| match std::fs::read_link(entry.unwrap().path()) {
            Ok(path) => Some(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("descriptor inspection failed: {error}"),
        })
        .collect()
}
#[cfg(target_os = "macos")]
pub fn descriptors(pid: u32) -> Vec<PathBuf> {
    // lsof uses Darwin's native libproc inspection. Restrict output to numeric
    // descriptors, excluding cwd/text records, and never print its path data.
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "0-65535", "-Fn"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "native descriptor inspector failed"
    );
    use std::os::unix::ffi::OsStringExt;
    output
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|line| line.strip_prefix(b"n"))
        .map(|name| PathBuf::from(std::ffi::OsString::from_vec(name.to_vec())))
        .collect()
}
