# Flarenv

Flarenv is a Nix-based environment manager for agent workspaces.

The daemon is responsible for:

- SSH-compatible session routing into agent workspaces.
- btrfs-backed writable roots with snapshots and branches.
- `systemd-nspawn` execution with cgroup limits and network policy.
- A fixed read-only Nix toolset shared across all environments.

This repository currently contains the Rust control-plane foundation:

- Workspace lifecycle types and API.
- Storage and executor adapter traits.
- In-memory implementations for deterministic tests.
- Host command builders for btrfs and `systemd-nspawn` integration.

## Development

```sh
cargo test
cargo run -- --help
nix develop
nix build
```

The daemon scaffold intentionally does not require root for tests. Real host
integration should be wired through the adapter traits in `src/storage.rs` and
`src/executor.rs`.
