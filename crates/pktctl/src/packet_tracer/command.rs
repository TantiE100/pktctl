use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Ok,
    Ambiguous,
    Invalid,
    Incomplete,
    NotImplemented,
}

impl CommandStatus {
    pub(crate) fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => Self::Ok,
            1 => Self::Ambiguous,
            2 => Self::Invalid,
            3 => Self::Incomplete,
            4 => Self::NotImplemented,
            _ => return None,
        })
    }

    /// What IOS prints for a rejected command; Packet Tracer's `enterCommand` reply
    /// carries only the status.
    pub(crate) fn explanation(self) -> &'static str {
        match self {
            Self::Ok => "",
            Self::Ambiguous => "% Ambiguous command",
            Self::Invalid => "% Invalid input detected",
            Self::Incomplete => "% Incomplete command.",
            Self::NotImplemented => "Packet Tracer does not implement this command",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_documented_status() {
        let statuses: Vec<_> = (0..=4).filter_map(CommandStatus::from_code).collect();
        assert_eq!(
            statuses,
            [
                CommandStatus::Ok,
                CommandStatus::Ambiguous,
                CommandStatus::Invalid,
                CommandStatus::Incomplete,
                CommandStatus::NotImplemented
            ]
        );
        assert_eq!(CommandStatus::from_code(5), None);
    }
}
