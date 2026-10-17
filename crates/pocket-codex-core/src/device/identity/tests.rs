use super::*;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("pcx-identity-{}", Uuid::new_v4())))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn metadata(os: &str, environment: Option<&str>, source: Option<&str>) -> DeviceMetadata {
    DeviceMetadata {
        os: os.into(),
        environment: environment.map(Into::into),
        source_name: source.map(Into::into),
        hostname: "same computer".into(),
        ..DeviceMetadata::current()
    }
}

#[test]
fn same_machine_environments_and_installations_register_independently() {
    let root = Root::new();
    let windows =
        HostIdentity::load_or_create(&root.0, &metadata("windows", None, None)).expect("Windows");
    let wsl =
        HostIdentity::load_or_create(&root.0, &metadata("linux", Some("wsl"), Some("Ubuntu")))
            .expect("WSL");
    let linux = HostIdentity::load_or_create(&root.0, &metadata("linux", None, Some("Debian")))
        .expect("Linux");
    let another = Root::new();
    let ubuntu2 =
        HostIdentity::load_or_create(&another.0, &metadata("linux", Some("wsl"), Some("Ubuntu")))
            .expect("second WSL installation");
    let entries = [&windows, &wsl, &linux, &ubuntu2];
    assert_eq!(
        entries
            .iter()
            .map(|identity| identity.id())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        entries
            .iter()
            .map(|identity| identity.device())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    for identity in entries {
        let key = crate::service::ServiceId::new(
            identity.device(),
            crate::service::ServiceKind::Host,
            "pocket",
        )
        .key();
        assert_eq!(
            crate::service::ServiceId::parse_key(&key)
                .expect("key")
                .device,
            identity.device()
        );
    }
}

#[test]
fn identity_and_registration_survive_restart_rename_and_os_upgrade() {
    let root = Root::new();
    let first = HostIdentity::load_or_create(
        &root.0,
        &metadata("linux", Some("wsl"), Some("Ubuntu 24.04")),
    )
    .expect("first start");
    let mut current = metadata("linux", Some("wsl"), Some("Ubuntu 26.04"));
    current.hostname = "renamed computer".into();
    current.instance_id = Uuid::new_v4().to_string();
    let next = HostIdentity::load_or_create(&root.0, &current).expect("restart");
    assert_eq!(first, next);
}

#[test]
fn concurrent_first_starts_share_the_durable_identity() {
    let root = Root::new();
    let results = std::thread::scope(|scope| {
        let mut pending = Vec::new();
        for _ in 0..8 {
            let path = &root.0;
            pending.push(scope.spawn(move || {
                HostIdentity::load_or_create(path, &metadata("linux", Some("wsl"), Some("Ubuntu")))
                    .expect("identity")
            }));
        }
        pending
            .into_iter()
            .map(|job| job.join().expect("thread"))
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|entry| entry == &results[0]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&root.0).expect("dir").permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(root.0.join("linux-wsl.json"))
                .expect("file")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn corrupt_or_future_identity_fails_without_creating_a_replacement() {
    let root = Root::new();
    fs::create_dir_all(&root.0).expect("root");
    let path = root.0.join("windows-native.json");
    for bytes in [
        "{broken".into(),
        serde_json::json!({"version":2,"id":Uuid::new_v4().to_string(),"device":"valid"})
            .to_string(),
    ] {
        fs::write(&path, &bytes).expect("fixture");
        assert!(HostIdentity::load_or_create(&root.0, &metadata("windows", None, None)).is_err());
        assert_eq!(fs::read_to_string(&path).expect("preserved"), bytes);
    }
}
