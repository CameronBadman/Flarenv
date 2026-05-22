use flarenv::{
    measure_usage, plan_gc, run_preflight, AllowListAuthorizer, DaemonConfig, FileMetadataStore,
    FlarenvSshServer, GcPolicy, PathUsageProbe, SshWireConfig, WorkspaceId,
};
use std::env;
use std::net::SocketAddr;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "check-host") {
        check_host();
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "usage") {
        usage(args.get(2));
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "serve-ssh") {
        serve_ssh(args.get(2), args.get(3), args.get(4));
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "gc-plan") {
        gc_plan(args.get(2));
        return;
    }

    if let Err(err) = initialize() {
        eprintln!("flarenvd: {err}");
        std::process::exit(1);
    }
}

fn initialize() -> flarenv::Result<()> {
    let config = DaemonConfig::from_env()?;
    let _control_plane = config.build_host_control_plane()?;

    println!("flarenvd control plane initialized");
    println!("state root: {}", config.state_root.display());
    println!("nix store: {}", config.nix_profile.store_path.display());
    println!("nix profile: {}", config.nix_profile.profile_path.display());
    println!("persistent daemon, ssh frontend, and host adapters are not wired yet");
    Ok(())
}

fn serve_ssh(addr: Option<&String>, workspace: Option<&String>, authorized_key: Option<&String>) {
    let (Some(addr), Some(workspace), Some(authorized_key)) = (addr, workspace, authorized_key)
    else {
        eprintln!("flarenvd: serve-ssh requires <addr> <workspace-id> <authorized-key>");
        std::process::exit(2);
    };
    let addr: SocketAddr = match addr.parse() {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("flarenvd: invalid listen addr: {err}");
            std::process::exit(2);
        }
    };
    let workspace_id = match WorkspaceId::new(workspace) {
        Ok(workspace_id) => workspace_id,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(2);
        }
    };
    let config = match DaemonConfig::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(1);
        }
    };
    let control_plane = match config.build_host_control_plane() {
        Ok(control_plane) => control_plane,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(1);
        }
    };
    let mut authorizer = AllowListAuthorizer::default();
    authorizer.grant(authorized_key, workspace_id.clone());
    let mut server = FlarenvSshServer::new(
        SshWireConfig::new(addr, workspace_id),
        control_plane,
        authorizer,
    );
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("flarenvd: failed to build async runtime: {err}");
            std::process::exit(1);
        }
    };
    if let Err(err) = runtime.block_on(server.run()) {
        eprintln!("flarenvd: {err}");
        std::process::exit(1);
    }
}

fn gc_plan(metadata_path: Option<&String>) {
    let Some(metadata_path) = metadata_path else {
        eprintln!("flarenvd: gc-plan requires a metadata path");
        std::process::exit(2);
    };
    let store = FileMetadataStore::new(metadata_path);
    let metadata = match store.load() {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(1);
        }
    };
    for action in plan_gc(
        &metadata,
        std::time::SystemTime::now(),
        &GcPolicy::default(),
    ) {
        println!("{action:?}");
    }
}

fn usage(metadata_path: Option<&String>) {
    let Some(metadata_path) = metadata_path else {
        eprintln!("flarenvd: usage requires a metadata path");
        std::process::exit(2);
    };
    let store = FileMetadataStore::new(metadata_path);
    let metadata = match store.load() {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(1);
        }
    };
    let report = match measure_usage(&metadata, &PathUsageProbe) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(1);
        }
    };

    println!(
        "total\tworkspaces={}\tready={}\tdeleted={}\tlogical_bytes={}\tretained_bytes={}\tquota_bytes={}\tmemory_limit_bytes={}\tpids_limit={}",
        report.workspaces.len(),
        report.ready_workspaces,
        report.deleted_workspaces,
        report.logical_bytes,
        optional_bytes(report.retained_bytes),
        report.quota_bytes,
        report.memory_limit_bytes,
        report.pids_limit
    );
    for workspace in report.workspaces {
        println!(
            "workspace\t{}\tstate={:?}\tlogical_bytes={}\tretained_bytes={}\tquota_bytes={}\tmemory_limit_bytes={}\tpids_limit={}",
            workspace.workspace_id,
            workspace.state,
            workspace.logical_bytes,
            optional_bytes(workspace.retained_bytes),
            workspace.disk_quota_bytes,
            workspace.memory_limit_bytes,
            workspace.pids_limit
        );
    }
}

fn optional_bytes(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn check_host() {
    let config = match DaemonConfig::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("flarenvd: {err}");
            std::process::exit(1);
        }
    };

    let report = run_preflight(&config);
    for check in &report.checks {
        let status = if check.ok { "ok" } else { "fail" };
        println!("{status}\t{}\t{}", check.name, check.detail);
    }

    if !report.ok() {
        std::process::exit(1);
    }
}

fn print_help() {
    println!("flarenvd");
    println!();
    println!("USAGE:");
    println!("    flarenvd [--help]");
    println!("    flarenvd check-host");
    println!("    flarenvd gc-plan <metadata-path>");
    println!("    flarenvd serve-ssh <addr> <workspace-id> <authorized-key>");
    println!("    flarenvd usage <metadata-path>");
    println!();
    println!("ENV:");
    println!("    FLARENV_STATE_ROOT       default /var/lib/flarenv");
    println!("    FLARENV_NIX_STORE        default /nix/store");
    println!("    FLARENV_NIX_PROFILE      default /nix/var/nix/profiles/flarenv/global");
    println!("    FLARENV_MACHINE_PREFIX   default flarenv");
    println!();
    println!("The current binary initializes the control-plane scaffold.");
}
