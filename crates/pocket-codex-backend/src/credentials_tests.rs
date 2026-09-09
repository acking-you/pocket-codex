use super::*;

fn cached() -> Cached {
    Cached {
        credential: "cached-test-credential".into(),
        key_id: 7,
        expires_at: now_secs() + 7200,
    }
}

#[tokio::test]
async fn a_busy_account_does_not_block_other_cached_accounts() {
    let vendor = Credentials::new(RelaySession::for_test("127.0.0.1:1"));
    let busy = vendor.account("busy").await;
    let _busy = busy.lock().await;
    *vendor.account("ready").await.lock().await = Some(cached());
    let (credential, _) =
        tokio::time::timeout(Duration::from_millis(100), vendor.for_account("ready"))
            .await
            .expect("unrelated account must not wait")
            .expect("cached credential");
    assert_eq!(credential, "cached-test-credential");
}

#[tokio::test]
async fn renewal_transport_failure_preserves_the_cached_namespace() {
    let vendor = Credentials::new(RelaySession::for_test("127.0.0.1:1"));
    let mut entry = cached();
    entry.expires_at = now_secs() + 60;
    *vendor.account("user").await.lock().await = Some(entry);
    assert!(vendor.for_account("user").await.is_err());
    assert_eq!(vendor.namespace_of("user").await.expect("cached namespace"), Some(7));
}

#[tokio::test]
async fn a_lost_renewal_reply_never_mints_a_second_namespace() {
    use tokio::net::{TcpListener, TcpStream};
    let (Ok(addr), Ok(key)) =
        (std::env::var("PCX_TEST_RELAY"), std::env::var("PCX_TEST_RELAY_KEY"))
    else {
        eprintln!("skipping lost-reply test: PCX_TEST_RELAY / PCX_TEST_RELAY_KEY unset");
        return;
    };
    let relay = RelaySession::new(addr.clone(), key.clone());
    let issued = pocket_codex_pb::issue_credential(
        &relay,
        Duration::from_secs(7200),
        Some("lost-renewal-test".into()),
    )
    .await
    .expect("issue fixture");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("proxy");
    let vendor = Credentials::new(RelaySession::new(
        listener.local_addr().expect("address").to_string(),
        key,
    ));
    *vendor.account("user").await.lock().await = Some(Cached {
        credential: issued.credential,
        key_id: issued.key_id,
        expires_at: now_secs() + 60,
    });
    let proxy = tokio::spawn(async move {
        // Lose the renewal connection, then allow the authoritative key listing.
        drop(listener.accept().await.expect("renewal").0);
        let (mut downstream, _) = listener.accept().await.expect("key listing");
        let mut upstream = TcpStream::connect(addr).await.expect("relay");
        tokio::io::copy_bidirectional(&mut downstream, &mut upstream)
            .await
            .expect("forward listing");
        listener
    });
    assert!(vendor.for_account("user").await.is_err());
    let listener = proxy.await.expect("proxy task");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "must not mint after an ambiguous failure"
    );
    assert_eq!(vendor.namespace_of("user").await.expect("namespace"), Some(issued.key_id));
    pocket_codex_pb::revoke_credential(&relay, issued.key_id)
        .await
        .expect("cleanup");
}
