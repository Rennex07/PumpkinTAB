//! The sidebar.
//!
//! Switched off in the shipped `config.yml`: a sidebar line has no viewer, so
//! every placeholder worth showing (`%player%`, `%ping%`, `%group%`,
//! `%perm_prefix%`) renders as literal text. Per-viewer rows need
//! `Player::send-packet` to accept the scoreboard packets, which is v0.2 plugin
//! API work. Everything below is then unreachable and stays tested.
//!
//! ## What the WIT actually allows, and how a free-form line is built
//!
//! A sidebar row is `<team prefix><entry name><team suffix><number>`, and each
//! part comes from somewhere different:
//!
//! - the **entry name** is a plain `string`, so it carries no colour of its own;
//! - the **team prefix and suffix** are `text-component`s, so they can be
//!   anything, colour included;
//! - the **number format** is a `text-component` too (`number-format` is a
//!   variant with a `fixed(text-component)` case).
//!
//! The entry looks like it has to be a real player, because the WIT calls the
//! field `player-name`. It does not have to be. The host's `add_player_to_team`
//! is a `team.players.push(player)` and a packet, with no lookup of any entity
//! (`world/scoreboard.rs:553`, and the `team.players.push` at `:578`) - the name
//! is stored and echoed verbatim. So a team can be built around **any string
//! at all**.
//!
//! That is what makes `lines:` expressible: one team per line, the rendered line
//! as the team prefix, and an invisible key in the team as the entry name. The key
//! is two `§`-formatting codes carrying the line number plus a reset, which the
//! client parses and consumes rather than draws. See `line_key`, which spells
//! out why it is codes and not characters - the scoreboard is keyed by
//! `(entry name, objective)`, so two lines that rendered the same text have to
//! stay two rows.
//!
//! **The invisible key is the one thing here that has not been seen on a real
//! client.** It is verified against the host source, not against a rendering, and
//! there is no config key for it: `line_key` is one function and its encoding is
//! TAB's, so a client that ever did draw it is a change to that function rather
//! than to a setting.
//!
//! ## Rows for players still exist, and carry the rank
//!
//! One row per online player, with the right-hand text as the score's
//! `number-format`. The team also carries the prefix above the player's name tag,
//! which is what a rank prefix is for.
//!
//! ## Lines are shared, so they cannot use a per-player placeholder
//!
//! The sidebar is the **world** scoreboard, which one set of rows serves every
//! viewer. So a line's `%player%`, `%ping%` and `%group%` have no viewer to
//! resolve against and are left in the text as written. Everything viewerless -
//! `%online%`, `%tps%`, `%world%`, the clock - works, and those are what the
//! shipped defaults use.
//!
//! ## The `player-list` slot is never touched
//!
//! In vanilla the `player-list` display slot wins over the tab list. Two features
//! setting it would toggle each other off every refresh, so the sidebar uses
//! `sidebar` and nothing else ever claims `player-list`.
//!
//! ## The handle is obtained once
//!
//! `world.get-scoreboard` pushes a **new** entry into the plugin's resource table
//! on every call - the host's arm calls `state::add`
//! (`wasm_host/wit/v0_1/world.rs:749`), which is a `resource_table.push`
//! (`wasm_host/state.rs:177-179`) - and nothing reuses one. Calling it
//! every refresh would grow that table for as long as the server is up. The
//! handle is fetched once and kept, and the world it belongs to is remembered so
//! a world change re-fetches it rather than writing to the wrong one.

use std::collections::BTreeSet;

use pumpkin_plugin_api::{
    common::NamedColor,
    player::Player,
    scoreboard::{
        CollisionRule, DisplaySlot, NametagVisibility, NumberFormat, RenderType, Scoreboard,
        TeamSettings,
    },
    text::TextComponent,
};
use tracing::warn;

use crate::{config::Scoreboard as ScoreboardConfig, render::View, text};

/// The objective's name, as the host stores it.
///
/// Short because it is a protocol identifier the client sees, and because a long
/// name shows up in every vanilla `/scoreboard objectives` listing.
pub const OBJECTIVE: &str = "pumpkintab";

/// A team's prefix, suffix, a line's text or a number format, as a component.
///
/// Every one of those five is a `text-component` on this WIT - the team prefix
/// and suffix on `TeamSettings`, the number format as `NumberFormat::Fixed` -
/// and all five come through here, so there is one place that knows how a
/// rendered line becomes a component and not four.
///
/// It is [`text::component`] rather than a legacy string: the sidebar's fixed
/// lines are the one place a `<gradient:…>` or a `<click:…>` in a `lines:` entry
/// could actually reach a client, and neither survives `&` codes. See
/// [`crate::tab::component`] for the same switch on the tab list.
pub fn prefix_component(value: &str) -> TextComponent {
    text::component(value)
}

/// The host render type for a configured name, defaulting to `integer`.
pub fn render_type(name: &str) -> RenderType {
    match name.trim().to_ascii_lowercase().as_str() {
        "hearts" => RenderType::Hearts,
        _ => RenderType::Integer,
    }
}

/// The team settings for a group.
///
/// `nametag-visibility: always` and `collision-rule: always` on purpose: this team
/// is how a player gets their rank prefix above their head, and either of the
/// alternatives would silently take it away.
pub fn team_settings(prefix: &str, suffix: &str) -> TeamSettings {
    TeamSettings {
        display_name: TextComponent::text("PumpkinTAB"),
        friendly_fire: false,
        see_friendly_invisibles: false,
        nametag_visibility: NametagVisibility::Always,
        collision_rule: CollisionRule::Always,
        color: NamedColor::White,
        prefix: prefix_component(prefix),
        suffix: prefix_component(suffix),
    }
}

/// The team name for line `index`.
///
/// Namespaced so a `/tab reload` that shortens `lines:` can find and remove the
/// teams the previous config left behind, without touching a group team.
pub fn line_team(index: usize) -> String {
    format!("pumpkintab_line_{index}")
}

/// The entry name that carries line `index`.
///
/// The row is `<team prefix><entry name>`, and the text goes in the prefix, so
/// the entry name has to render as nothing. This is TAB's scheme, taken from
/// `ScoreboardLineHolder`:
///
/// ```java
/// forcedPlayerNameStart = String.format("§%d§%d§r", (lineNumber / 10) % 10, lineNumber % 10);
/// ```
///
/// **Formatting codes, not characters.** The client parses and consumes them, so
/// nothing is drawn - unlike a zero-width space, which is a real glyph the client
/// has to render and may draw as a missing-character box.
///
/// Two properties fall out of the encoding, which is why TAB uses it:
///
/// - the digits make every key **unique**, so two lines that rendered identical
///   text stay two rows rather than collapsing into one;
/// - the digits also carry the **line number**, and the client sorts equal scores
///   by name, so the key pins each row to its configured position.
///
/// Note the two `§` are separate: `§01` would be the colour black followed by a
/// literal `1` on screen, which is not the same thing at all.
pub fn line_key(index: usize) -> String {
    let number = index + 1;
    format!(
        "\u{a7}{}\u{a7}{}\u{a7}r",
        number / 10 % 10,
        number % 10
    )
}

/// One entry of `lines:`, parsed from TAB's syntax.
///
/// TAB writes a line as `<text>||<number format>||<score>` and splits on `||`,
/// so the three parts are independent and any of them may be absent:
///
/// ```yaml
/// lines:
///   - '* &eOnline&7:||%online%'      # text, and something on the right
///   - '||%ping%&8ms'                 # no text, just the right-hand value
///   - 'Plain text'                   # just text
///   - 'Left||||15'                   # text and a score, no number format
/// ```
pub struct Line<'a> {
    /// The left-hand text, which becomes the team prefix.
    pub text: &'a str,
    /// The right-hand text, which becomes the score's `number-format`.
    pub number_format: &'a str,
    /// An explicit score, overriding the positional default.
    pub score: Option<i32>,
    /// Configured as nothing at all, so it stays a blank row.
    ///
    /// TAB distinguishes this from a line whose placeholders happen to render
    /// empty: a line written `""` is a **spacer** and always shows, while a line
    /// that merely *resolves* to empty is hidden. Without that distinction a
    /// sidebar could not be spaced out at all.
    pub spacer: bool,
}

/// Parses one `lines:` entry.
///
/// A leading `Long|` is TAB's marker for putting the text in the entry name rather
/// than the prefix, to dodge the old 16-character holder limit. That limit is gone
/// in 1.13 and later, so the two paths are identical here and the marker is only
/// stripped, which means a TAB config pastes in unchanged.
pub fn parse_line(raw: &str) -> Line<'_> {
    let raw = raw.strip_prefix("Long|").unwrap_or(raw);
    let mut parts = raw.split("||");
    let text = parts.next().unwrap_or("");
    let number_format = parts.next().unwrap_or("");
    Line {
        text,
        number_format,
        // A score that is not a number is dropped rather than failing the whole
        // config: a typo in one line should not cost the admin every other line.
        score: parts.next().and_then(|part| part.trim().parse().ok()),
        spacer: text.is_empty() && number_format.is_empty(),
    }
}

/// The score for line `index`, unless the line set one itself.
///
/// `total` is every row on the board, lines and players together, so a line and a
/// player can never land on the same value.
pub fn line_score(line: &Line<'_>, index: usize, total: usize) -> i32 {
    line.score.unwrap_or_else(|| total.saturating_sub(index) as i32)
}

/// Whether a line should be on the board at all this refresh.
///
/// Hidden when it resolved to nothing and was not written as a spacer. TAB does the
/// same, so a conditional line can drop out and take its row with it.
pub fn line_is_visible(line: &Line<'_>, rendered_text: &str, rendered_right: &str) -> bool {
    line.spacer || !rendered_text.is_empty() || !rendered_right.is_empty()
}

/// The team settings for a fixed line: the line's own text as the prefix.
///
/// `nametag-visibility: never` and `collision-rule: never`, matching TAB: no real
/// player is ever in one of these teams, and `always` would be a live setting on a
/// team that must never touch anybody.
pub fn line_settings(rendered: &str) -> TeamSettings {
    TeamSettings {
        display_name: TextComponent::text("PumpkinTAB line"),
        friendly_fire: false,
        see_friendly_invisibles: false,
        nametag_visibility: NametagVisibility::Never,
        collision_rule: CollisionRule::Never,
        color: NamedColor::White,
        prefix: prefix_component(rendered),
        suffix: TextComponent::text(""),
    }
}

/// Creates or updates the objective and the teams, and puts it in the sidebar slot.
///
/// **Idempotent, deliberately.** A `/tab reload` calls this again, and the host
/// makes both halves of the naive version misbehave: `add_team` warns and returns
/// without touching an existing team (`world/scoreboard.rs:481`), so a group's
/// settings would never change, and `remove-objective` followed by `add-objective`
/// makes every client's sidebar blink out for a moment.
///
/// So the board is asked what it already has - `get-teams` for the teams - and
/// each piece is updated in place if it is there and created if it is not.
///
/// The one thing that cannot be asked is whether the objective exists, because
/// there is no getter for it. `exists` is that answer, tracked by the caller: a
/// first build removes the objective before adding it, because the world scoreboard
/// is saved with the level and a previous run of the server may have left one
/// behind, and every later build updates it in place.
///
/// Also clears line teams left by a longer previous `lines:`, because a reload
/// that removes a line would otherwise leave its row on the board forever.
///
/// Returns the group teams that exist, and whether the objective is now present.
pub fn create(
    board: &Scoreboard,
    config: &ScoreboardConfig,
    view: &View<'_>,
    previous_lines: usize,
    exists: bool,
) -> Created {
    let mut teams = BTreeSet::new();
    if !config.enabled {
        return Created {
            teams,
            objective_exists: exists,
        };
    }

    // Drop the rows a shorter config no longer has, before the new ones, so a
    // shrink does not leave orphaned teams and rows behind.
    for index in config.lines.len()..previous_lines {
        board.remove_score(&line_key(index), OBJECTIVE);
        board.remove_team(&line_team(index));
    }

    let heading = title(config, view);
    let render = render_type(&config.render_type);
    if exists {
        board.update_objective(OBJECTIVE, heading, render, Some(NumberFormat::Blank));
    } else {
        // Clears anything a previous run of the server left in the level data.
        board.remove_objective(OBJECTIVE);
        board.add_objective(OBJECTIVE, heading, render, Some(NumberFormat::Blank));
    }
    board.set_display_slot(DisplaySlot::Sidebar, OBJECTIVE);

    // What the board already has, so each team is updated rather than
    // re-created. Asked once: it is a resource call, and the list cannot change
    // underneath the loop below.
    let mut present: BTreeSet<String> = board.get_teams().into_iter().collect();
    let mut put = |board: &Scoreboard, name: &str, settings: TeamSettings| {
        if present.contains(name) {
            board.update_team(name, settings);
        } else {
            board.create_team(name, settings);
            present.insert(name.to_string());
        }
    };

    // One team per group, and one for the fallback, so a player's previous team
    // can be removed when they change group. Their prefixes are empty: PumpkinTAB
    // does not know what any group's prefix is, because the prefix belongs to
    // PumpkinPerm and reaches the tab list as `%perm_prefix%`.
    for name in view.config.groups_by_order() {
        put(board, name, team_settings("", ""));
        teams.insert(name.clone());
    }
    let fallback = crate::config::DEFAULT_GROUP;
    put(board, fallback, team_settings("", ""));
    teams.insert(fallback.to_string());

    // One team per fixed line, carrying that line's rendered text as its prefix.
    // The text itself is written each refresh by [`set_line`], so the team is
    // created empty here and only its existence matters.
    for index in 0..config.lines.len() {
        let team = line_team(index);
        put(board, &team, line_settings(""));
        board.add_player_to_team(&team, &line_key(index));
    }

    Created {
        teams,
        objective_exists: true,
    }
}

/// What [`create`] built.
pub struct Created {
    /// The group teams, for [`assign_team`] to work against.
    pub teams: BTreeSet<String>,
    /// Whether the objective is now on the board, to pass back next time.
    pub objective_exists: bool,
}

/// Writes one fixed line's row.
///
/// `rendered_text` goes into the team prefix and `rendered_right` into the score's
/// number format, which is how TAB's `<text>||<number format>` reaches the screen.
///
/// The team is updated rather than recreated because `add_team` refuses to touch an
/// existing team, and the row's key is derived from the line's position so the
/// entry name never has to change.
pub fn set_line(
    board: &Scoreboard,
    index: usize,
    value: i32,
    rendered_text: &str,
    rendered_right: &str,
) {
    let team = line_team(index);
    let entry = line_key(index);
    board.update_team(&team, line_settings(rendered_text));
    // **Every time, not once at build.** The host's `update_team` writes a fresh
    // `Team` built from the WIT's `TeamSettings`, which has no member list, over
    // the one it stored (`world/scoreboard.rs:534`), and sends `TeamMethod::Update`
    // with `players: []` (`:531`). So the entry carrying this row is dropped from
    // its team on the very first refresh and the prefix - which is where all of
    // this line's text lives - stops being applied.
    //
    // That is why the labels used to disappear while the numbers stayed: a
    // number format rides on the score, and it survives; the text is a team
    // prefix, and it did not. `add_player_to_team` is idempotent here because the
    // host returns early when the entry is already a member
    // (`world/scoreboard.rs:567`) - which is what stops this costing a packet per
    // player per refresh.
    board.add_player_to_team(&team, &entry);
    let right = if rendered_right.trim().is_empty() {
        Some(NumberFormat::Blank)
    } else {
        Some(NumberFormat::Fixed(prefix_component(rendered_right)))
    };
    board.update_score(&entry, OBJECTIVE, value, right);
}

/// Removes one line's row, for a line that resolved to nothing this refresh.
pub fn hide_line(board: &Scoreboard, index: usize) {
    board.remove_score(&line_key(index), OBJECTIVE);
}

/// Clears one line's row, used when a world stops being shown to a player.
pub fn clear_line(board: &Scoreboard, index: usize) {
    board.remove_score(&line_key(index), OBJECTIVE);
}

/// The objective's title, resolved.
pub fn title(config: &ScoreboardConfig, view: &View<'_>) -> TextComponent {
    let resolved = crate::render::render_locally(view, &config.title);
    prefix_component(&resolved)
}

/// Puts `player` in their group's team, removing them from whichever team the
/// plugin put them in before.
///
/// The tracking set is what makes that possible: the host has no "which team is
/// this player in, as set by me" question, and adding a player to a second team
/// without removing them from the first leaves the client to pick.
pub fn assign_team(board: &Scoreboard, teams: &BTreeSet<String>, player: &Player, group: &str) {
    if !teams.contains(group) {
        // The group was renamed or removed by a `/tab reload`, so the team does
        // not exist. Say so once rather than writing to a name that is not there.
        warn!("no PumpkinTAB team for group '{group}'; the sidebar row will show the bare name");
        return;
    }
    let name = player.get_name();
    for other in teams {
        if other != group {
            board.remove_player_from_team(other, &name);
        }
    }
    board.add_player_to_team(group, &name);
}

/// Writes one row: the player's score, and the text on its right.
///
/// The score value is what orders the rows, and the client shows the highest
/// first, so the caller passes a descending value. `number-format` is per call,
/// which is what lets a row show `%ping%ms` on the right while another shows
/// nothing at all.
pub fn set_row(board: &Scoreboard, player: &str, value: i32, right: Option<&str>) {
    let number_format = match right {
        // `Blank` rather than `None`: `None` means "no override", which would
        // leave whatever the objective was created with in place.
        Some(text) => Some(NumberFormat::Fixed(prefix_component(text))),
        None => Some(NumberFormat::Blank),
    };
    board.update_score(player, OBJECTIVE, value, number_format);
}

/// Removes a player's row by name.
///
/// A name rather than a `&Player`, because the caller is handling a leave event
/// and only has the name to hand by then.
pub fn clear_row(board: &Scoreboard, player: &str) {
    board.remove_score(player, OBJECTIVE);
}

/// Whether `player` is in a world the sidebar is enabled in.
pub fn applies(config: &ScoreboardConfig, player: &Player) -> bool {
    config.applies_in(&player.get_world().get_name())
}

/// The descending score for row `index` of `total`.
///
/// Highest first, because that is how a Java client sorts a sidebar. Ties are
/// broken by name by the client, so the values only have to be distinct.
///
/// A client with both `lines:` and players on it needs every value distinct
/// across *both* sets, so the two are numbered from one shared range rather than
/// each starting at its own count: lines take the top, players follow.
pub fn row_value(index: usize, total: usize) -> i32 {
    (total.saturating_sub(index)) as i32
}

/// The score for line `index`, given how many lines and players share the board.
///
/// The two sets are numbered from **one** range, lines taking the top, or they
/// both count from their own size and overlap.
pub fn line_value(index: usize, lines: usize, players: usize) -> i32 {
    row_value(index, lines + players)
}

/// The score for a player row, below every line.
pub fn player_value(index: usize, lines: usize, players: usize) -> i32 {
    row_value(lines + index, lines + players)
}

/// Whether every row on the board has a distinct score.
///
/// Two rows sharing a score are sorted by name by the client, which reorders the
/// sidebar for no reason the admin asked for.
pub fn scores_are_distinct(lines: usize, players: usize) -> bool {
    let mut values: Vec<i32> = (0..lines)
        .map(|index| line_value(index, lines, players))
        .chain((0..players).map(|index| player_value(index, lines, players)))
        .collect();
    let before = values.len();
    values.sort_unstable();
    values.dedup();
    values.len() == before
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_highest_first() {
        assert_eq!(row_value(0, 4), 4);
        assert_eq!(row_value(3, 4), 1);
    }

    /// Two rows sharing a score are ordered by name by the client, which reshuffles
    /// the sidebar for no reason. The two row sets are numbered separately, so this
    /// is exactly where an off-by-one would hide.
    #[test]
    fn no_two_rows_share_a_score() {
        for lines in 0..6 {
            for players in 0..6 {
                assert!(
                    scores_are_distinct(lines, players),
                    "{lines} line(s) and {players} player(s) collide"
                );
            }
        }
    }

    #[test]
    fn a_line_parses_into_its_three_parts() {
        let line = parse_line("* &eOnline&7:||%online%||15");
        assert_eq!(line.text, "* &eOnline&7:");
        assert_eq!(line.number_format, "%online%");
        assert_eq!(line.score, Some(15));
        assert!(!line.spacer);
    }

    /// `||` is matched non-overlapping, so `||||` is two separators and leaves an
    /// empty middle. Same as TAB, which splits with a limit of -1.
    #[test]
    fn an_empty_middle_is_left_empty() {
        let line = parse_line("Left||||7");
        assert_eq!(line.text, "Left");
        assert_eq!(line.number_format, "");
        assert_eq!(line.score, Some(7));
    }

    #[test]
    fn every_part_is_optional() {
        let only_text = parse_line("Plain");
        assert_eq!(only_text.text, "Plain");
        assert_eq!(only_text.number_format, "");
        assert_eq!(only_text.score, None);

        let only_right = parse_line("||%ping%ms");
        assert_eq!(only_right.text, "");
        assert_eq!(only_right.number_format, "%ping%ms");

        let spacer = parse_line("");
        assert!(spacer.spacer);
    }

    #[test]
    fn an_unparseable_score_is_dropped_rather_than_failing_the_line() {
        let line = parse_line("Left||right||not-a-number");
        assert_eq!(line.text, "Left");
        assert_eq!(line.number_format, "right");
        assert_eq!(line.score, None);
    }

    /// TAB's marker exists to dodge a 16-character limit that 1.13 removed, so here
    /// it only has to come off. A TAB config has to paste in unchanged.
    #[test]
    fn the_long_marker_is_stripped() {
        let line = parse_line("Long|Some quite long text||%ping%");
        assert_eq!(line.text, "Some quite long text");
        assert_eq!(line.number_format, "%ping%");
    }

    /// The whole point of the distinction: a line written `""` is a spacer that must
    /// show, while a line that merely *resolves* to empty must disappear.
    #[test]
    fn a_configured_blank_line_survives_but_an_empty_result_does_not() {
        let spacer = parse_line("");
        assert!(line_is_visible(&spacer, "", ""));

        let conditional = parse_line("%some_placeholder_that_is_off%");
        assert!(!line_is_visible(&conditional, "", ""));

        assert!(line_is_visible(&conditional, "&7Offline", ""));
        assert!(line_is_visible(&conditional, "", "0"));
    }

    /// The key is two separate colour codes plus a reset, not the digits as text:
    /// `§01` would draw a literal `1`. Line numbers are 1-based, as in TAB.
    #[test]
    fn a_line_key_is_codes_not_text() {
        assert_eq!(line_key(0), "\u{a7}0\u{a7}1\u{a7}r");
        assert_eq!(line_key(8), "\u{a7}0\u{a7}9\u{a7}r");
        assert_eq!(line_key(9), "\u{a7}1\u{a7}0\u{a7}r");
        assert_eq!(line_key(10), "\u{a7}1\u{a7}1\u{a7}r");
        assert_eq!(line_key(14), "\u{a7}1\u{a7}5\u{a7}r");
    }

    /// Two lines rendering identical text must stay two rows, so every key differs.
    #[test]
    fn line_keys_are_distinct() {
        let keys: Vec<String> = (0..99).map(line_key).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "two lines share a key");
    }

    /// A key must never collide with a real player name, or a player would be pulled
    /// out of their group team and onto a line. A username is `[A-Za-z0-9_]`, and
    /// every key contains `§`, which is not in that set.
    #[test]
    fn a_line_key_could_not_be_a_player_name() {
        for index in 0..99 {
            let key = line_key(index);
            assert!(key.contains('\u{a7}'), "{key:?} carries no colour code");
            assert!(
                !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{key:?} is a legal player name"
            );
        }
    }

    /// Above 99 the two-digit encoding repeats, which is why TAB refuses to go there
    /// and so must the config check.
    #[test]
    fn line_keys_are_unique_only_up_to_tab_s_limit() {
        assert_eq!(line_key(0), line_key(100));
    }

    #[test]
    fn an_explicit_score_beats_the_positional_default() {
        let line = parse_line("Left||||7");
        assert_eq!(line_score(&line, 0, 5), 7);
    }

    #[test]
    fn without_one_a_line_scores_from_the_top_down() {
        let line = parse_line("Left");
        assert_eq!(line_score(&line, 0, 4), 4);
        assert_eq!(line_score(&line, 3, 4), 1);
    }

    #[test]
    fn render_types_map_onto_the_host_enum() {
        assert_eq!(render_type("integer"), RenderType::Integer);
        assert_eq!(render_type("hearts"), RenderType::Hearts);
        assert_eq!(render_type("HEARTS"), RenderType::Hearts);
    }

    #[test]
    fn an_unknown_render_type_falls_back_rather_than_failing() {
        assert_eq!(render_type("pixels"), RenderType::Integer);
    }

    /// In vanilla the player-list slot wins over the tab list, so a second feature
    /// claiming it would fight this one every refresh.
    #[test]
    fn the_player_list_slot_is_never_the_one_used() {
        assert_ne!(DisplaySlot::Sidebar, DisplaySlot::PlayerList);
        assert_eq!(DisplaySlot::Sidebar, DisplaySlot::Sidebar);
    }

    /// Deliberately absent: `team_settings` builds two `TextComponent`s, and on a
    /// host build the wit-bindgen constructors are `unreachable!()` stubs, so this
    /// cannot run in a unit test at all.
    ///
    /// What it *does* guarantee is written down next to the code that relies on it.
    #[test]
    fn the_objective_name_is_short_and_protocol_safe() {
        assert!(OBJECTIVE.len() <= 16);
        assert!(OBJECTIVE.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    }
}