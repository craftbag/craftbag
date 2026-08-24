//! Host-neutral skill origin. No `Project` variant in v1.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Where a skill was loaded from.
///
/// Repo-local `.agents/skills` is [`Self::Agents`]. Opt-in vendor trees
/// (`.bline`, `.claude`, `.cursor`, `.grok`) are [`Self::Vendor`]. Extra
/// `--paths` are [`Self::ExtraPath`]. A host-supplied user dir is
/// [`Self::User`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    /// Host-supplied user skills directory.
    User,
    /// Portable `.agents/skills` tree (repo walk or `$HOME`).
    Agents,
    /// Opt-in vendor tree. `name` is a host token: `bline`, `claude`,
    /// `cursor`, or `grok`.
    Vendor { name: String },
    /// Extra path from the host (`DiscoveryOptions.paths` later).
    ExtraPath,
}

impl SkillSource {
    /// Wire/label name. Unit variants are fixed; vendor uses `name`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "user",
            Self::Agents => "agents",
            Self::Vendor { name } => name.as_str(),
            Self::ExtraPath => "extra",
        }
    }
}

impl fmt::Display for SkillSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::SkillSource;

    #[test]
    fn as_str_labels() {
        assert_eq!(SkillSource::User.as_str(), "user");
        assert_eq!(SkillSource::Agents.as_str(), "agents");
        assert_eq!(SkillSource::ExtraPath.as_str(), "extra");
        assert_eq!(
            SkillSource::Vendor {
                name: "bline".to_owned()
            }
            .as_str(),
            "bline"
        );
        assert_eq!(
            SkillSource::Vendor {
                name: "claude".to_owned()
            }
            .as_str(),
            "claude"
        );
    }

    #[test]
    fn serde_unit_variants_are_camel_case_strings() {
        let user = serde_json::to_string(&SkillSource::User).expect("ser");
        let agents = serde_json::to_string(&SkillSource::Agents).expect("ser");
        let extra = serde_json::to_string(&SkillSource::ExtraPath).expect("ser");
        assert_eq!(user, "\"user\"");
        assert_eq!(agents, "\"agents\"");
        assert_eq!(extra, "\"extraPath\"");

        assert_eq!(
            serde_json::from_str::<SkillSource>(&user).expect("de"),
            SkillSource::User
        );
        assert_eq!(
            serde_json::from_str::<SkillSource>(&agents).expect("de"),
            SkillSource::Agents
        );
        assert_eq!(
            serde_json::from_str::<SkillSource>(&extra).expect("de"),
            SkillSource::ExtraPath
        );
    }

    #[test]
    fn serde_vendor_is_externally_tagged() {
        let src = SkillSource::Vendor {
            name: "cursor".to_owned(),
        };
        let json = serde_json::to_string(&src).expect("ser");
        assert_eq!(json, r#"{"vendor":{"name":"cursor"}}"#);
        let back: SkillSource = serde_json::from_str(&json).expect("de");
        assert_eq!(back, src);
    }

    #[test]
    fn v1_has_no_project_variant() {
        // Exhaustive match is the contract: adding Project fails to compile.
        let src = SkillSource::Agents;
        match src {
            SkillSource::User
            | SkillSource::Agents
            | SkillSource::Vendor { .. }
            | SkillSource::ExtraPath => {}
        }
    }
}
