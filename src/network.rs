use crate::ids::PolicyId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkPolicy {
    DenyAll { id: PolicyId },
    AllowEgress { id: PolicyId, cidrs: Vec<String> },
}

impl NetworkPolicy {
    pub fn id(&self) -> &PolicyId {
        match self {
            Self::DenyAll { id } | Self::AllowEgress { id, .. } => id,
        }
    }
}
