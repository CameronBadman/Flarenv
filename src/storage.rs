use crate::error::{FlarenvError, Result};
use crate::ids::{SnapshotId, WorkspaceId};
use crate::layout::initialize_workspace_root;
use crate::model::ResourceLimits;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub trait StorageBackend {
    fn create_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<PathBuf>;
    fn clone_workspace(
        &mut self,
        snapshot_id: &SnapshotId,
        workspace_id: &WorkspaceId,
    ) -> Result<PathBuf>;
    fn snapshot_workspace(
        &mut self,
        workspace_id: &WorkspaceId,
        snapshot_id: &SnapshotId,
    ) -> Result<PathBuf>;
    fn delete_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<()>;
    fn set_quota(&mut self, workspace_id: &WorkspaceId, limits: &ResourceLimits) -> Result<()>;
}

#[derive(Debug)]
pub struct InMemoryStorage {
    root: PathBuf,
    workspaces: BTreeMap<WorkspaceId, PathBuf>,
    snapshots: BTreeMap<SnapshotId, PathBuf>,
}

impl InMemoryStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            workspaces: BTreeMap::new(),
            snapshots: BTreeMap::new(),
        }
    }
}

impl StorageBackend for InMemoryStorage {
    fn create_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<PathBuf> {
        if self.workspaces.contains_key(workspace_id) {
            return Err(FlarenvError::AlreadyExists(format!(
                "workspace {workspace_id}"
            )));
        }

        let path = self.root.join("workspaces").join(workspace_id.as_str());
        self.workspaces.insert(workspace_id.clone(), path.clone());
        Ok(path)
    }

    fn clone_workspace(
        &mut self,
        snapshot_id: &SnapshotId,
        workspace_id: &WorkspaceId,
    ) -> Result<PathBuf> {
        if !self.snapshots.contains_key(snapshot_id) {
            return Err(FlarenvError::NotFound(format!("snapshot {snapshot_id}")));
        }
        self.create_workspace(workspace_id)
    }

    fn snapshot_workspace(
        &mut self,
        workspace_id: &WorkspaceId,
        snapshot_id: &SnapshotId,
    ) -> Result<PathBuf> {
        if !self.workspaces.contains_key(workspace_id) {
            return Err(FlarenvError::NotFound(format!("workspace {workspace_id}")));
        }
        if self.snapshots.contains_key(snapshot_id) {
            return Err(FlarenvError::AlreadyExists(format!(
                "snapshot {snapshot_id}"
            )));
        }

        let path = self.root.join("snapshots").join(snapshot_id.as_str());
        self.snapshots.insert(snapshot_id.clone(), path.clone());
        Ok(path)
    }

    fn delete_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<()> {
        if self.workspaces.remove(workspace_id).is_none() {
            return Err(FlarenvError::NotFound(format!("workspace {workspace_id}")));
        }
        Ok(())
    }

    fn set_quota(&mut self, workspace_id: &WorkspaceId, _limits: &ResourceLimits) -> Result<()> {
        if !self.workspaces.contains_key(workspace_id) {
            return Err(FlarenvError::NotFound(format!("workspace {workspace_id}")));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct BtrfsStorage {
    root: PathBuf,
}

impl BtrfsStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn workspace_path(&self, workspace_id: &WorkspaceId) -> PathBuf {
        self.root.join("workspaces").join(workspace_id.as_str())
    }

    pub fn snapshot_path(&self, snapshot_id: &SnapshotId) -> PathBuf {
        self.root.join("snapshots").join(snapshot_id.as_str())
    }

    pub fn create_workspace_command(&self, workspace_id: &WorkspaceId) -> Command {
        let mut command = Command::new("btrfs");
        command
            .arg("subvolume")
            .arg("create")
            .arg(self.workspace_path(workspace_id));
        command
    }

    pub fn snapshot_command(
        &self,
        workspace_id: &WorkspaceId,
        snapshot_id: &SnapshotId,
    ) -> Command {
        let mut command = Command::new("btrfs");
        command
            .arg("subvolume")
            .arg("snapshot")
            .arg("-r")
            .arg(self.workspace_path(workspace_id))
            .arg(self.snapshot_path(snapshot_id));
        command
    }

    pub fn clone_command(&self, snapshot_id: &SnapshotId, workspace_id: &WorkspaceId) -> Command {
        let mut command = Command::new("btrfs");
        command
            .arg("subvolume")
            .arg("snapshot")
            .arg(self.snapshot_path(snapshot_id))
            .arg(self.workspace_path(workspace_id));
        command
    }

    pub fn delete_command(path: &Path) -> Command {
        let mut command = Command::new("btrfs");
        command.arg("subvolume").arg("delete").arg(path);
        command
    }

    pub fn quota_command(&self, workspace_id: &WorkspaceId, bytes: u64) -> Command {
        let mut command = Command::new("btrfs");
        command
            .arg("qgroup")
            .arg("limit")
            .arg(bytes.to_string())
            .arg(self.workspace_path(workspace_id));
        command
    }

    fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.root.join("workspaces"))?;
        fs::create_dir_all(self.root.join("snapshots"))?;
        Ok(())
    }
}

impl StorageBackend for BtrfsStorage {
    fn create_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<PathBuf> {
        self.ensure_layout()?;
        let path = self.workspace_path(workspace_id);
        run_command(self.create_workspace_command(workspace_id))?;
        initialize_workspace_root(&path)?;
        Ok(path)
    }

    fn clone_workspace(
        &mut self,
        snapshot_id: &SnapshotId,
        workspace_id: &WorkspaceId,
    ) -> Result<PathBuf> {
        self.ensure_layout()?;
        let path = self.workspace_path(workspace_id);
        run_command(self.clone_command(snapshot_id, workspace_id))?;
        initialize_workspace_root(&path)?;
        Ok(path)
    }

    fn snapshot_workspace(
        &mut self,
        workspace_id: &WorkspaceId,
        snapshot_id: &SnapshotId,
    ) -> Result<PathBuf> {
        self.ensure_layout()?;
        let path = self.snapshot_path(snapshot_id);
        run_command(self.snapshot_command(workspace_id, snapshot_id))?;
        Ok(path)
    }

    fn delete_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<()> {
        run_command(Self::delete_command(&self.workspace_path(workspace_id)))
    }

    fn set_quota(&mut self, workspace_id: &WorkspaceId, limits: &ResourceLimits) -> Result<()> {
        run_command(self.quota_command(workspace_id, limits.disk_max_bytes))
    }
}

fn run_command(mut command: Command) -> Result<()> {
    let program = command.get_program().to_owned();
    let args: Vec<_> = command.get_args().map(|arg| arg.to_owned()).collect();
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(FlarenvError::Storage(format!(
        "{} {} failed with status {}: {}",
        display_os(&program),
        args.iter()
            .map(|arg| display_os(arg.as_os_str()))
            .collect::<Vec<_>>()
            .join(" "),
        output.status,
        stderr.trim()
    )))
}

fn display_os(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::BtrfsStorage;
    use crate::ids::{SnapshotId, WorkspaceId};

    #[test]
    fn builds_btrfs_snapshot_paths() {
        let storage = BtrfsStorage::new("/var/lib/flarenv");
        let workspace = WorkspaceId::new("agent_a").unwrap();
        let snapshot = SnapshotId::new("snap_1").unwrap();

        assert_eq!(
            storage.workspace_path(&workspace).to_string_lossy(),
            "/var/lib/flarenv/workspaces/agent_a"
        );
        assert_eq!(
            storage.snapshot_path(&snapshot).to_string_lossy(),
            "/var/lib/flarenv/snapshots/snap_1"
        );
    }
}
