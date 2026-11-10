//! Persisted engine identity. Legacy profiles migrate to the standalone engine.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineKind {
    #[default]
    #[serde(alias = "Servo")]
    Wega,
}
impl EngineKind {
    pub const fn name(self) -> &'static str {
        "Wega"
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "wega" => Ok(Self::Wega),
            _ => Err("Only the Wega engine is supported".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_identity_migrates_without_losing_the_profile() {
        assert_eq!(
            serde_json::from_str::<EngineKind>("\"Servo\"").unwrap(),
            EngineKind::Wega
        );
        assert_eq!(
            serde_json::to_string(&EngineKind::Wega).unwrap(),
            "\"Wega\""
        );
        assert!(EngineKind::parse("servo").is_err());
    }
}
