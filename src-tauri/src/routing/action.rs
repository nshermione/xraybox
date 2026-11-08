use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Direct,
    Proxy,
    Block,
}

impl RuleAction {
    pub fn outbound_tag(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Proxy => "proxy",
            Self::Block => "block",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_tags() {
        assert_eq!(RuleAction::Direct.outbound_tag(), "direct");
        assert_eq!(RuleAction::Proxy.outbound_tag(), "proxy");
        assert_eq!(RuleAction::Block.outbound_tag(), "block");
    }
}
