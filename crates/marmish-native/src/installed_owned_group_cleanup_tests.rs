//! Deterministic cleanup policy tests; injected EPERM is not native XNU evidence.
use super::*;
use std::{cell::RefCell, os::unix::process::ExitStatusExt};

async fn ordered(
    kill_errno: Option<i32>,
    wait_result: Result<ExitStatus>,
    probe_errno: Option<i32>,
) -> (Result<ExitStatus>, Vec<&'static str>) {
    let calls = RefCell::new(Vec::new());
    let result = cleanup_owned_group(
        || {
            calls.borrow_mut().push("kill(SIGKILL)");
            kill_errno.map_or(Ok(()), |errno| {
                Err(std::io::Error::from_raw_os_error(errno))
            })
        },
        || async {
            calls.borrow_mut().push("wait");
            wait_result
        },
        || {
            calls.borrow_mut().push("probe(0)");
            probe_errno.map_or(Ok(()), |errno| {
                Err(std::io::Error::from_raw_os_error(errno))
            })
        },
    )
    .await;
    (result, calls.into_inner())
}
fn natural_exit() -> Result<ExitStatus> {
    Ok(ExitStatus::from_raw(23 << 8))
}

#[tokio::test]
async fn denied_cleanup_requires_wait_then_fresh_absence_and_preserves_status() {
    let (result, calls) = ordered(Some(libc::EPERM), natural_exit(), Some(libc::ESRCH)).await;
    assert_eq!(result.unwrap().code(), Some(23));
    assert_eq!(calls, ["kill(SIGKILL)", "wait", "probe(0)"]);
}

#[tokio::test]
async fn normal_kill_and_missing_group_wait_without_a_probe() {
    for errno in [None, Some(libc::ESRCH)] {
        let (result, calls) = ordered(errno, natural_exit(), Some(libc::EINVAL)).await;
        assert_eq!(result.unwrap().code(), Some(23));
        assert_eq!(calls, ["kill(SIGKILL)", "wait"]);
    }
}

#[tokio::test]
async fn denied_cleanup_refuses_present_denied_and_unknown_groups() {
    for errno in [None, Some(libc::EPERM), Some(libc::EINVAL)] {
        let (result, calls) = ordered(Some(libc::EPERM), natural_exit(), errno).await;
        match errno {
            None => assert!(matches!(
                result,
                Err(Error::Invalid("installed_owned_group_present"))
            )),
            Some(expected) => assert!(matches!(
                result, Err(Error::Io(error)) if error.raw_os_error() == Some(expected)
            )),
        }
        assert_eq!(calls, ["kill(SIGKILL)", "wait", "probe(0)"]);
    }
}

#[tokio::test]
async fn wait_failure_preserves_category_and_never_probes() {
    for initial in [None, Some(libc::ESRCH), Some(libc::EPERM)] {
        for timeout in [false, true] {
            let wait = if timeout {
                Err(Error::Timeout)
            } else {
                Err(std::io::Error::from_raw_os_error(libc::ECHILD).into())
            };
            let (result, calls) = ordered(initial, wait, Some(libc::ESRCH)).await;
            if timeout {
                assert!(matches!(result, Err(Error::Timeout)));
            } else {
                assert!(matches!(
                    result, Err(Error::Io(error)) if error.raw_os_error() == Some(libc::ECHILD)
                ));
            }
            assert_eq!(calls, ["kill(SIGKILL)", "wait"]);
        }
    }
}

#[tokio::test]
async fn unrelated_kill_error_never_waits_or_probes() {
    let (result, calls) = ordered(Some(libc::EINVAL), natural_exit(), Some(libc::ESRCH)).await;
    assert!(matches!(
        result, Err(Error::Io(error)) if error.raw_os_error() == Some(libc::EINVAL)
    ));
    assert_eq!(calls, ["kill(SIGKILL)"]);
}

#[tokio::test]
async fn live_child_cleanup_kills_then_reaps_the_owned_leader() {
    let root = crate::journal::private_test_directory();
    let environment = process::Environment {
        home: root.path().into(),
        temporary: root.path().into(),
        utilities: root.path().into(),
    };
    let mut child = process::command(
        Path::new("/bin/sleep"),
        &[OsString::from("30")],
        root.path(),
        &environment,
    )
    .spawn()
    .unwrap();
    let status = reap_owned_group(&mut child).await.unwrap();
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert!(child.id().is_none());
    assert!(reap_owned_group(&mut child).await.is_err());
}
