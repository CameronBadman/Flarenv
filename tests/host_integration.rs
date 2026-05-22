use flarenv::{run_preflight, DaemonConfig};

#[test]
#[ignore = "requires a Linux host with Flarenv host dependencies configured"]
fn host_preflight_passes() {
    let config = DaemonConfig::from_env().unwrap();
    let report = run_preflight(&config);
    assert!(
        report.ok(),
        "host preflight failed: {:?}",
        report
            .checks
            .iter()
            .filter(|check| !check.ok)
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "constructs the real host adapters but does not create subvolumes"]
fn host_control_plane_constructs() {
    let config = DaemonConfig::from_env().unwrap();
    config.build_host_control_plane().unwrap();
}
