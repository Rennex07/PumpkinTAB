//! Which group a player is shown as.
//!
//! The rule is one sentence: **the first group, in descending `order`, that the
//! player qualifies for.** A player in no group gets the default group.
//!
//! ## How membership is decided
//!
//! `has-permission-set` answers with three states, not two, and all three are
//! used. Collapsing them into a boolean is how a group silently stops working:
//!
//! | Answer | Meaning | This plugin |
//! |:--|:--|:--|
//! | `Some(true)` | explicitly granted | in the group |
//! | `Some(false)` | explicitly denied | not in it, keep looking |
//! | `None` | never set, so it would inherit | not a member; see below |
//!
//! The `None` case is where `op-level-groups` comes in: an op level that maps to
//! a group qualifies the player for it. That is the only way to be in a group
//! without being granted its node.
//!
//! ## Why nothing here talks to another plugin
//!
//! There is no service registry in the WIT, so one plugin cannot hand another an
//! object. The node name is built from a *string* in this plugin's configuration
//! and the permission manager is asked about it, so the other plugin only has to
//! grant a node with that name. It does not have to exist at all.

use pumpkin_plugin_api::permission::PermissionLevel;
use pumpkin_plugin_api::player::Player;

use crate::config::{Config, MinOpLevel};

/// The host's op-level enum for a configured level.
pub fn level_of(level: MinOpLevel) -> PermissionLevel {
    match level {
        MinOpLevel::Zero => PermissionLevel::Zero,
        MinOpLevel::One => PermissionLevel::One,
        MinOpLevel::Two => PermissionLevel::Two,
        MinOpLevel::Three => PermissionLevel::Three,
        MinOpLevel::Four => PermissionLevel::Four,
    }
}

/// Why a player is in a group, for a log line and for `/tab check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The group's node is granted to them.
    Granted,
    /// They hold at least the op level mapped to this group.
    OpLevel,
}

impl Reason {
    /// The wording `/tab check` prints.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::OpLevel => "op level",
        }
    }
}

/// The group a player is shown as, its position in the list, and why.
///
/// The position is the sort order: the first configured group is the highest.
/// [`None`] for the reason means the player is in no group and has the default.
pub struct Resolved<'a> {
    /// The group's name, or the default group's.
    pub name: &'a str,
    /// Index into [`Config::groups`], or [`None`] for the default group.
    pub index: Option<usize>,
    /// Why they are in it, or [`None`] for the default group.
    pub reason: Option<Reason>,
}

/// Which group a player is shown as.
///
/// The first group in the configured order that they qualify for, then the
/// default group. The order is the whole rule, and it is the config's list order.
pub fn resolve<'a>(config: &'a Config, player: &Player) -> Resolved<'a> {
    let level = player.get_permission_level();

    for (index, name) in config.groups_by_order().iter().enumerate() {
        let Some(node) = config.node_for(name) else {
            // Reported by `Config::problems`; here it just cannot match, so one
            // unusable name does not stop the rest of the ladder.
            continue;
        };
        if player.has_permission_set(&node) == Some(true) {
            return Resolved {
                name,
                index: Some(index),
                reason: Some(Reason::Granted),
            };
        }
    }

    // No node granted. An op level can still put them somewhere, which is how an
    // operator shows as an admin without anyone having run `/pumpkinperm set`.
    for (configured, name) in &config.op_level_groups {
        if level >= level_of(*configured)
            && let Some(index) = config.groups_by_order().iter().position(|listed| listed == name)
        {
            return Resolved {
                name,
                index: Some(index),
                reason: Some(Reason::OpLevel),
            };
        }
    }

    // Nobody qualified, so the fallback. `default` is normally in the list and would
    // already have matched through its own node; this covers a player PumpkinPerm
    // has never heard of.
    Resolved {
        name: crate::config::DEFAULT_GROUP,
        index: config
            .groups_by_order()
            .iter()
            .position(|listed| listed.eq_ignore_ascii_case(crate::config::DEFAULT_GROUP)),
        reason: None,
    }
}

/// The sort value to push for a player.
///
/// Descending from the group count, so the first configured group is the largest
/// value and lands at the top.
pub fn sort_value(config: &Config, resolved: &Resolved<'_>) -> i32 {
    match resolved.index {
        Some(index) => config.sort_value(index),
        // No `default` in the list at all, which `Config::problems` reports. Sit
        // below everything rather than colliding with the bottom group.
        None => 0,
    }
}

/// Whether a group name is usable.
///
/// A group name goes into a permission node and into log lines, so the character
/// set is restricted to what is safe and readable in both.
pub fn name_problem(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return Some("the name is empty".to_string());
    }
    if name.len() > 64 {
        return Some(format!("the name is {} characters, the limit is 64", name.len()));
    }
    if let Some(bad) = name
        .bytes()
        .find(|b| !(b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_' || *b == b'.'))
    {
        return Some(format!(
            "the name contains {:?}, which cannot appear in a permission node; use letters, \
             digits, '-', '_' and '.'",
            bad as char
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::parse(
            "groups: [owner, admin, vip, default]\n\
             op-level-groups:\n  four: owner\n  two: admin\n",
        )
        .expect("valid")
    }

    #[test]
    fn the_list_order_is_the_order() {
        assert_eq!(config().groups_by_order(), ["owner", "admin", "vip", "default"]);
    }

    #[test]
    fn a_group_is_detected_by_pumpkinperms_own_node() {
        assert_eq!(
            config().node_for("owner").as_deref(),
            Some("PumpkinPerm:group.owner")
        );
    }

    /// A name with a space or a colon could never be part of a node, so building
    /// one would ask about something nobody can be granted.
    #[test]
    fn an_unusable_group_name_has_no_node() {
        assert_eq!(config().node_for("has space"), None);
        assert_eq!(config().node_for("has:colon"), None);
    }

    #[test]
    fn the_first_group_sorts_highest() {
        let config = config();
        assert_eq!(config.sort_value(0), 4);
        assert_eq!(config.sort_value(1), 3);
        assert_eq!(config.sort_value(3), 1);
    }

    #[test]
    fn an_empty_list_does_not_produce_a_negative_sort_value() {
        let config = Config::parse("groups: []\n").expect("valid");
        assert_eq!(config.sort_value(0), 1);
    }

    #[test]
    fn a_group_listed_twice_is_reported() {
        let config = Config::parse("groups: [vip, vip]\n").expect("valid");
        assert!(
            config
                .problems()
                .iter()
                .any(|problem| problem.contains("twice")),
            "{:?}",
            config.problems()
        );
    }

    /// The mapping is a trap if the group is not in the list: nothing would ever
    /// qualify, and the section would look like it worked.
    #[test]
    fn an_op_level_pointing_at_an_unlisted_group_is_reported() {
        let config =
            Config::parse("groups: [vip]\nop-level-groups:\n  four: owner\n").expect("valid");
        assert!(
            config
                .problems()
                .iter()
                .any(|problem| problem.contains("not in `groups`")),
            "{:?}",
            config.problems()
        );
    }

    /// With no `default` anywhere, a player in no group has nothing to fall back to.
    #[test]
    fn a_missing_default_group_is_reported() {
        let config = Config::parse("groups: [vip, admin]\n").expect("valid");
        assert!(
            config
                .problems()
                .iter()
                .any(|problem| problem.contains("no 'default'")),
            "{:?}",
            config.problems()
        );
    }

    /// LuckPerms and every plugin that copies it spell it `default`, and
    /// PumpkinPerm grants `PumpkinPerm:group.default` to everybody, so anything
    /// else would never match.
    #[test]
    fn a_misspelled_default_group_is_reported() {
        let config = Config::parse("groups: [vip, Default]\n").expect("valid");
        assert!(
            config
                .problems()
                .iter()
                .any(|problem| problem.contains("should be written 'default'")),
            "{:?}",
            config.problems()
        );
    }

    #[test]
    fn op_levels_map_onto_the_host_enum() {
        assert_eq!(level_of(MinOpLevel::Zero), PermissionLevel::Zero);
        assert_eq!(level_of(MinOpLevel::One), PermissionLevel::One);
        assert_eq!(level_of(MinOpLevel::Two), PermissionLevel::Two);
        assert_eq!(level_of(MinOpLevel::Three), PermissionLevel::Three);
        assert_eq!(level_of(MinOpLevel::Four), PermissionLevel::Four);
    }

    #[test]
    fn a_reason_has_readable_wording() {
        assert_eq!(Reason::Granted.as_str(), "granted");
        assert_eq!(Reason::OpLevel.as_str(), "op level");
    }

    #[test]
    fn names_are_checked_against_what_a_node_can_hold() {
        assert_eq!(name_problem("admin"), None);
        assert_eq!(name_problem("junior-mod_2"), None);
        assert!(name_problem("").is_some());
        assert!(name_problem("   ").is_some());
        assert!(name_problem("has space").is_some());
        assert!(name_problem("has:colon").is_some());
        assert!(name_problem(&"x".repeat(65)).is_some());
    }

    #[test]
    fn the_name_problem_message_names_the_offending_character() {
        let problem = name_problem("bad name").expect("a problem");
        assert!(problem.contains(' '), "{problem}");
    }
}