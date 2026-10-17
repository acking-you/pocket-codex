//! Launching an owned ACP agent in two steps: [`prepare`] checks and builds
//! everything a start needs (the spec, the executable or adapter, the
//! model-service launch and the session source) without
//! touching any running host; [`launch_prepared`] reserves the name, starts
//! the agent and publishes it. An apply prepares the new launch before it
//! stops the old host, so an unusable configuration interrupts nothing.

use super::*;

/// A start ready to run (see the module docs).
pub(crate) struct Prepared {
    pub(super) name: String,
    pub(super) spec: AgentSpec,
    pub(super) program: PathBuf,
    pub(super) claude: Option<claude::Launch>,
    pub(super) provider_launch: acp::provider::AcpProviderLaunch,
    pub(super) source: SourceId,
    pub(super) provider: host_profiles::Resolved,
    pub(super) state: PathBuf,
}

/// What a running agent was launched with, kept with its host so it can
/// be launched again exactly — the same executable or adapter, arguments,
/// environment, provider launch and session source — without resolving
/// anything anew (a failed apply's rollback).
pub(crate) struct Captured {
    pub(super) program: PathBuf,
    pub(super) claude: Option<claude::Launch>,
    pub(super) provider_launch: acp::provider::AcpProviderLaunch,
    pub(super) source: SourceId,
    pub(super) state: PathBuf,
}

impl Prepared {
    /// The exact launch `captured` ran, again.
    pub(super) fn again(
        name: &str,
        spec: AgentSpec,
        captured: Captured,
        provider: host_profiles::Resolved,
    ) -> Self {
        Self {
            name: name.to_string(),
            spec,
            program: captured.program,
            claude: captured.claude,
            provider_launch: captured.provider_launch,
            source: captured.source,
            provider,
            state: captured.state,
        }
    }
}

/// The executable a spec stands for (it keys the session record), and the
/// discovered launch when it is the Claude preset's adapter. Starting never
/// prepares a missing adapter; that is an explicit request of its own.
fn resolve(spec: &AgentSpec) -> Result<(PathBuf, Option<claude::Launch>)> {
    if claude::applies(spec) {
        let launch = claude_setup::environment()?
            .launch(&spec.args)
            .map_err(|missing| anyhow!("{missing}"))?;
        return Ok((launch.source.clone(), Some(launch)));
    }
    let program = locate(&spec.program, Some(&spec.profile_id)).ok_or_else(|| {
        anyhow!("the agent executable was not found; install it or choose its full path")
    })?;
    Ok((program, None))
}

/// The session source of `program` + `args`, under an instance provider
/// also keyed by its authority generation: sessions made through one
/// provider or account are never adopted through another. A NUL-led marker
/// cannot be a real argument.
pub(super) fn source_of(
    acp_root: &Path,
    program: &Path,
    args: &[String],
    authority: Option<&str>,
) -> std::io::Result<SourceId> {
    let Some(authority) = authority else {
        return SourceId::derive(acp_root, program, args);
    };
    let mut keyed = args.to_vec();
    keyed.push(format!("\0pocket-codex-provider\0{authority}"));
    SourceId::derive(acp_root, program, &keyed)
}

/// Check and build a start of `spec` under the canonical `name` with
/// `provider` (`None`: the instance's saved one in `profiles`). Starts and
/// stops nothing.
pub(crate) fn prepare(
    name: &str,
    spec: AgentSpec,
    provider: Option<host_profiles::Resolved>,
    profiles: &Path,
) -> Result<Prepared> {
    if !hosting_supported() {
        bail!("hosting ACP agents is not supported on this platform yet");
    }
    spec.validate().map_err(|error| anyhow!("{error}"))?;
    // An unreadable or unsupported saved configuration fails the start
    // rather than silently launching with the agent's own provider.
    let provider = match provider {
        Some(provider) => provider,
        None => host_profiles::resolve_in(profiles, name)?,
    };
    let (mut provider_launch, authority) = match host_profiles::acp(&provider, &spec, name)? {
        Some((launch, authority)) => (launch, Some(authority)),
        None => (Default::default(), None),
    };
    if spec.profile_id == claude::PROFILE_ID {
        host_profiles::environment::launch(&spec, &provider, &mut provider_launch)
            .context("resolving the Claude host environment")?;
    }
    let authority = authority.or_else(|| provider.authority.clone());
    let (program, claude) = resolve(&spec)?;
    let state = pocket_codex_core::paths::state_dir().context("resolving the state directory")?;
    // Session authority belongs to exactly this invocation (resolved
    // executable and arguments, and an instance provider), never to the name
    // or profile label.
    let source = source_of(&state.join("acp"), &program, &spec.args, authority.as_deref())
        .context("preparing the agent's session record")?;
    Ok(Prepared {
        name: name.to_string(),
        spec,
        program,
        claude,
        provider_launch,
        source,
        provider,
        state,
    })
}

/// Reserve the name, start the prepared agent and publish it.
pub(crate) fn launch_prepared(prepared: Prepared) -> Result<AcpServeReport> {
    let provider = prepared.provider.clone();
    // Agent output in an error is shown only with the key's lines withheld.
    run(prepared).map_err(|e| anyhow!(host_profiles::redact(&format!("{e:#}"), &provider)))
}

fn run(prepared: Prepared) -> Result<AcpServeReport> {
    let Prepared {
        name,
        spec,
        program,
        claude,
        provider_launch,
        source,
        provider,
        state,
    } = prepared;
    let captured = Captured {
        program: program.clone(),
        claude: claude.clone(),
        provider_launch: provider_launch.clone(),
        source: source.clone(),
        state: state.clone(),
    };
    let reservation = hosting::reserve(&name)?;
    let store = SessionStore::open(&state.join("acp"), &name, source);
    let uploads = state.join("acp-uploads");
    let rt = runtime::runtime();
    let host = {
        let _runtime = rt.enter();
        let mut env = Vec::new();
        let path = pocket_codex_codex::external_tool_path();
        let native_history = match &claude {
            Some(launch) => {
                let root = provider_launch
                    .env
                    .iter()
                    .rev()
                    .chain(launch.env.iter().rev())
                    .find(|(key, _)| key == "CLAUDE_CONFIG_DIR")
                    .map(|(_, value)| PathBuf::from(value))
                    .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
                    .or_else(|| {
                        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"))
                    })
                    .ok_or_else(|| anyhow!("Claude's local history directory is unavailable"))?;
                let mut history =
                    pocket_codex_host_svc::native_history::ClaudeHistory::from_adapter(
                        launch.program.clone(),
                        &launch.source,
                        root,
                    )?;
                let cli = provider_launch
                    .env
                    .iter()
                    .rev()
                    .chain(launch.env.iter().rev())
                    .find(|(key, _)| key == claude::CLI_ENV)
                    .map(|(_, value)| PathBuf::from(value))
                    .or_else(|| std::env::var_os(claude::CLI_ENV).map(PathBuf::from));
                if let (Some(cli), Some(home)) = (cli, std::env::var_os("HOME")) {
                    history = history.with_owner_probe(cli, home.into());
                }
                Some(Arc::new(history))
            },
            None => None,
        };
        let session_meta = provider_launch.session_meta;
        let (program, launch_args) = match claude {
            Some(launch) => {
                env.extend(launch.env);
                (launch.program, Some(launch.args))
            },
            None => (program.clone(), None),
        };
        // Last, so the instance's provider wins over inherited values.
        env.extend(provider_launch.env);
        AgentHost::create_with_claude_history(
            HostOptions {
                spec: spec.clone(),
                program,
                path,
                env,
                store,
            },
            launch_args,
            session_meta,
            native_history,
        )
    };
    // Visible to a quit before the agent exists, so it can never escape it.
    let Some(id) = owners().begin(host.clone()) else {
        bail!("the app is quitting");
    };
    // Released only after the agent was stopped: an early return below
    // stops it first, and a hosted agent's owner lives in its `AcpServe`.
    let owner = Owner(id);
    if let Err(error) = rt.block_on(host.launch()) {
        rt.block_on(host.stop());
        drop(owner);
        return Err(anyhow!("{error}"));
    }
    let serving = match publish(&name, &host, &program, uploads, owner) {
        Ok(mut serving) => {
            serving.config = Some(provider);
            serving.launch = Some(captured);
            serving
        },
        Err((error, owner)) => {
            // Roll back: the agent stops and the reservation drops.
            rt.block_on(host.stop());
            drop(owner);
            return Err(error);
        },
    };
    let out = report(&serving, false);
    let refused = {
        let mut barrier = owners();
        if barrier.commit(id) {
            let replaced = hosts().insert(name, serving);
            debug_assert!(replaced.is_none(), "the name reservation excludes a second host");
            drop(barrier);
            // Not expected (the reservation), but never dropped under a lock.
            if let Some(replaced) = replaced {
                stop_host(replaced);
            }
            None
        } else {
            Some(serving)
        }
    };
    if let Some(serving) = refused {
        // A quit began meanwhile; it stops this agent too.
        stop_host(serving);
        bail!("the app is quitting");
    }
    reservation.commit();
    Ok(out)
}

/// Serve the gateway and meta service for a launched `host` and publish
/// both. On error everything started here is stopped again and `owner` is
/// handed back (the caller still has to stop the agent).
fn publish(
    name: &str,
    host: &AgentHost,
    program: &Path,
    uploads: PathBuf,
    owner: Owner,
) -> Result<AcpServe, (anyhow::Error, Owner)> {
    let prepared = (|| {
        Ok::<_, anyhow::Error>((
            host_services::config_store()?,
            host_services::host_store()?,
            bind_loopback("ACP gateway")?,
            bind_loopback("meta service")?,
        ))
    })();
    let (config_store, host_store, (gateway_std, gateway_local), (meta_std, meta_local)) =
        match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Err((error, owner)),
        };
    let rt = runtime::runtime();
    let device = runtime::device_id();
    let gateway_host = host.clone();
    let gateway_task = rt.spawn(host_services::supervise(
        "the ACP gateway",
        gateway_local,
        gateway_std,
        move |listener| {
            let host = gateway_host.clone();
            async move { acp::api::serve(listener, host).await }
        },
    ));
    let meta_host = host.clone();
    let meta_task = rt.spawn(host_services::supervise(
        "the ACP meta service",
        meta_local,
        meta_std,
        move |listener| {
            let (store, host_store, uploads) =
                (config_store.clone(), host_store.clone(), uploads.clone());
            let dirs: Arc<dyn SessionDirResolver> = Arc::new(meta_host.clone());
            async move {
                pocket_codex_host_svc::serve_generic(listener, store, host_store, uploads, dirs)
                    .await
            }
        },
    ));
    let abort = || {
        gateway_task.abort();
        meta_task.abort();
    };
    let transport = match transport::resolve_blocking() {
        Ok(transport) => transport,
        Err(error) => {
            abort();
            return Err((error, owner));
        },
    };
    let meta_register =
        host_services::register_service(&transport, &device, ServiceKind::Meta, name, meta_local)
            .ok()
            .flatten();
    let register = match host_services::register_service(
        &transport,
        &device,
        ServiceKind::Acp,
        name,
        gateway_local,
    ) {
        Ok(register) => register,
        Err(error) => {
            abort();
            drop(meta_register);
            return Err((error, owner));
        },
    };
    Ok(AcpServe {
        key: ServiceId::new(&device, ServiceKind::Acp, name).key(),
        meta_key: ServiceId::new(&device, ServiceKind::Meta, name).key(),
        device,
        name: name.to_string(),
        gateway_local,
        meta_local,
        gateway_task,
        meta_task,
        register: Slot {
            published: register,
            revision: 0,
        },
        meta_register: Slot {
            published: meta_register,
            revision: 0,
        },
        host: host.clone(),
        program: program.display().to_string(),
        context: transport::Context::of(&transport),
        config: None,
        launch: None,
        _owner: owner,
    })
}
