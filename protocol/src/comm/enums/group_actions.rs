use std::fmt::Display;

#[derive(Debug, Eq, PartialEq, Clone)]
/// Used for Determining group actions that can be taken on a group member
pub enum GroupOptions {
    /// Used to promote a member to admin
    PromoteMember,
    /// Used to demote a member from admin
    DemoteMember,
    /// Used to remove a member from a group
    RemoveMember,
}

impl Display for GroupOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroupOptions::PromoteMember => write!(f, "Promote to Admin"),
            GroupOptions::DemoteMember => write!(f, "Demote to Member"),
            GroupOptions::RemoveMember => write!(f, "Remove Member"),
        }
    }
}

impl GroupOptions {
    #[must_use]
    pub fn iterator() -> Vec<Self> {
        vec![Self::PromoteMember, Self::DemoteMember, Self::RemoveMember]
    }
}
