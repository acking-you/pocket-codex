//! ACP installer tests (TRD §8.2 "安装器"). Downloads come from a local HTTP
//! server (`allow_loopback_http`), npm is a recording fake, and validation
//! talks to the in-process FakeAgent; nothing touches the real state
//! directory, network, npm or agents.

#![cfg(not(any(target_os = "android", target_os = "ios")))]

use std::{
    collections::{BTreeMap, HashMap},
    io::Write as _,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use pocket_codex_core::acp::{
    pcx::{AcpSettingsView, CustomAgentDef, GatewayView, JobProgress},
    AuthMethod,
};
use pocket_codex_host_svc::acp::{
    install::{
        self, archive,
        audit::{self, AuditEntry},
        fetch::{self, FetchPolicy},
        jobs,
        npm::{NpmArgs, NpmRunner},
        parse_catalog,
        platform::{self, detect_from, Platform},
        resolve::archive_target,
        settings::check_gateway_url,
        store::{read_installed, Layout},
        InstallContext,
    },
    terminal_launch,
    testing::{DuplexConnector, FakeAgent, FakeScript},
    AcpError, AgentConnector, AgentIo, LaunchSpec,
};
use serde_json::{json, Value};
use tempfile::TempDir;

// ------------------------------------------------------------ test server

#[derive(Clone, Default)]
struct Files(Arc<Mutex<HashMap<String, Served>>>);

#[derive(Clone)]
enum Served {
    Bytes(Vec<u8>),
    Redirect(String),
}

impl Files {
    fn put(&self, path: &str, bytes: Vec<u8>) {
        self.0
            .lock()
            .expect("files")
            .insert(path.into(), Served::Bytes(bytes));
    }

    fn redirect(&self, path: &str, to: &str) {
        self.0
            .lock()
            .expect("files")
            .insert(path.into(), Served::Redirect(to.into()));
    }
}

async fn serve_file(State(files): State<Files>, uri: Uri) -> Response {
    let served = files.0.lock().expect("files").get(uri.path()).cloned();
    match served {
        Some(Served::Bytes(bytes)) => bytes.into_response(),
        Some(Served::Redirect(to)) => (StatusCode::FOUND, [(header::LOCATION, to)]).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn server() -> (Files, SocketAddr) {
    let files = Files::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = Router::new().fallback(serve_file).with_state(files.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (files, addr)
}

// ------------------------------------------------------------- archives

struct TarEntry<'a> {
    path: &'a str,
    data: &'a [u8],
    mode: u32,
    link: Option<&'a str>,
}

fn file<'a>(path: &'a str, data: &'a [u8], mode: u32) -> TarEntry<'a> {
    TarEntry {
        path,
        data,
        mode,
        link: None,
    }
}

fn symlink<'a>(path: &'a str, target: &'a str) -> TarEntry<'a> {
    TarEntry {
        path,
        data: b"",
        mode: 0o777,
        link: Some(target),
    }
}

fn tar_gz(entries: &[TarEntry<'_>]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_mode(entry.mode);
        match entry.link {
            Some(target) => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                builder
                    .append_link(&mut header, entry.path, target)
                    .expect("link");
            },
            None => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(entry.data.len() as u64);
                // `append_data` rejects `..`; write the raw name for the
                // traversal tests.
                let name = entry.path.as_bytes();
                let gnu = header.as_gnu_mut().expect("gnu header");
                gnu.name[..name.len()].copy_from_slice(name);
                header.set_cksum();
                builder.append(&header, entry.data).expect("file");
            },
        }
    }
    builder.into_inner().expect("tar").finish().expect("gzip")
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            zip.start_file(*name, options).expect("start");
            zip.write_all(data).expect("write");
        }
        zip.finish().expect("finish");
    }
    out.into_inner()
}

// ---------------------------------------------------------------- fakes

#[derive(Clone, Default)]
struct FakeNpm {
    calls: Arc<Mutex<Vec<NpmArgs>>>,
    fail: Arc<Mutex<bool>>,
}

#[async_trait]
impl NpmRunner for FakeNpm {
    async fn ci(&self, args: &NpmArgs) -> Result<(), AcpError> {
        self.calls.lock().expect("calls").push(args.clone());
        if *self.fail.lock().expect("fail") {
            return Err(AcpError::NpmFailed("boom".into()));
        }
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(args.prefix.join("package.json")).expect("package.json"),
        )
        .expect("json");
        for package in manifest["dependencies"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
        {
            let dist = args.prefix.join("node_modules").join(package).join("dist");
            std::fs::create_dir_all(&dist).expect("dist");
            std::fs::write(dist.join("index.js"), "// fake").expect("entry");
        }
        Ok(())
    }
}

/// Records every spec, then delegates to a FakeAgent connector.
#[derive(Clone)]
struct Recording {
    inner: DuplexConnector,
    specs: Arc<Mutex<Vec<LaunchSpec>>>,
}

#[async_trait]
impl AgentConnector for Recording {
    async fn connect(&self, spec: &LaunchSpec) -> Result<AgentIo, AcpError> {
        let mut record = spec.clone();
        if let Some(cwd) = &spec.cwd {
            let empty = std::fs::read_dir(cwd)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            record
                .env
                .insert("__cwd_was_empty".into(), empty.to_string());
        }
        self.specs.lock().expect("specs").push(record);
        self.inner.connect(spec).await
    }
}

fn host_platform() -> Platform {
    detect_from(std::env::consts::OS, std::env::consts::ARCH, false, true)
        .expect("supported test host")
}

const LOCK: &str =
    r#"{"name":"pcx","lockfileVersion":3,"requires":true,"packages":{"":{"name":"pcx"}}}"#;

struct Env {
    _dir: TempDir,
    root: PathBuf,
    files: Files,
    base: String,
    npm: FakeNpm,
    specs: Arc<Mutex<Vec<LaunchSpec>>>,
    connector: DuplexConnector,
    in_use: Arc<Mutex<Vec<(String, String)>>>,
}

impl Env {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("acp");
        let (files, addr) = server().await;
        let (connector, _fake) = FakeAgent::spawn(FakeScript::default());
        Self {
            _dir: dir,
            root,
            files,
            base: format!("http://{addr}"),
            npm: FakeNpm::default(),
            specs: Arc::default(),
            connector,
            in_use: Arc::default(),
        }
    }

    fn ctx(&self, catalog: &str) -> InstallContext {
        let in_use = self.in_use.clone();
        InstallContext {
            root: self.root.clone(),
            catalog: Arc::new(parse_catalog(catalog).expect("catalog")),
            locks: Arc::new(BTreeMap::from([("lock-a".to_string(), LOCK.to_string())])),
            npm: Arc::new(self.npm.clone()),
            fetch: FetchPolicy {
                allowed_hosts: Vec::new(),
                max_bytes: 1 << 30,
                allow_loopback_http: true,
            },
            connector: Arc::new(Recording {
                inner: self.connector.clone(),
                specs: self.specs.clone(),
            }),
            platform: Ok(host_platform()),
            codex_binary: None,
            in_use: Arc::new(move |id, version| {
                in_use
                    .lock()
                    .expect("in use")
                    .iter()
                    .any(|(i, v)| i == id && (v == version || v == "*"))
            }),
            opencode_major: Arc::new(|| None),
        }
    }

    /// Serve a fake Node runtime and return the `[node]` catalog table.
    fn node_table(&self) -> String {
        let script = b"#!/bin/sh\necho v24.21.0\n";
        let archive = tar_gz(&[
            file("node-test/bin/node", script, 0o755),
            file("node-test/lib/node_modules/npm/bin/npm-cli.js", b"", 0o644),
        ]);
        let integrity = fetch::sri("sha256", &archive);
        self.files.put("/node.tar.gz", archive);
        format!(
            "[node]\nversion = \"24.21.0\"\n[node.targets.{}]\nurl = \
             \"{}/node.tar.gz\"\nintegrity = \"{integrity}\"\nroot = \"node-test\"\n",
            host_platform().key,
            self.base
        )
    }

    /// Serve an archive agent build and return its target table key/value.
    fn archive_target(&self, path: &str) -> String {
        let archive = tar_gz(&[file("bin/agent", b"#!/bin/sh\n", 0o755)]);
        let integrity = fetch::sri("sha512", &archive);
        self.files.put(path, archive);
        format!(
            "[agents.releases.targets.{}]\nurl = \"{}{path}\"\nintegrity = \"{integrity}\"\ncmd = \
             \"bin/agent\"\n",
            host_platform().key,
            self.base
        )
    }

    fn installed_version(&self, id: &str) -> Option<String> {
        read_installed(&Layout::new(&self.root))
            .agents
            .get(id)
            .map(|a| a.version.clone())
    }
}

fn npm_agent(id: &str, version: &str, extra: &str) -> String {
    let platforms = platform_list();
    format!(
        "[[agents]]\nid = \"{id}\"\nname = \"{id} name\"\napprox_size_mb = \
         1\n{extra}\n[[agents.releases]]\nversion = \"{version}\"\nkind = \"npm\"\npackage = \
         \"@test/{id}\"\nentry = \"node_modules/@test/{id}/dist/index.js\"\nlock = \
         \"lock-a\"\nomit = [\"optional\"]\nplatforms = {platforms}\n"
    )
}

fn platform_list() -> String {
    format!("[\"{}\"]", host_platform().key)
}

fn archive_agent(env: &Env, id: &str, version: &str, family: Option<&str>) -> String {
    let family = family
        .map(|f| format!("data_family = \"{f}\"\n"))
        .unwrap_or_default();
    format!(
        "[[agents]]\nid = \"{id}\"\nname = \"{id} name\"\napprox_size_mb = \
         1\n[[agents.releases]]\nversion = \"{version}\"\nkind = \"archive\"\nargs = \
         [\"acp\"]\n{family}{}",
        env.archive_target(&format!("/{id}-{version}.tar.gz"))
    )
}

fn catalog(parts: &[String]) -> String {
    format!("schema = 1\ngenerated_at = \"2026-10-01T00:00:00Z\"\n{}", parts.join("\n"))
}

async fn wait_job(id: &str) -> JobProgress {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(job) = install::job(id) {
                if matches!(job.state.as_str(), "done" | "failed") {
                    return job;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("job finishes")
}

async fn install_ok(ctx: &InstallContext, id: &str, version: Option<&str>) -> JobProgress {
    let job = install::start_install(id, version, false, ctx)
        .await
        .expect("install starts");
    let done = wait_job(&job).await;
    assert_eq!(done.state, "done", "{done:?}");
    done
}

// ---------------------------------------------------------------- tests

#[test]
fn embedded_catalog_parses() {
    let catalog = parse_catalog(install::catalog::embedded_text()).expect("embedded catalog");
    assert_eq!(catalog.schema, 1);
    assert_eq!(install::catalog(), &catalog);
    let locks: Vec<&str> = install::locks::LOCKS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    for agent in &catalog.agents {
        assert!(!agent.releases.is_empty(), "{} has releases", agent.id);
        for release in &agent.releases {
            if let install::ReleaseKind::Npm {
                lock, ..
            } = &release.kind
            {
                assert!(locks.contains(&lock.as_str()), "{lock} is embedded");
                assert!(catalog.node.is_some(), "npm agents need [node]");
            }
        }
    }
    for (_, text) in install::locks::LOCKS {
        let lock: Value = serde_json::from_str(text).expect("lock json");
        assert_eq!(lock["lockfileVersion"], 3);
    }
}

#[test]
fn platform_key_and_baseline_selection() {
    let mac = detect_from("macos", "aarch64", false, true).expect("mac");
    assert_eq!(
        (mac.key, mac.npm_os, mac.npm_cpu, mac.libc),
        ("darwin-aarch64", "darwin", "arm64", None)
    );
    let linux = detect_from("linux", "x86_64", false, false).expect("linux");
    assert_eq!(
        (linux.key, linux.npm_os, linux.npm_cpu, linux.libc),
        ("linux-x86_64", "linux", "x64", Some("glibc"))
    );
    assert_eq!(linux.archive_keys(), vec![
        "linux-x86_64-baseline".to_string(),
        "linux-x86_64".into()
    ]);
    let win = detect_from("windows", "aarch64", false, false).expect("win");
    assert_eq!((win.key, win.npm_os, win.npm_cpu), ("windows-aarch64", "win32", "arm64"));
    assert!(win.avx2, "AVX2 is irrelevant off x86_64");
    assert_eq!(win.archive_keys(), vec!["windows-aarch64".to_string()]);
    assert!(detect_from("freebsd", "x86_64", false, true).is_err());
    assert!(detect_from("linux", "riscv64", false, true).is_err());
    let target = |cmd: &str| install::catalog::ArchiveTarget {
        cmd: Some(cmd.into()),
        ..Default::default()
    };
    let targets = BTreeMap::from([
        ("linux-x86_64".to_string(), target("plain")),
        ("linux-x86_64-baseline".to_string(), target("baseline")),
    ]);
    assert_eq!(
        archive_target(&targets, &linux)
            .and_then(|t| t.cmd.clone())
            .as_deref(),
        Some("baseline")
    );
    let avx = detect_from("linux", "x86_64", false, true).expect("avx");
    assert_eq!(
        archive_target(&targets, &avx)
            .and_then(|t| t.cmd.clone())
            .as_deref(),
        Some("plain")
    );
    assert!(platform::detect().is_ok() || cfg!(target_env = "musl"));
}

#[tokio::test]
async fn musl_is_unsupported() {
    let error = detect_from("linux", "x86_64", true, true).expect_err("musl");
    assert_eq!(error.code(), "acp.unsupported_platform");
    assert!(error.detail().contains("musl"));
    let env = Env::new().await;
    let mut ctx = env.ctx(&catalog(&[npm_agent("musl-a", "1.0.0", "")]));
    ctx.platform = Err(error);
    let custom_bin = env.root.join("custom-agent");
    std::fs::create_dir_all(&env.root).expect("root");
    std::fs::write(&custom_bin, "#!/bin/sh\n").expect("custom");
    install::put_custom_agent(&ctx, &CustomAgentDef {
        id: "mine".into(),
        name: "Mine".into(),
        command: custom_bin.to_string_lossy().into_owned(),
        args: vec!["--acp".into()],
        env: vec![],
    })
    .expect("custom agents still work");
    let status = install::agents_status(&ctx);
    assert_eq!(status[0].state, "unsupported_platform");
    assert!(!status[0].remote_install_allowed);
    assert_eq!(status[1].state, "installed");
    let refused = install::start_install("musl-a", None, false, &ctx)
        .await
        .expect_err("refused");
    assert_eq!(refused.code(), "acp.unsupported_platform");
    assert_eq!(install::resolve_launch("mine", &ctx).expect("custom").args, vec![
        "--acp".to_string()
    ]);
}

#[tokio::test]
async fn download_verifies_sha256_and_sha512() {
    let (files, addr) = server().await;
    let data = b"payload".repeat(1000);
    files.put("/a.bin", data.clone());
    let dir = tempfile::tempdir().expect("dir");
    let policy = FetchPolicy {
        allowed_hosts: vec![],
        max_bytes: 1 << 20,
        allow_loopback_http: true,
    };
    let progress = Arc::new(Mutex::new(0u64));
    let seen = progress.clone();
    for algorithm in ["sha256", "sha512"] {
        let dest = dir.path().join(format!("{algorithm}.bin"));
        fetch::download(
            &format!("http://{addr}/a.bin"),
            &fetch::sri(algorithm, &data),
            &dest,
            &policy,
            &|b, _| {
                *seen.lock().expect("progress") = b;
            },
        )
        .await
        .expect("download");
        assert_eq!(std::fs::read(&dest).expect("read"), data);
    }
    assert_eq!(*progress.lock().expect("progress"), data.len() as u64);
    let small = FetchPolicy {
        max_bytes: 10,
        ..policy.clone()
    };
    let too_big = fetch::download(
        &format!("http://{addr}/a.bin"),
        &fetch::sri("sha256", &data),
        &dir.path().join("big"),
        &small,
        &|_, _| {},
    )
    .await;
    assert_eq!(too_big.expect_err("limit").code(), "acp.download_failed");
}

#[tokio::test]
async fn integrity_mismatch_deletes_file() {
    let (files, addr) = server().await;
    files.put("/a.bin", b"real".to_vec());
    let dir = tempfile::tempdir().expect("dir");
    let dest = dir.path().join("a.bin");
    let policy = FetchPolicy {
        allowed_hosts: vec![],
        max_bytes: 1 << 20,
        allow_loopback_http: true,
    };
    let error = fetch::download(
        &format!("http://{addr}/a.bin"),
        &fetch::sri("sha256", b"other"),
        &dest,
        &policy,
        &|_, _| {},
    )
    .await
    .expect_err("mismatch");
    assert_eq!(error.code(), "acp.integrity_mismatch");
    assert!(!dest.exists());
    let unknown =
        fetch::download(&format!("http://{addr}/a.bin"), "md5-abc", &dest, &policy, &|_, _| {})
            .await;
    assert_eq!(unknown.expect_err("algorithm").code(), "acp.integrity_mismatch");
}

#[tokio::test]
async fn redirect_to_unlisted_host_is_rejected() {
    let (files, addr) = server().await;
    files.put("/real.bin", b"x".to_vec());
    files.redirect("/elsewhere", "http://downloads.example.invalid/x.bin");
    files.redirect("/local", &format!("http://{addr}/real.bin"));
    let dir = tempfile::tempdir().expect("dir");
    let policy = FetchPolicy {
        allowed_hosts: vec![],
        max_bytes: 1 << 20,
        allow_loopback_http: true,
    };
    let sri = fetch::sri("sha256", b"x");
    let error = fetch::download(
        &format!("http://{addr}/elsewhere"),
        &sri,
        &dir.path().join("a"),
        &policy,
        &|_, _| {},
    )
    .await
    .expect_err("redirect refused");
    assert_eq!(error.code(), "acp.download_failed");
    fetch::download(
        &format!("http://{addr}/local"),
        &sri,
        &dir.path().join("b"),
        &policy,
        &|_, _| {},
    )
    .await
    .expect("allowed redirect");
    let production = FetchPolicy::production();
    assert!(!production.allow_loopback_http);
    let refused = fetch::download(
        &format!("http://{addr}/real.bin"),
        &sri,
        &dir.path().join("c"),
        &production,
        &|_, _| {},
    )
    .await;
    assert_eq!(refused.expect_err("http refused").code(), "acp.download_failed");
    assert!(production.allows(&url::Url::parse("https://registry.npmjs.org/x").expect("url")));
    assert!(!production.allows(&url::Url::parse("https://evil.example.com/x").expect("url")));
}

#[test]
fn archive_rejects_traversal_symlink_escape_and_size_bombs() {
    let dir = tempfile::tempdir().expect("dir");
    let write = |name: &str, bytes: Vec<u8>| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    };
    let rejected = |archive: &Path| {
        let dest = dir
            .path()
            .join(format!("out-{}", uuid::Uuid::new_v4().simple()));
        let error = archive::extract(archive, &dest).expect_err("rejected");
        assert_eq!(error.code(), "acp.archive_rejected", "{error}");
    };
    rejected(&write("traversal.tar.gz", tar_gz(&[file("../evil.txt", b"x", 0o644)])));
    rejected(&write("absolute.zip", zip_bytes(&[("/etc/evil", b"x")])));
    rejected(&write("drive.zip", zip_bytes(&[("C:/evil", b"x")])));
    rejected(&write("zipdots.zip", zip_bytes(&[("a/../../evil", b"x")])));
    rejected(&write("escape.tar.gz", tar_gz(&[symlink("a/link", "../../outside")])));
    rejected(&write("absolute-link.tar.gz", tar_gz(&[symlink("link", "/etc/passwd")])));
    #[cfg(unix)]
    rejected(&write("chain.tar.gz", tar_gz(&[symlink("l1", "."), symlink("l2", "l1/..")])));
    #[cfg(unix)]
    rejected(&write(
        "through-link.tar.gz",
        tar_gz(&[
            file("real/f", b"", 0o644),
            symlink("dirlink", "real"),
            file("dirlink/g", b"x", 0o644),
        ]),
    ));
    let bomb = write("bomb.tar.gz", tar_gz(&[file("big", &[0u8; 4096], 0o644)]));
    let error =
        archive::extract_with_limits(&bomb, &dir.path().join("bomb"), 1024, 10).expect_err("size");
    assert_eq!(error.code(), "acp.archive_rejected");
    let many = write("many.zip", zip_bytes(&[("a", b"1"), ("b", b"2"), ("c", b"3")]));
    let error = archive::extract_with_limits(&many, &dir.path().join("many"), 1 << 20, 2)
        .expect_err("entries");
    assert_eq!(error.code(), "acp.archive_rejected");
    // A well-formed archive with an internal link and a setuid bit.
    let good = write(
        "good.tar.gz",
        tar_gz(&[file("pkg/lib/cli.js", b"cli", 0o4755), symlink("pkg/bin/npm", "../lib/cli.js")]),
    );
    let out = dir.path().join("good");
    archive::extract(&good, &out).expect("good archive");
    assert_eq!(std::fs::read(out.join("pkg/lib/cli.js")).expect("cli"), b"cli");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(out.join("pkg/lib/cli.js"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755, "setuid cleared");
        assert_eq!(std::fs::read(out.join("pkg/bin/npm")).expect("via link"), b"cli");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn npm_install_builds_expected_command() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[env.node_table(), npm_agent("npm-a", "1.0.0", "")]));
    install_ok(&ctx, "npm-a", None).await;
    let calls = env.npm.calls.lock().expect("calls").clone();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let platform = host_platform();
    assert!(call.node.ends_with("node-test/bin/node"));
    assert!(call
        .npm_cli
        .ends_with("lib/node_modules/npm/bin/npm-cli.js"));
    let prefix = call.prefix.to_string_lossy().into_owned();
    assert!(prefix.contains("agents/npm-a/1.0.0.staging-"), "{prefix}");
    let args = call.args.join(" ");
    for expected in [
        "ci --prefix".to_string(),
        "--ignore-scripts".into(),
        "--no-audit".into(),
        "--no-fund".into(),
        "--no-update-notifier".into(),
        format!("--os={}", platform.npm_os),
        format!("--cpu={}", platform.npm_cpu),
        "--omit=optional".into(),
        "--registry=https://registry.npmjs.org/".into(),
        format!("--cache={}", env.root.join("npm-cache").display()),
        format!("--userconfig={}", env.root.join("npmrc").display()),
    ] {
        assert!(args.contains(&expected), "missing {expected} in {args}");
    }
    assert_eq!(
        call.env
            .get("npm_config_update_notifier")
            .map(String::as_str),
        Some("false")
    );
    assert!(call.env.get("PATH").is_some_and(|p| p.starts_with(
        &call
            .node
            .parent()
            .expect("bin")
            .to_string_lossy()
            .into_owned()
    )));
    let dir = env.root.join("agents/npm-a/1.0.0");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).expect("pkg"))
            .expect("json");
    assert_eq!(
        manifest,
        json!({"name": "pcx-npm-a", "private": true, "dependencies": {"@test/npm-a": "1.0.0"}})
    );
    assert_eq!(std::fs::read_to_string(dir.join("package-lock.json")).expect("lock"), LOCK);
    let installed = read_installed(&Layout::new(&env.root));
    assert_eq!(installed.node.as_ref().map(|n| n.version.as_str()), Some("24.21.0"));
    let record = &installed.agents["npm-a"];
    assert_eq!(
        (record.kind.as_str(), record.source.as_str(), record.path.as_str()),
        ("npm", "catalog", "agents/npm-a/1.0.0")
    );
    assert_eq!(record.agent_version.as_deref(), Some("1.0.0"));
    let spec = install::resolve_launch("npm-a", &ctx).expect("launch");
    assert!(spec.program.ends_with("node-test/bin/node"));
    assert_eq!(PathBuf::from(&spec.args[0]), dir.join("node_modules/@test/npm-a/dist/index.js"));
    assert!(spec.pinned);
}

#[cfg(unix)]
#[tokio::test]
async fn validation_failure_keeps_previous_version() {
    let env = Env::new().await;
    let v1 = env.ctx(&catalog(&[archive_agent(&env, "val-a", "1.0.0", None)]));
    install_ok(&v1, "val-a", None).await;
    let mut bad = FakeScript::default();
    bad.initialize["protocolVersion"] = json!(2);
    let (connector, _fake) = FakeAgent::spawn(bad);
    let mut v2 = env.ctx(&catalog(&[archive_agent(&env, "val-a", "2.0.0", None)]));
    v2.connector = Arc::new(connector);
    let job = install::start_install("val-a", None, false, &v2)
        .await
        .expect("starts");
    let failed = wait_job(&job).await;
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.error_code.as_deref(), Some("acp.validation_failed"));
    assert_eq!(env.installed_version("val-a").as_deref(), Some("1.0.0"));
    assert!(env.root.join("agents/val-a/1.0.0/bin/agent").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(env.root.join("agents/val-a"))
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, vec!["1.0.0".to_string()]);
    assert_eq!(install::agents_status(&v2)[0].state, "installed");
}

#[cfg(unix)]
#[tokio::test]
async fn upgrade_defers_gc_while_hosted() {
    let env = Env::new().await;
    let v1 = env.ctx(&catalog(&[archive_agent(&env, "gc-a", "1.0.0", None)]));
    install_ok(&v1, "gc-a", None).await;
    env.in_use
        .lock()
        .expect("in use")
        .push(("gc-a".into(), "1.0.0".into()));
    let v2 = env.ctx(&catalog(&[archive_agent(&env, "gc-a", "2.0.0", None)]));
    install_ok(&v2, "gc-a", None).await;
    let installed = read_installed(&Layout::new(&env.root));
    assert_eq!(installed.agents["gc-a"].version, "2.0.0");
    assert_eq!(installed.gc_pending, vec!["agents/gc-a/1.0.0".to_string()]);
    assert!(env.root.join("agents/gc-a/1.0.0").exists());
    install::gc_sweep(&v2);
    assert!(env.root.join("agents/gc-a/1.0.0").exists(), "still hosted");
    env.in_use.lock().expect("in use").clear();
    install::gc_sweep(&v2);
    assert!(!env.root.join("agents/gc-a/1.0.0").exists());
    assert!(read_installed(&Layout::new(&env.root))
        .gc_pending
        .is_empty());
}

#[cfg(unix)]
fn fake_codex(dir: &Path, version: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(format!("codex-{version}"));
    std::fs::write(&path, format!("#!/bin/sh\necho codex-cli {version}\n")).expect("codex");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[cfg(unix)]
#[tokio::test]
async fn codex_release_selected_by_engine_version() {
    let env = Env::new().await;
    let platforms = platform_list();
    let text = catalog(&[
        env.node_table(),
        format!(
            "[[agents]]\nid = \"codex-t\"\nname = \"Codex test\"\napprox_size_mb = \
             1\nexternal_engine = {{ env = \"CODEX_PATH\", binary = \"codex\", setting = \
             \"codex_binary\", version_args = [\"--version\"] }}\n[[agents.releases]]\nversion = \
             \"2.0.1\"\nengine_range = \">=0.159.1, <0.160.0\"\nkind = \"npm\"\npackage = \
             \"@test/codex\"\nentry = \"node_modules/@test/codex/dist/index.js\"\nlock = \
             \"lock-a\"\nomit = [\"optional\"]\nplatforms = \
             {platforms}\n[[agents.releases]]\nversion = \"1.12.0\"\nengine_range = \">=0.154.0, \
             <0.155.0\"\nkind = \"npm\"\npackage = \"@test/codex\"\nentry = \
             \"node_modules/@test/codex/dist/index.js\"\nlock = \"lock-a\"\nomit = \
             [\"optional\"]\nplatforms = {platforms}\n"
        ),
    ]);
    let bins = tempfile::tempdir().expect("bins");
    let mut ctx = env.ctx(&text);
    let missing = install::start_install("codex-t", None, false, &ctx)
        .await
        .expect_err("no codex");
    assert_eq!(missing.code(), "acp.engine_missing");
    ctx.codex_binary = Some(fake_codex(bins.path(), "0.1.0"));
    let old = install::start_install("codex-t", None, false, &ctx)
        .await
        .expect_err("too old");
    assert_eq!(old.code(), "acp.engine_incompatible");
    ctx.codex_binary = Some(fake_codex(bins.path(), "0.154.0"));
    let job = install_ok(&ctx, "codex-t", None).await;
    assert_eq!(job.version, "1.12.0");
    let spec = install::resolve_launch("codex-t", &ctx).expect("launch");
    assert_eq!(
        spec.env.get("CODEX_PATH"),
        ctx.codex_binary
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .as_ref()
    );
    ctx.codex_binary = Some(fake_codex(bins.path(), "0.159.3"));
    match install::resolve_launch("codex-t", &ctx) {
        Err(AcpError::EngineIncompatible {
            found,
            suggested,
            ..
        }) => {
            assert_eq!(found, "0.159.3");
            assert_eq!(suggested.as_deref(), Some("2.0.1"));
        },
        other => panic!("expected engine_incompatible, got {other:?}"),
    }
    assert_eq!(install::agents_status(&ctx)[0].state, "engine_incompatible");
}

#[tokio::test]
async fn remote_install_requires_toggle_and_pinned_version() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[archive_agent(&env, "remote-a", "1.0.0", None)]));
    let mut view = install::settings(&ctx).expect("settings");
    assert!(view.remote_management, "on by default (D14)");
    view.remote_management = false;
    install::save_settings(&ctx, &view, false).expect("save");
    let disabled = install::start_install("remote-a", None, true, &ctx)
        .await
        .expect_err("disabled");
    assert_eq!(disabled, AcpError::RemoteManagementDisabled);
    view.remote_management = true;
    install::save_settings(&ctx, &view, false).expect("save");
    let unpinned = install::start_install("remote-a", Some("9.9.9"), true, &ctx)
        .await
        .expect_err("unpinned");
    assert_eq!(unpinned.code(), "acp.version_not_pinned");
    let unknown = install::start_install("nope", None, true, &ctx)
        .await
        .expect_err("unknown");
    assert_eq!(unknown.code(), "acp.unknown_agent");
    if cfg!(unix) {
        let job = install::start_install("remote-a", Some("1.0.0"), true, &ctx)
            .await
            .expect("pinned remote install");
        assert_eq!(wait_job(&job).await.state, "done");
    }
    let log = std::fs::read_to_string(env.root.join("audit.log")).expect("audit");
    let entries: Vec<Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).expect("jsonl"))
        .collect();
    assert!(entries.iter().any(|e| e["action"] == "install"
        && e["remote"] == true
        && e["code"] == "acp.remote_management_disabled"));
}

#[tokio::test]
async fn audit_log_rotates_and_omits_env() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[]));
    let layout = Layout::new(&env.root);
    std::fs::create_dir_all(&env.root).expect("root");
    let command = env.root.join("bin-mine");
    std::fs::write(&command, "").expect("bin");
    install::put_custom_agent(&ctx, &CustomAgentDef {
        id: "mine".into(),
        name: "Mine".into(),
        command: command.to_string_lossy().into_owned(),
        args: vec![],
        env: vec![("API_KEY".into(), "super-secret-value".into())],
    })
    .expect("custom");
    let log = std::fs::read_to_string(layout.audit_log()).expect("audit");
    assert!(log.contains("\"custom_agent\""));
    assert!(!log.contains("super-secret-value"));
    assert!(!log.contains("bin-mine"), "no paths in the audit log");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(layout.audit_log())
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let entry = AuditEntry::new("install", &"a".repeat(2000), false);
    for _ in 0..(4 * audit::ROTATE_BYTES as usize / 2000) {
        audit::record(&layout, &entry);
    }
    let rotated = |n: u32| PathBuf::from(format!("{}.{n}", layout.audit_log().display()));
    assert!(rotated(1).exists() && rotated(2).exists() && rotated(3).exists());
    assert!(!rotated(4).exists());
    assert!(
        std::fs::metadata(layout.audit_log())
            .expect("current")
            .len()
            <= audit::ROTATE_BYTES + 4096
    );
}

#[tokio::test]
async fn settings_round_trip_preserves_unknown_keys() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[]));
    std::fs::create_dir_all(&env.root).expect("root");
    std::fs::write(
        env.root.join("agents.toml"),
        "schema = 1\nremote_management = true\nfuture_key = \"kept\"\n[agents.other]\nunknown = \
         3\n[future_table]\nx = [1, 2]\n",
    )
    .expect("write");
    let mut view = install::settings(&ctx).expect("read");
    assert!(view.remote_management);
    view.remote_management = false;
    view.npm_registry = Some("https://registry.npmmirror.com/".into());
    let (saved, warnings) = install::save_settings(&ctx, &view, false).expect("save");
    assert!(saved && warnings.is_empty());
    let text = std::fs::read_to_string(env.root.join("agents.toml")).expect("read back");
    let parsed: toml::Table = text.parse().expect("toml");
    assert_eq!(parsed["future_key"].as_str(), Some("kept"));
    assert_eq!(parsed["agents"]["other"]["unknown"].as_integer(), Some(3));
    assert_eq!(parsed["future_table"]["x"].as_array().map(Vec::len), Some(2));
    assert_eq!(parsed["remote_management"].as_bool(), Some(false));
    let again = install::settings(&ctx).expect("read again");
    assert_eq!(again.npm_registry.as_deref(), Some("https://registry.npmmirror.com/"));
    view.npm_registry = Some("http://insecure.example.com/".into());
    assert!(install::save_settings(&ctx, &view, false).is_err(), "the mirror must be https");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(env.root.join("agents.toml"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn validation_runs_from_staging_dir_with_empty_cwd() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[archive_agent(&env, "stage-a", "1.0.0", Some("opencode-v2"))]));
    install_ok(&ctx, "stage-a", None).await;
    let specs = env.specs.lock().expect("specs").clone();
    let spec = specs.last().expect("validation launch");
    let program = spec.program.to_string_lossy().into_owned();
    assert!(program.contains("agents/stage-a/1.0.0.staging-"), "{program}");
    let cwd = spec.cwd.clone().expect("cwd");
    assert!(cwd.starts_with(env.root.join("tmp")));
    assert_eq!(spec.env.get("__cwd_was_empty").map(String::as_str), Some("true"));
    assert_eq!(
        spec.env.get("OPENCODE_DB"),
        Some(&cwd.join("validate.db").to_string_lossy().into_owned())
    );
    assert!(!cwd.exists(), "the validation directory is removed");
    // At launch, D19 isolates the database when the user's OpenCode differs.
    let spec = install::resolve_launch("stage-a", &ctx).expect("launch");
    assert_eq!(
        spec.env.get("OPENCODE_DB").map(PathBuf::from),
        Some(env.root.join("data/opencode-v2/opencode.db"))
    );
    let mut shared = ctx.clone();
    shared.opencode_major = Arc::new(|| Some(2));
    let env_vars = install::resolve_launch("stage-a", &shared)
        .expect("launch")
        .env;
    assert!(!env_vars.contains_key("OPENCODE_DB"));
}

#[cfg(unix)]
#[tokio::test]
async fn shared_data_mode_returns_coexistence_warning() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[
        archive_agent(&env, "oc1", "1.0.0", Some("opencode-v1")),
        archive_agent(&env, "oc2", "2.0.0", Some("opencode-v2")),
    ]));
    install_ok(&ctx, "oc2", None).await;
    let mut view = install::settings(&ctx).expect("settings");
    assert_eq!(view.opencode_data, vec![
        ("opencode-v1".into(), "auto".into()),
        ("opencode-v2".into(), "auto".into())
    ]);
    view.opencode_data = vec![("opencode-v1".into(), "shared".into())];
    let (saved, warnings) = install::save_settings(&ctx, &view, false).expect("save");
    assert!(!saved);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("oc2"));
    assert_eq!(install::settings(&ctx).expect("unchanged").opencode_data[0].1, "auto");
    let (saved, warnings) = install::save_settings(&ctx, &view, true).expect("force");
    assert!(saved && !warnings.is_empty());
    assert_eq!(install::settings(&ctx).expect("saved").opencode_data[0].1, "shared");
}

fn claude_like(id: &str) -> String {
    npm_agent(
        id,
        "0.84.0",
        "engine_override = { env = \"CLAUDE_CODE_EXECUTABLE\", setting = \"engine_path\" }",
    )
    .replace(
        "omit = [\"optional\"]",
        "omit = []\nconditional_args = [{ setting = \"allow_subscription_login\", default = \
         false, when_false = [\"--hide-claude-auth\"], label_key = \"acpSubscriptionLogin\", \
         confirm_key = \"acpSubscriptionLoginConfirmBody\" }]",
    )
}

#[cfg(unix)]
#[tokio::test]
async fn conditional_args_add_hide_claude_auth_by_default() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[env.node_table(), claude_like("claude-t")]));
    install_ok(&ctx, "claude-t", None).await;
    let spec = install::resolve_launch("claude-t", &ctx).expect("launch");
    assert_eq!(spec.launch_only_args, vec!["--hide-claude-auth".to_string()]);
    assert!(!spec.args.contains(&"--hide-claude-auth".to_string()));
    let validation = env
        .specs
        .lock()
        .expect("specs")
        .last()
        .cloned()
        .expect("validated");
    assert_eq!(validation.launch_only_args, vec!["--hide-claude-auth".to_string()]);
    let mut view = install::settings(&ctx).expect("settings");
    assert_eq!(view.flags.len(), 1);
    assert_eq!(view.flags[0].label_key, "acpSubscriptionLogin");
    assert!(!view.flags[0].value);
    view.flags[0].value = true;
    view.claude_engine_path = Some("/opt/claude/bin/claude".into());
    install::save_settings(&ctx, &view, false).expect("save");
    let spec = install::resolve_launch("claude-t", &ctx).expect("launch");
    assert!(spec.launch_only_args.is_empty());
    assert_eq!(
        spec.env.get("CLAUDE_CODE_EXECUTABLE").map(String::as_str),
        Some("/opt/claude/bin/claude")
    );
    let log = std::fs::read_to_string(env.root.join("audit.log")).expect("audit");
    assert!(log.contains("allow_subscription_login"));
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_login_excludes_launch_only_args() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[env.node_table(), claude_like("claude-term")]));
    install_ok(&ctx, "claude-term", None).await;
    let spec = install::resolve_launch("claude-term", &ctx).expect("launch");
    let spec_method = AuthMethod {
        id: "console".into(),
        kind: Some("terminal".into()),
        args: vec!["--cli".into(), "auth".into(), "login".into(), "--console".into()],
        ..AuthMethod::default()
    };
    let launch = terminal_launch(&spec_method, &spec).expect("spec form");
    assert!(!launch.args.contains(&"--hide-claude-auth".to_string()));
    assert_eq!(launch.args[1..], ["--cli", "auth", "login", "--console"].map(String::from));
    let program = spec
        .program
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let legacy = AuthMethod {
        id: "legacy".into(),
        meta: Some(
            json!({"terminal-auth": {"command": program, "args": [spec.args[0].clone(), "--hide-claude-auth", "--cli", "login"]}}),
        ),
        ..AuthMethod::default()
    };
    let launch = terminal_launch(&legacy, &spec).expect("legacy form");
    assert_eq!(launch.args, vec![spec.args[0].clone(), "--cli".into(), "login".into()]);
}

#[tokio::test]
async fn gateway_token_is_write_only_and_not_audited() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[]));
    let mut view = AcpSettingsView {
        remote_management: true,
        ..AcpSettingsView::default()
    };
    view.gateways = vec![GatewayView {
        agent_id: "claude-acp".into(),
        base_url: "https://relay.example.com".into(),
        token: Some("sk-very-secret".into()),
        extra_headers: vec![("X-Team".into(), "a".into())],
        ..GatewayView::default()
    }];
    install::save_settings(&ctx, &view, false).expect("save");
    let read = install::settings(&ctx).expect("read");
    let gateway = &read.gateways[0];
    assert_eq!(gateway.token, None);
    assert!(gateway.has_token);
    assert_eq!(gateway.base_url, "https://relay.example.com");
    let auth = install::gateway_auth(&ctx, "claude-acp").expect("gateway auth");
    assert_eq!(
        auth.headers.get("Authorization").map(String::as_str),
        Some("Bearer sk-very-secret")
    );
    assert_eq!(auth.headers.get("X-Team").map(String::as_str), Some("a"));
    assert!(!format!("{auth:?}").contains("sk-very-secret"));
    // Saving the read view (token None) keeps the token.
    let mut keep = read.clone();
    keep.gateways[0].base_url = "https://relay2.example.com".into();
    install::save_settings(&ctx, &keep, false).expect("keep");
    assert!(install::gateway_auth(&ctx, "claude-acp").is_some());
    let log = std::fs::read_to_string(env.root.join("audit.log")).expect("audit");
    assert!(log.contains("\"gateway\""));
    assert!(!log.contains("sk-very-secret"));
    assert!(!log.contains("relay"), "no values in the audit log");
    // Some("") deletes the token; clear removes the entry.
    let mut delete = keep.clone();
    delete.gateways[0].token = Some(String::new());
    install::save_settings(&ctx, &delete, false).expect("delete token");
    assert!(!install::settings(&ctx).expect("read").gateways[0].has_token);
    assert!(install::gateway_auth(&ctx, "claude-acp").is_none());
    let mut clear = delete.clone();
    clear.gateways[0].clear = true;
    install::save_settings(&ctx, &clear, false).expect("clear");
    assert!(install::settings(&ctx).expect("read").gateways.is_empty());
}

#[tokio::test]
async fn gateway_url_requires_https_except_private_networks() {
    assert!(!check_gateway_url("https://relay.example.com").expect("https"));
    for private in [
        "http://127.0.0.1:3000",
        "http://localhost:8080",
        "http://10.1.2.3",
        "http://172.16.0.1",
        "http://172.31.255.1",
        "http://192.168.1.5/v1",
    ] {
        assert!(check_gateway_url(private).expect(private), "{private} is allowed with a warning");
    }
    for refused in ["http://172.32.0.1", "http://example.com", "ftp://192.168.1.1", "not a url"] {
        assert!(check_gateway_url(refused).is_err(), "{refused}");
    }
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[]));
    let view = AcpSettingsView {
        remote_management: true,
        gateways: vec![GatewayView {
            agent_id: "a".into(),
            base_url: "http://example.com".into(),
            token: Some("t".into()),
            ..GatewayView::default()
        }],
        ..AcpSettingsView::default()
    };
    assert_eq!(
        install::save_settings(&ctx, &view, false)
            .expect_err("refused")
            .code(),
        "acp.invalid_params"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn in_use_predicate_blocks_uninstall_and_defers_gc() {
    let env = Env::new().await;
    let ctx = env.ctx(&catalog(&[archive_agent(&env, "use-a", "1.0.0", None)]));
    install_ok(&ctx, "use-a", None).await;
    env.in_use
        .lock()
        .expect("in use")
        .push(("use-a".into(), "*".into()));
    assert_eq!(
        install::uninstall("use-a", &ctx)
            .expect_err("hosted")
            .code(),
        "acp.in_use"
    );
    assert!(env.root.join("agents/use-a/1.0.0").exists());
    env.in_use.lock().expect("in use").clear();
    install::uninstall("use-a", &ctx).expect("uninstall");
    assert!(!env.root.join("agents/use-a/1.0.0").exists());
    assert!(env.installed_version("use-a").is_none());
    assert_eq!(install::uninstall("use-a", &ctx).expect_err("gone").code(), "acp.not_installed");
    // Hosted custom agents cannot be deleted either.
    let command = env.root.join("custom-bin");
    std::fs::write(&command, "").expect("bin");
    let def = CustomAgentDef {
        id: "cust".into(),
        name: "C".into(),
        command: command.to_string_lossy().into_owned(),
        args: vec![],
        env: vec![],
    };
    install::put_custom_agent(&ctx, &def).expect("custom");
    env.in_use
        .lock()
        .expect("in use")
        .push(("cust".into(), "*".into()));
    assert_eq!(
        install::delete_custom_agent(&ctx, "cust")
            .expect_err("hosted")
            .code(),
        "acp.in_use"
    );
    env.in_use.lock().expect("in use").clear();
    install::delete_custom_agent(&ctx, "cust").expect("delete");
    // Startup cleanup removes leftover staging directories and tmp/.
    std::fs::create_dir_all(env.root.join("agents/use-a/9.9.9.staging-abc")).expect("staging");
    std::fs::create_dir_all(env.root.join("tmp/leftover")).expect("tmp");
    install::startup_cleanup(&ctx);
    assert!(!env.root.join("agents/use-a/9.9.9.staging-abc").exists());
    assert!(!env.root.join("tmp/leftover").exists());
    let custom_ids: Vec<String> = install::custom_agents(&ctx)
        .expect("custom")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert!(custom_ids.is_empty());
    assert!(jobs::running_job("use-a", "install").is_none());
}

#[tokio::test]
async fn management_routes_answer_404_until_registered() {
    use pocket_codex_host_svc::acp::install::manage;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, manage::router()).await;
    });
    let status = reqwest::get(format!("http://{addr}/acp/v1/agents"))
        .await
        .expect("get")
        .status();
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
}
