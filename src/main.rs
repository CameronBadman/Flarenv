use flarenv::{
    ControlPlane, FixedNixProfile, InMemoryStorage, NetworkPolicy, NspawnExecutor, PolicyId,
};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return;
    }

    if let Err(err) = initialize() {
        eprintln!("flarenvd: {err}");
        std::process::exit(1);
    }
}

fn initialize() -> flarenv::Result<()> {
    let _control_plane = ControlPlane::new(
        InMemoryStorage::new("/var/lib/flarenv"),
        NspawnExecutor::new("flarenv"),
        FixedNixProfile::default(),
        NetworkPolicy::DenyAll {
            id: PolicyId::new("deny")?,
        },
    )?;

    println!("flarenvd control plane initialized");
    println!("persistent daemon, ssh frontend, and host adapters are not wired yet");
    Ok(())
}

fn print_help() {
    println!("flarenvd");
    println!();
    println!("USAGE:");
    println!("    flarenvd [--help]");
    println!();
    println!("The current binary initializes the control-plane scaffold.");
}
