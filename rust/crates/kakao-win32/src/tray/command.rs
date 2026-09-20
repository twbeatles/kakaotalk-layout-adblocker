pub const ID_TOGGLE_ENABLED: u32 = 1001;
pub const ID_TOGGLE_AGGRESSIVE: u32 = 1002;
pub const ID_TOGGLE_STARTUP: u32 = 1003;
pub const ID_RESET_RESTORE: u32 = 1004;
pub const ID_OPEN_LOGS: u32 = 1005;
pub const ID_OPEN_RELEASES: u32 = 1006;
pub const ID_CHECK_UPDATE: u32 = 1007;
pub const ID_EXIT: u32 = 1008;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleEnabled,
    ToggleAggressive,
    ToggleStartup,
    ResetRestoreFailures,
    OpenLogs,
    OpenReleases,
    CheckUpdate,
    Exit,
}

impl TrayCommand {
    pub(super) fn from_id(id: u32) -> Option<Self> {
        match id {
            ID_TOGGLE_ENABLED => Some(Self::ToggleEnabled),
            ID_TOGGLE_AGGRESSIVE => Some(Self::ToggleAggressive),
            ID_TOGGLE_STARTUP => Some(Self::ToggleStartup),
            ID_RESET_RESTORE => Some(Self::ResetRestoreFailures),
            ID_OPEN_LOGS => Some(Self::OpenLogs),
            ID_OPEN_RELEASES => Some(Self::OpenReleases),
            ID_CHECK_UPDATE => Some(Self::CheckUpdate),
            ID_EXIT => Some(Self::Exit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_commands() {
        assert_eq!(
            TrayCommand::from_id(ID_TOGGLE_ENABLED),
            Some(TrayCommand::ToggleEnabled)
        );
        assert_eq!(TrayCommand::from_id(ID_EXIT), Some(TrayCommand::Exit));
        assert_eq!(TrayCommand::from_id(0), None);
    }
}
