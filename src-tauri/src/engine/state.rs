use serde::Serialize;

use crate::engine::error::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl EngineStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineStatus::Stopped => "stopped",
            EngineStatus::Starting => "starting",
            EngineStatus::Running => "running",
            EngineStatus::Stopping => "stopping",
            EngineStatus::Failed => "failed",
        }
    }
}

impl Default for EngineStatus {
    fn default() -> Self {
        EngineStatus::Stopped
    }
}

#[derive(Debug, Default)]
pub struct EngineStateMachine {
    status: EngineStatus,
}

impl EngineStateMachine {
    pub fn status(&self) -> EngineStatus {
        self.status
    }

    pub fn begin_start(&mut self) -> Result<(), EngineError> {
        match self.status {
            EngineStatus::Stopped | EngineStatus::Failed => {
                self.status = EngineStatus::Starting;
                Ok(())
            }
            other => Err(EngineError::InvalidState {
                from: other.as_str().into(),
                attempted: "start".into(),
            }),
        }
    }

    pub fn mark_running(&mut self) -> Result<(), EngineError> {
        match self.status {
            EngineStatus::Starting => {
                self.status = EngineStatus::Running;
                Ok(())
            }
            other => Err(EngineError::InvalidState {
                from: other.as_str().into(),
                attempted: "mark_running".into(),
            }),
        }
    }

    pub fn begin_reload(&mut self) -> Result<(), EngineError> {
        match self.status {
            EngineStatus::Running => {
                self.status = EngineStatus::Starting;
                Ok(())
            }
            other => Err(EngineError::InvalidState {
                from: other.as_str().into(),
                attempted: "reload".into(),
            }),
        }
    }

    pub fn begin_stop(&mut self) -> Result<(), EngineError> {
        match self.status {
            EngineStatus::Running | EngineStatus::Starting => {
                self.status = EngineStatus::Stopping;
                Ok(())
            }
            other => Err(EngineError::InvalidState {
                from: other.as_str().into(),
                attempted: "stop".into(),
            }),
        }
    }

    pub fn mark_stopped(&mut self) {
        self.status = EngineStatus::Stopped;
    }

    pub fn mark_failed(&mut self) {
        self.status = EngineStatus::Failed;
    }

    pub fn adopt_running(&mut self) {
        self.status = EngineStatus::Running;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub status: EngineStatus,
    pub pid: Option<u32>,
    pub last_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_from_stopped_goes_starting_then_running_on_ack() {
        let mut sm = EngineStateMachine::default();
        assert_eq!(sm.status(), EngineStatus::Stopped);
        sm.begin_start().unwrap();
        assert_eq!(sm.status(), EngineStatus::Starting);
        sm.mark_running().unwrap();
        assert_eq!(sm.status(), EngineStatus::Running);
    }

    #[test]
    fn cannot_start_while_running() {
        let mut sm = EngineStateMachine::default();
        sm.begin_start().unwrap();
        sm.mark_running().unwrap();
        assert!(sm.begin_start().is_err());
    }

    #[test]
    fn stop_from_running_goes_stopping_then_stopped() {
        let mut sm = EngineStateMachine::default();
        sm.begin_start().unwrap();
        sm.mark_running().unwrap();
        sm.begin_stop().unwrap();
        sm.mark_stopped();
        assert_eq!(sm.status(), EngineStatus::Stopped);
    }

    #[test]
    fn stop_from_starting_goes_stopping_then_stopped() {
        let mut sm = EngineStateMachine::default();
        sm.begin_start().unwrap();
        sm.begin_stop().unwrap();
        sm.mark_stopped();
        assert_eq!(sm.status(), EngineStatus::Stopped);
    }

    #[test]
    fn reload_from_running_goes_starting_then_running() {
        let mut sm = EngineStateMachine::default();
        sm.begin_start().unwrap();
        sm.mark_running().unwrap();
        sm.begin_reload().unwrap();
        assert_eq!(sm.status(), EngineStatus::Starting);
        sm.mark_running().unwrap();
        assert_eq!(sm.status(), EngineStatus::Running);
    }

    #[test]
    fn cannot_reload_unless_running() {
        let mut sm = EngineStateMachine::default();
        assert!(sm.begin_reload().is_err());
        sm.begin_start().unwrap();
        assert!(sm.begin_reload().is_err());
    }

    #[test]
    fn adopt_running_from_stopped_after_ui_reopen() {
        let mut sm = EngineStateMachine::default();
        sm.adopt_running();
        assert_eq!(sm.status(), EngineStatus::Running);
    }
}
