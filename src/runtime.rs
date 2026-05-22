use crate::ids::{SessionId, WorkspaceId};
use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlEvent {
    SessionStarted {
        workspace_id: WorkspaceId,
        session_id: SessionId,
    },
    SessionExited {
        workspace_id: WorkspaceId,
        session_id: SessionId,
        code: i32,
    },
    Tick,
    Shutdown,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeStats {
    pub active_sessions: usize,
    pub exited_sessions: u64,
    pub ticks: u64,
}

#[derive(Debug, Default)]
pub struct SessionRuntime {
    active_sessions: BTreeSet<(WorkspaceId, SessionId)>,
    exited_sessions: u64,
    ticks: u64,
}

impl SessionRuntime {
    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            active_sessions: self.active_sessions.len(),
            exited_sessions: self.exited_sessions,
            ticks: self.ticks,
        }
    }

    pub fn apply(&mut self, event: ControlEvent) -> bool {
        match event {
            ControlEvent::SessionStarted {
                workspace_id,
                session_id,
            } => {
                self.active_sessions.insert((workspace_id, session_id));
                true
            }
            ControlEvent::SessionExited {
                workspace_id,
                session_id,
                code: _,
            } => {
                self.active_sessions.remove(&(workspace_id, session_id));
                self.exited_sessions += 1;
                true
            }
            ControlEvent::Tick => {
                self.ticks += 1;
                true
            }
            ControlEvent::Shutdown => false,
        }
    }

    pub fn run_blocking(
        &mut self,
        receiver: &Receiver<ControlEvent>,
        tick_interval: Duration,
    ) -> RuntimeStats {
        loop {
            match receiver.recv_timeout(tick_interval) {
                Ok(event) => {
                    if !self.apply(event) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.apply(ControlEvent::Tick);
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        self.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlEvent, SessionRuntime};
    use crate::ids::{SessionId, WorkspaceId};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn tracks_active_sessions_from_events() {
        let mut runtime = SessionRuntime::default();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        let session_id = SessionId::new("session_a").unwrap();

        assert!(runtime.apply(ControlEvent::SessionStarted {
            workspace_id: workspace_id.clone(),
            session_id: session_id.clone(),
        }));
        assert_eq!(runtime.stats().active_sessions, 1);

        assert!(runtime.apply(ControlEvent::SessionExited {
            workspace_id,
            session_id,
            code: 0,
        }));
        assert_eq!(runtime.stats().active_sessions, 0);
        assert_eq!(runtime.stats().exited_sessions, 1);
    }

    #[test]
    fn blocking_loop_ticks_without_busy_polling() {
        let (sender, receiver) = mpsc::channel();
        sender.send(ControlEvent::Shutdown).unwrap();
        let mut runtime = SessionRuntime::default();

        let stats = runtime.run_blocking(&receiver, Duration::from_secs(60));

        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.ticks, 0);
    }
}
