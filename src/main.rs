use flarenv::{run_preflight, DaemonConfig};
use std::env;

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
    println!();
    println!("ENV:");
    println!("    FLARENV_STATE_ROOT       default /var/lib/flarenv");
    println!("    FLARENV_NIX_STORE        default /nix/store");
    println!("    FLARENV_NIX_PROFILE      default /nix/var/nix/profiles/flarenv/global");
    println!("    FLARENV_MACHINE_PREFIX   default flarenv");
    println!();
    println!("The current binary initializes the control-plane scaffold.");
}
