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
    ///
    /// [`Self::ExtraPath`] is `"extra"` here. Serde still emits
    /// `"extraPath"` (`rename_all = "camelCase"`). Hosts that need a
    /// stable display / list token should call [`Self::wire_name`].
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "user",
            Self::Agents => "agents",
            Self::Vendor { name } => name.as_str(),
            Self::ExtraPath => "extra",
        }
    }

    /// Stable host-facing token. Same as [`Self::as_str`].
    ///
    /// This is not the serde enum token for [`Self::ExtraPath`]
    /// (`"extraPath"`). Some hosts serialize extra roots as `"config"`;
    /// use [`Self::from_host_token`] to accept that.
    pub fn wire_name(&self) -> String {
        self.as_str().to_owned()
    }

    /// Map a host list / TUI token onto a v1 variant.
    ///
    /// Accepts `user`, `agents`, `extra` / `extraPath` / `config`,
    /// and vendor tokens `bline` / `claude` / `cursor` / `grok`
    /// (a leading dot is the on-disk tree: `.claude` is `claude`).
    /// Vendor spellings match [`Self::parse_vendor_token`]: ASCII case
    /// and surrounding whitespace are ignored. `extra` / `user` stay
    /// non-vendor. `project` and `community` have no v1 variant (the
    /// host keeps those).
    pub fn from_host_token(token: &str) -> Option<Self> {
        if let Ok(Some(name)) = Self::parse_vendor_token(token) {
            return Some(Self::Vendor { name });
        }
        match token.trim() {
            "user" => Some(Self::User),
            "agents" => Some(Self::Agents),
            "extra" | "extraPath" | "config" => Some(Self::ExtraPath),
            _ => None,
        }
    }

    /// Frozen v1 vendor tokens (`bline`, `claude`, `cursor`, `grok`).
    pub const VENDOR_TOKENS: &'static [&'static str] = &["bline", "claude", "cursor", "grok"];

    /// Parse one `--vendor` / MCP `vendor` token.
    ///
    /// Empty or whitespace is omitted (same as empty `--path` items).
    /// A leading dot and ASCII case are ignored (`.Claude` is `claude`).
    /// Other values, including `user` / `extra`, are errors.
    pub fn parse_vendor_token(token: &str) -> Result<Option<String>, String> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let stripped = trimmed.strip_prefix('.').unwrap_or(trimmed);
        let lower = stripped.to_ascii_lowercase();
        if Self::VENDOR_TOKENS.contains(&lower.as_str()) {
            return Ok(Some(lower));
        }
        Err(format!(
            "unknown vendor: {token} (use bline, claude, cursor, or grok)"
        ))
    }

    /// Parse a host vendor list. First spelling of each token wins.
    pub fn parse_vendor_roots<I, S>(tokens: I) -> Result<Vec<String>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = Vec::new();
        for token in tokens {
            let Some(name) = Self::parse_vendor_token(token.as_ref())? else {
                continue;
            };
            if !out.iter().any(|e| e == &name) {
                out.push(name);
            }
        }
        Ok(out)
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

    #[test]
    fn extra_as_str_is_not_serde_token() {
        assert_eq!(SkillSource::ExtraPath.as_str(), "extra");
        assert_eq!(SkillSource::ExtraPath.wire_name(), "extra");
        let json = serde_json::to_string(&SkillSource::ExtraPath).expect("ser");
        assert_eq!(json, "\"extraPath\"");
    }

    #[test]
    fn from_host_token_maps_list_and_tui_tokens() {
        assert_eq!(
            SkillSource::from_host_token("user"),
            Some(SkillSource::User)
        );
        assert_eq!(
            SkillSource::from_host_token("agents"),
            Some(SkillSource::Agents)
        );
        assert_eq!(
            SkillSource::from_host_token("extra"),
            Some(SkillSource::ExtraPath)
        );
        assert_eq!(
            SkillSource::from_host_token("extraPath"),
            Some(SkillSource::ExtraPath)
        );
        assert_eq!(
            SkillSource::from_host_token("config"),
            Some(SkillSource::ExtraPath)
        );
        assert_eq!(
            SkillSource::from_host_token("bline"),
            Some(SkillSource::Vendor {
                name: "bline".to_owned()
            })
        );
        assert_eq!(
            SkillSource::from_host_token("claude"),
            Some(SkillSource::Vendor {
                name: "claude".to_owned()
            })
        );
        assert_eq!(
            SkillSource::from_host_token("cursor"),
            Some(SkillSource::Vendor {
                name: "cursor".to_owned()
            })
        );
        assert_eq!(
            SkillSource::from_host_token("grok"),
            Some(SkillSource::Vendor {
                name: "grok".to_owned()
            })
        );
        assert_eq!(SkillSource::from_host_token("project"), None);
        assert_eq!(SkillSource::from_host_token("community"), None);
        assert_eq!(SkillSource::from_host_token("Project"), None);
        assert_eq!(
            SkillSource::from_host_token(".claude"),
            Some(SkillSource::Vendor {
                name: "claude".to_owned()
            })
        );
        assert_eq!(
            SkillSource::from_host_token(".bline"),
            Some(SkillSource::Vendor {
                name: "bline".to_owned()
            })
        );
        assert_eq!(SkillSource::from_host_token(".user"), None);
        assert_eq!(SkillSource::from_host_token(".extra"), None);
        // parse_vendor_roots folds ASCII case and a leading dot. Hosts
        // that pass the same token through from_host_token must land on
        // the same Vendor name, not None.
        for token in [".Claude", "Claude", " CLAUDE ", ".BLINE", "Cursor"] {
            let parsed =
                SkillSource::parse_vendor_roots([token]).unwrap_or_else(|e| panic!("{token}: {e}"));
            assert_eq!(
                SkillSource::from_host_token(token),
                Some(SkillSource::Vendor {
                    name: parsed[0].clone()
                }),
                "from_host_token({token:?}) must match parse_vendor_roots"
            );
        }
    }

    #[test]
    fn parse_vendor_roots_accepts_dot_and_rejects_unknown() {
        assert_eq!(
            SkillSource::parse_vendor_roots([".claude", "Bline", " claude "]).expect("ok"),
            ["claude", "bline"]
        );
        assert!(
            SkillSource::parse_vendor_roots(["", "  "])
                .expect("empty items")
                .is_empty()
        );
        let err = SkillSource::parse_vendor_roots(["nope"]).expect_err("unknown");
        assert!(err.contains("unknown vendor: nope"), "{err}");
        assert!(err.contains("claude"), "{err}");
        let extra = SkillSource::parse_vendor_roots(["extra"]).expect_err("extra");
        assert!(
            extra.contains("unknown vendor: extra"),
            "extra is --path, not a vendor: {extra}"
        );
    }
}
