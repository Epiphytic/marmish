//! Actual finite matrix inbox loops with a deterministic hard-link publication interval.
use super::*;
use std::{
    future::Future,
    task::{Context as TaskContext, Poll, Waker},
};

#[tokio::test(start_paused = true)]
async fn published_matrix_rendezvous_waits_then_accepts_unchanged_validators() {
    for clients_loop in [false, true] {
        let (_root, context, input, output) = tests::rollup_fixture();
        let ctx: Context = records::read(&context, 65536).unwrap();
        let ids = records::matrix_identities(&input, &ctx).unwrap();
        let mut ready = BTreeMap::new();
        for role in ROLES {
            let record =
                records::read(&input.join(format!("{role}-server-ready.json")), 65536).unwrap();
            ready.insert(role.into(), record);
        }
        let options = Run {
            context,
            workspace: input.clone(),
            identities: input.clone(),
            inbox: input.clone(),
            output,
        };
        let kind = if clients_loop {
            "client-result"
        } else {
            "server-ready"
        };
        let path = input.join(format!("{}-{kind}.json", ROLES[0]));
        let staging = input.join("publication-staging");
        fs::hard_link(&path, &staging).unwrap();
        let until = Instant::now() + Duration::from_secs(1);
        let mut work = std::pin::pin!(async {
            if clients_loop {
                wait_clients(&options, &ctx, &ids, &ready, until)
                    .await
                    .map(|all| all.len())
            } else {
                wait_servers(&options, &ctx, &ids, until)
                    .await
                    .map(|all| all.len())
            }
        });
        assert!(matches!(
            work.as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop())),
            Poll::Pending
        ));
        fs::remove_file(staging).unwrap();
        assert_eq!(work.await.unwrap(), 3);
    }
}

#[tokio::test(start_paused = true)]
async fn published_matrix_rendezvous_bounds_pending_and_rejects_completed_malformed_records() {
    for clients_loop in [false, true] {
        for finish_publication in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let ctx = records::tests::ctx();
            let options = Run {
                context: root.path().join("ctx"),
                workspace: root.path().into(),
                identities: root.path().into(),
                inbox: root.path().into(),
                output: root.path().into(),
            };
            let kind = if clients_loop {
                "client-result"
            } else {
                "server-ready"
            };
            let path = root.path().join(format!("{}-{kind}.json", ROLES[0]));
            let staging = root.path().join("staging");
            fs::write(&path, b"not-json").unwrap();
            fs::hard_link(&path, &staging).unwrap();
            let ids = BTreeMap::new();
            let ready = BTreeMap::new();
            let until = Instant::now() + Duration::from_millis(250);
            let mut work = std::pin::pin!(async {
                if clients_loop {
                    wait_clients(&options, &ctx, &ids, &ready, until)
                        .await
                        .map(|_| ())
                } else {
                    wait_servers(&options, &ctx, &ids, until).await.map(|_| ())
                }
            });
            assert!(matches!(
                work.as_mut()
                    .poll(&mut TaskContext::from_waker(Waker::noop())),
                Poll::Pending
            ));
            if finish_publication {
                fs::remove_file(staging).unwrap();
            }
            let error = work.await.unwrap_err();
            if finish_publication {
                assert!(matches!(error, Error::Invalid(_)));
            } else {
                assert!(matches!(error, Error::Timeout));
                assert!(Instant::now() >= until);
            }
        }
    }
}
