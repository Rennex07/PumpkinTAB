//! Resolving `%…%` placeholders.
//!
//! Four sources, in a fixed order, and the order is the design:
//!
//! 1. **`%animation:<name>%`** - a frame, selected from a wall clock and then
//!    resolved *again* as a template, because a frame is a template. An animation
//!    that embeds `%time%` really does show the time.
//! 2. **`%condition:<name>%`** - a configured condition, whose `true:` or `false:`
//!    text is itself resolved as a template. See [`Condition`], and read
//!    [below](#conditional-placeholders) for what is and is not wired up.
//! 3. **The built-ins** below, all answered from the plugin API. These are all
//!    O(1) host reads, which is why they are here rather than left to PIE: the
//!    tab list then works with no PIE installed at all.
//! 4. **Everything else** is handed to PIE in one batch per refresh, and whatever
//!    PIE cannot answer is left in the text as written.
//!
//! An unknown name is never blanked. It is left exactly as the author wrote it,
//! which is what TAB.java does and the only useful behaviour: a typo has to be
//! visible, and blanking it would hide both the typo and the fact that something
//! is missing. [`crate::pie`] names what is still unresolved, at a rate an admin
//! can read.
//!
//! ## Conditional placeholders
//!
//! The syntax is TAB's, from
//! <https://github.com/NEZNAMY/TAB/wiki/Feature-guide:-Conditional-placeholders>
//! (read 2026-09-30). A `conditions:` section names a test, and `%condition:<name>%`
//! returns the `true:` or `false:` text:
//!
//! ```yaml
//! conditions:
//!   low_ping:
//!     conditions:
//!     - '%ping%<50'
//!     true: '&a%ping%'
//!     false: '&c%ping%'
//!   is_staff:
//!     conditions:
//!     - 'permission:pumpkintab.staff'
//!     true: 'STAFF'
//! ```
//!
//! The twelve comparators are `>=` `>` `<=` `<` `=` `!=` `<-` `!<-` `|-` `!|-`
//! `-|` `!-|`, plus `permission:<node>` and `!permission:<node>`. More than one
//! expression needs `type: AND` or `type: OR`; `AND` is the default. `true:` and
//! `false:` default to the literal words `true` and `false`, which is what makes
//! a bare test usable as another condition's expression.
//!
//! **The engine is wired to a `conditions:` section**, evaluated between an
//! animation frame and the built-ins, so a condition can see an animated value
//! but not the other way round. A `%condition:…%` naming a condition that is not
//! defined resolves to [`None`] and is left as written, and `config::problems()`
//! names it at load, because a misspelled condition is otherwise indistinguishable
//! from a typo in any other placeholder.
//!
//! Three deliberate differences from TAB, all forced by the host:
//!
//! - **A condition sees the built-ins and nothing else.** It is evaluated during
//!   the local pass, which happens *before* the PIE batch, so a condition on
//!   `%perm_group%` compares the literal text `%perm_group%`.
//! - **No relational conditions.** `%viewer:…%` and `%rel_condition:…%` exist to
//!   read one placeholder as the viewer and another as the target. There is only
//!   ever one [`View`] per line on this WIT - and `set-tab-list-name` broadcasts
//!   (`entity/player.rs:1009`), so there is no distinct viewer to differ from.
//! - **An unparseable number counts as `0`,** as in TAB. A side that is not a
//!   number after placeholders are resolved - which usually means a placeholder
//!   resolved to nothing - therefore compares as zero rather than failing the
//!   expression. The unresolved-placeholder report is what says so out loud.
//!
//! ## What is deliberately absent
//!
//! There is no `%group_prefix%`. The group list is just names, and the prefix
//! belongs to PumpkinPerm, which exposes it as `%perm_prefix%`. Defining it in both
//! places is how a prefix ends up disagreeing with itself.
//!
//! Nor is there anything that reads the **host's** view of offline data -
//! statistics, skin, inventory, the UUID of somebody who is not here.
//! `CPlayerInfoUpdate` is dropped by `send-packet` (`player.rs:3577-3581`), so
//! there are no fake players to ask about and no offline player to ask for.
//!
//! `%playtime%` is the deliberate exception, and it is worth being exact about
//! why it is not the same thing: the host is never asked anything. It is answered
//! from this plugin's own `playtime.yml`, counted by [`crate::playtime`] from the
//! refresh loop that only ever sees connected players. (`%health%` is in the same
//! position - a live read of the player in front of you - and `%playtime%` is the
//! one live read that is remembered afterwards.)
//!
//! ## Why there is no per-placeholder cache
//!
//! The design called for one: an expensive placeholder on a slower interval than
//! the rest. There is nothing to put in it. Every built-in above is a single host
//! call, and PIE already has a per-namespace TTL that the expansion can ask for
//! (`protocol.rs::Cache`). A second cache here would be a third TTL to reason
//! about, tuned for a set of placeholders that does not exist. If an expensive
//! placeholder turns up, the right home for its interval is its expansion.
//!
//! The one TTL that does exist is declared rather than tuned here: PumpkinPerm
//! registers its `perm` namespace with `Cache::Ttl { ms: 5000 }`, so a `%perm_*%`
//! value may legitimately be up to five seconds behind. That is the price of not
//! asking the permission plugin once per player per refresh.
//!
//! ## Recursion
//!
//! Frames and conditions resolve as templates, so resolution can nest. It is
//! bounded at [`MAX_DEPTH`] rather than left open: an animation whose frame is
//! itself `%animation:itself%` is a config mistake, and an unbounded walk over it
//! inside a once-a-second refresh is a hang. [`MAX_DEPTH`] is also how many
//! conditions may chain, which covers TAB's own two-level ping example.

use std::collections::BTreeMap;

use pumpkin_plugin_api::Server;
use pumpkin_plugin_api::common::GameMode;
use pumpkin_plugin_api::permission::PermissionLevel;
use pumpkin_plugin_api::player::Player;

use crate::{
    config::{Animation, Config},
    playtime,
    text::{self, Segment},
};

/// The prefix that marks an animation reference.
pub const ANIMATION_PREFIX: &str = "animation:";

/// The tick rate a server can actually run at.
///
/// Twenty is a property of the tick loop, not of the machine: it sleeps to a 50 ms
/// tick (`crates/pumpkin/src/server/ticker.rs:22`), so no amount of headroom makes
/// the server tick faster and no client ever sees more than 20.00.
pub const MAX_TPS: f64 = 20.0;

/// The prefix that marks a conditional reference.
pub const CONDITION_PREFIX: &str = "condition:";

/// How deep a frame or a condition may nest before resolution gives up.
///
/// Two is enough for the real cases: a frame containing a built-in, a frame
/// containing another animation, and a condition whose `false:` text is a second
/// condition, which is how TAB's own ping-colouring example is written.
pub const MAX_DEPTH: u8 = 2;

/// The built-in placeholders, and the name each is written as in a config.
///
/// Documented in the shipped `config.yml` as well; this list is what actually
/// answers, and [`BUILT_IN_ALIASES`] are in it because an alias answers just as
/// much as a first-class name does.
pub const BUILT_INS: [&str; 22] = [
    "player",
    "viewer",
    "ping",
    "playtime",
    "world",
    "uuid",
    "permission_level",
    "health",
    "gamemode",
    "player_ip",
    "bedrock",
    "group",
    "online",
    "max_players",
    "worldonline",
    "server_name",
    "server_time",
    "server_date",
    "time",
    "date",
    "tps",
    "mspt",
];

/// Names written differently from the built-in they resolve to.
///
/// TAB.java calls the clock `%time%` and the date `%date%`, and a config written
/// for TAB should not have to be edited to work here. They are a table rather
/// than second match arms so that there is still exactly one arm per *value*,
/// and [`the_every_alias_points_at_a_built_in`] is what keeps the table honest.
pub const BUILT_IN_ALIASES: [(&str, &str); 2] = [("time", "server_time"), ("date", "server_date")];

/// The built-in `name` really answers, following [`BUILT_IN_ALIASES`].
pub fn canonical_built_in(name: &str) -> &str {
    BUILT_IN_ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or(name, |(_, target)| *target)
}

/// The wall clock animations are read against, as milliseconds since the plugin
/// loaded.
///
/// An [`Instant`] rather than a tick count, because an animation's interval is
/// configured in milliseconds and may be far faster than the refresh loop. A tick
/// count would force the two periods to line up.
#[derive(Debug, Clone, Copy)]
pub struct Clock(std::time::Instant);

impl Clock {
    /// Starts the clock.
    pub fn start() -> Self {
        Self(std::time::Instant::now())
    }

    /// Milliseconds since the clock started.
    pub fn elapsed_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::start()
    }
}

/// Everything a template can be resolved against.
pub struct View<'a> {
    /// The configuration, for groups and animations.
    pub config: &'a Config,

    /// The clock animations read.
    pub clock: Clock,

    /// The server, for the global values.
    pub server: &'a Server,

    /// Whose entry is being built, if any.
    ///
    /// [`None`] is a real case, not an oversight: the bossbar title and any
    /// server-wide text have no viewer, and `%player%` for a viewerless template
    /// is unknown rather than empty.
    pub player: Option<&'a Player>,

    /// The name of the group `player` is shown as, already resolved.
    ///
    /// A borrowed `&str` rather than the whole [`crate::groups::Resolved`],
    /// because the name is all a placeholder can ask for and holding the
    /// resolution would tie every `View` to a borrow of the configuration.
    pub group: Option<&'a str>,
}

impl View<'_> {
    /// Resolves one placeholder name, or [`None`] to leave it as written.
    pub fn resolve(&self, name: &str) -> Option<String> {
        self.resolve_at(name, MAX_DEPTH)
    }

    /// As [`Self::resolve`], with the depth budget carried through.
    ///
    /// [`pub`] because a condition's `true:` / `false:` text is a template that
    /// goes round the resolver again, one level shallower than the condition
    /// itself. That is the whole recursion guard for conditions.
    ///
    /// Order matters, and is the order the module header documents: an animation,
    /// then a condition, then the built-ins. A name can claim only one prefix, and
    /// neither prefix is a legal built-in, so the three can never collide - which
    /// the two `*_prefix_is_not_a_built_in` tests hold rather than leave to
    /// inspection.
    pub fn resolve_at(&self, name: &str, depth: u8) -> Option<String> {
        if let Some(name) = name.strip_prefix(ANIMATION_PREFIX) {
            return self.frame(name, depth);
        }
        if let Some(name) = name.strip_prefix(CONDITION_PREFIX) {
            return self
                .config
                .conditions
                .get(name)
                .and_then(|condition| condition.resolve(self, depth));
        }
        self.built_in(name)
    }

    /// The current frame of `animation`, resolved as a template itself.
    ///
    /// An unknown name resolves to [`None`], which leaves `%animation:foo%` on
    /// screen. Java TAB does the same, and it is the only useful behaviour: a
    /// mistyped animation name has to be visible.
    fn frame(&self, animation: &str, depth: u8) -> Option<String> {
        if depth == 0 {
            return None;
        }
        let frame = frame_text(self.config.animations.get(animation)?, self.clock.elapsed_ms())?;
        // A frame is a template, so it goes round again. The depth guard is what
        // stops an animation that refers to itself from spinning here.
        Some(text::substitute(&frame, |inner| self.resolve_at(inner, depth - 1)))
    }

    fn built_in(&self, name: &str) -> Option<String> {
        let player = self.player;
        let group = self.group;
        // Aliases first, so the arms below are one per value rather than one per
        // spelling of it.
        match canonical_built_in(name) {
            "player" => player.map(|player| player.get_name()),
            // Who is looking at it. The same person in almost every case, and
            // different only for a shared header, where the distinction earns its
            // keep.
            "viewer" => player.map(|player| player.get_name()),
            // Zero means "no keep-alive seen yet", and the host never writes it
            // for a Bedrock connection at all
            // (`net/java/play/keep_alive.rs:17` and `:24` are the only two writers of it,
            // and both are on the Java path). Printing
            // `0ms` for that would be a lie, so it is left unknown and the
            // placeholder stays visible. `%bedrock%` is what tells an admin why.
            "ping" => player.and_then(|player| match player.get_ping() {
                0 => None,
                millis => Some(millis.to_string()),
            }),
            // How long they have been connected, across every session, kept in
            // `playtime.yml` by [`crate::playtime`].
            //
            // **Never [`None`] for a player it can see**, which is the one place
            // this differs from `ping` above: zero playtime is a fact about a
            // brand-new player rather than a reading that has not arrived yet, so
            // it prints `0s`. Answering `None` would leave `%playtime%` sitting
            // on screen in exactly the shape of a typo, for every joiner, on
            // their first second.
            //
            // A template with **no player** still gets [`None`], the same as every
            // other per-player placeholder here - which is what stops a
            // server-wide header or a fixed sidebar row from inventing a number
            // for whoever happens to be looking.
            "playtime" => player.map(playtime::of),
            "world" => player.map(|player| player.get_world().get_name()),
            "uuid" => player.map(|player| pumpkin_plugin_api::uuid::to_string(player.get_id())),
            // The op level, as the number the config uses, so
            // `%permission_level%>=2` means exactly what `op-level-groups`'s
            // `two:` means.
            "permission_level" => player.map(|player| op_level(player.get_permission_level()).to_string()),
            // Rounded up, because that is what the heart display shows and a
            // `6.5` in a tab list is not what anybody is looking at.
            "health" => player.map(|player| player.get_health().ceil().to_string()),
            "gamemode" => player.map(|player| game_mode(player.get_gamemode()).to_string()),
            "player_ip" => player.map(|player| player.get_ip()),
            // `false` or `true`, as TAB writes it. Also the honest answer to "why
            // is my ping showing `%ping%`": a Bedrock connection never gets one.
            "bedrock" => player.map(|player| player.as_bedrock().is_some().to_string()),
            // Only the name, because that is all PumpkinTAB knows about a group.
            //
            // There is deliberately no `%group_prefix%` here. The group list is
            // just names, and the *prefix* lives in PumpkinPerm, so it comes
            // through as `%perm_prefix%`. One owner per setting is the whole
            // point: a prefix defined in two places is a prefix that will
            // disagree with itself.
            "group" => group.map(str::to_string),
            "online" => Some(self.server.get_player_count().to_string()),
            "max_players" => Some(self.server.get_max_players().to_string()),
            // The host has a call for this rather than it being counted by hand:
            // `get-player-count-in-world` reads the world's own player set
            // (`server.rs:227`). It takes the world handle by value, so the
            // handle is fetched fresh and cannot be reused afterwards.
            "worldonline" => player.map(|player| {
                self.server
                    .get_player_count_in_world(player.get_world())
                    .to_string()
            }),
            "server_name" => Some(self.server.get_motd()),
            "server_time" => Some(world_clock().format_time(self.config.utc_offset_hours)),
            "server_date" => Some(world_clock().format_date(self.config.utc_offset_hours)),
            // **Clamped, and not by choice.** The host computes `get_tps()` as
            // `1000.0 / mspt` with no ceiling (`server/mod.rs:1213`), so a server
            // doing 3.84 ms a tick reports `260.61`. That is the tick rate it
            // *could* run at, not the rate it does: the tick loop sleeps to 50 ms
            // (`server/ticker.rs:22`), so the real rate is 20 and every client
            // shows 20.00. Printing 260.61 is the host's number quoted without the
            // thing that bounds it, and it reads as a swapped or broken row.
            //
            // Only the fast case is affected - anything slower than 20 is already
            // reported correctly - so this leaves a struggling server's number
            // alone and only stops an idle one from claiming 260.
            "tps" => Some(format!("{:.2}", self.server.get_tps().min(MAX_TPS))),
            "mspt" => Some(format!("{:.2}", self.server.get_mspt())),
            _ => None,
        }
    }
}

/// The host's op level as the number the config and a condition both use.
fn op_level(level: PermissionLevel) -> u8 {
    match level {
        PermissionLevel::Zero => 0,
        PermissionLevel::One => 1,
        PermissionLevel::Two => 2,
        PermissionLevel::Three => 3,
        PermissionLevel::Four => 4,
    }
}

/// The game mode as TAB and `/gamemode` spell it.
fn game_mode(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "survival",
        GameMode::Creative => "creative",
        GameMode::Adventure => "adventure",
        GameMode::Spectator => "spectator",
    }
}

/// The frame of `animation` at `elapsed_ms`, as written.
pub fn frame_text(animation: &Animation, elapsed_ms: u64) -> Option<String> {
    animation.frame_at(elapsed_ms).map(str::to_string)
}

/// Resolves a template against the built-ins and animations only.
///
/// The PIE half is separate, in [`crate::pie`], because it costs something
/// different: a local substitution is arithmetic, and PIE is an inter-plugin
/// message. Splitting them is what lets the caller batch every line of a refresh
/// into one message.
pub fn render_locally(view: &View<'_>, template: &str) -> String {
    text::substitute(template, |name| view.resolve(name))
}

/// Whether `text` still holds a `%name%`.
pub fn has_placeholder(text: &str) -> bool {
    !placeholder_names(text).is_empty()
}

/// How many `%name%` are in `text`.
///
/// Used to decide whether a line is worth sending to PIE, and to report what is
/// still unresolved. A count rather than a call to [`placeholder_names`] because
/// it runs on every line of every refresh and the answer is a `usize`: allocating
/// a `String` per placeholder to throw it away would be the more expensive way of
/// asking. [`the_names_and_the_count_agree`] is what stops the two drifting.
pub fn count_placeholders(text: &str) -> usize {
    text::segment(text)
        .iter()
        .filter(|segment| matches!(segment, Segment::Placeholder(_)))
        .count()
}

/// The prefix that is a known optional dependency rather than a spelling mistake.
///
/// `%perm_prefix%` needs PumpkinPerm and PumpkinPIE, and a server running neither
/// is a supported configuration - so an unanswered one is blanked rather than left
/// as written. Every other placeholder keeps the "leave it as written" rule, so a
/// typo is still visible.
pub const OPTIONAL_PREFIX: &str = "perm_prefix";

/// Removes an unresolved `%perm_prefix%` from a rendered tab entry.
///
/// A resolved prefix comes back as `§`-coded text and never matches, so this only
/// ever removes the literal placeholder. It removes every occurrence rather than
/// just a leading one because a format may place it after a name, and a half
/// blanked entry reads worse than a fully blanked one.
///
/// Returns the input untouched when the prefix is not there, which is the common
/// case and the reason this is cheap enough to run per player per refresh.
pub fn blank_unresolved_prefix(rendered: &str) -> String {
    if !rendered.contains(OPTIONAL_PREFIX) {
        return rendered.to_string();
    }
    rendered
        // A trailing space goes with the placeholder, or a middle-placed prefix
        // leaves a double space.
        .replace(&format!("%{OPTIONAL_PREFIX}% "), "")
        .replace(&format!("%{OPTIONAL_PREFIX}%"), "")
        .trim()
        .to_string()
}

/// The `%name%` still in `text`, without the `%` signs, in the order written.
///
/// Owned rather than borrowed, because [`text::segment`] hands back the names by
/// value and the only alternative is a lifetime threaded through for a call that
/// runs once per refresh on a handful of names.
///
/// Deliberately the same set [`count_placeholders`] counts: the report has to be
/// able to name every placeholder the count says is there, and a name the count
/// does not see is a name an admin would never be shown.
pub fn placeholder_names(text: &str) -> Vec<String> {
    text::segment(text)
        .iter()
        .filter_map(|segment| match segment {
            Segment::Placeholder(name) => Some(name.clone()),
            Segment::Text(_) => None,
        })
        .collect()
}

/// The wall clock, read through [`std::time`], which WASI preview 2 supplies and
/// the host does not touch. Always UTC - see [`Config::utc_offset_hours`].
///
/// [`Config::utc_offset_hours`]: crate::config::Config::utc_offset_hours
struct WorldClock;

fn world_clock() -> WorldClock {
    WorldClock
}

/// Seconds since midnight, as `h:MM:SS AM` or `h:MM:SS PM`.
///
/// Takes no clock of its own so the boundaries can be pinned exactly: midnight
/// and noon are the two hours a 12-hour formatter gets wrong, and a formatter
/// reading "now" can never reach either in a test.
fn format_seconds(seconds: u64) -> String {
    let (hour24, minute, second) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    let meridiem = if hour24 < 12 { "AM" } else { "PM" };
    // 0 and 12 are the two hours that are not what they look like: midnight is
    // 12 AM and noon is 12 PM.
    let hour12 = match hour24 % 12 {
        0 => 12,
        other => other,
    };
    format!("{hour12}:{minute:02}:{second:02} {meridiem}")
}

impl WorldClock {
    /// Seconds since local midnight in `offset_hours`, as `h:MM:SS AM` or
/// `h:MM:SS PM`.
fn format_time(&self, offset_hours: i32) -> String {
    format_seconds(self.seconds_since_midnight(offset_hours))
}

    /// `YYYY-MM-DD`, shifted by `offset_hours` from UTC.
    ///
    /// Shifted from the epoch rather than from midnight, so a zone behind UTC
    /// crossing midnight backwards rolls the date back too - which is the whole
    /// point of applying an offset at all.
    fn format_date(&self, offset_hours: i32) -> String {
        let (year, month, day) = civil_from_days(self.days_since_epoch(offset_hours));
        format!("{year:04}-{month:02}-{day:02}")
    }

    fn unix_seconds(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64)
    }

    /// Seconds since local midnight, wrapping into a day.
    ///
    /// `rem_euclid` rather than `%`, because a negative offset takes the clock
    /// *below* the epoch's day start and Rust's `%` would hand back a negative
    /// remainder - which would then format as a negative hour.
    fn seconds_since_midnight(&self, offset_hours: i32) -> u64 {
        let shifted = self.unix_seconds() + i64::from(offset_hours) * 3_600;
        shifted.rem_euclid(86_400) as u64
    }

    fn days_since_epoch(&self, offset_hours: i32) -> i64 {
        (self.unix_seconds() + i64::from(offset_hours) * 3_600).div_euclid(86_400)
    }
}

/// Days since 1970-01-01 to a civil `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, which is the standard formulation: shift
/// the epoch to 0000-03-01 so leap days land at the end of the year, then count.
/// Written out rather than pulled in as a dependency for eleven lines.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch so the leap day is the last day of the year.
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097; // 0..=146096
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // 0..=365
    let mp = (5 * day_of_year + 2) / 153; // 0..=11, March = 0
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32; // 1..=31
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // January = 1
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The `conditions:` section, by the name `%condition:<name>%` writes.
pub type Conditions = BTreeMap<String, Condition>;

/// Whether every expression in a condition has to hold, or only one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ConditionKind {
    /// Every expression must hold. TAB's default, and the only safe one: a
    /// condition that silently behaves as `OR` is a condition nobody can debug.
    #[default]
    And,
    /// At least one expression must hold.
    #[serde(alias = "or", alias = "Or")]
    Or,
}

/// The twelve comparisons a condition expression can make.
///
/// The order of the variants is not the order they are *tried* in; [`OPERATORS`]
/// is that, and it is what has to be ordered by specificity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// The twelve comparators, ordered so that `compile`, which takes the first
    /// symbol it finds, cannot pick a shorter one that a longer one contains.
    ///
    /// `>=` before `>` and `<=` before `<`; every `!…` before the bare form it
    /// negates; the two bare comparators last. See [`OPERATORS`].
    GreaterThanOrEqual,
    /// `<=`
    LessThanOrEqual,
    /// `!<-`
    NotContains,
    /// `!|-`
    NotStartsWith,
    /// `!-|`
    NotEndsWith,
    /// `<-`
    Contains,
    /// `|-`
    StartsWith,
    /// `-|`
    EndsWith,
    /// `!=`
    NotEquals,
    /// `=`
    Equals,
    /// `>`
    GreaterThan,
    /// `<`
    LessThan,
}

/// The comparators, in the order [`Expression::compile`] tries them.
///
/// `compile` takes the **first** symbol it finds, so this list has to be ordered
/// by specificity: for every pair here, the earlier symbol must not be a
/// substring of the later one. Otherwise the shorter one is found first and the
/// expression is split in the wrong place.
///
/// **This is not TAB's registration order, and it is deliberately not.**
/// `ConditionalExpression`'s static block registers `>=`, `>`, `<=`, `<` and only
/// then `!<-`, and its `compile` returns on the first key found, so
/// `%world%!<-lobby-` - which TAB's own wiki documents as *not contains* - is
/// detected as `<` and split into `%world%!` against `-lobby-`. Putting the two
/// bare comparators last is what makes the documented syntax work.
/// [`a_shorter_operator_is_never_found_before_a_longer_one`] holds the list to
/// the rule above, in every pair rather than a hand-picked few.
pub const OPERATORS: [Operator; 12] = [
    Operator::GreaterThanOrEqual,
    Operator::LessThanOrEqual,
    Operator::NotContains,
    Operator::NotStartsWith,
    Operator::NotEndsWith,
    Operator::Contains,
    Operator::StartsWith,
    Operator::EndsWith,
    Operator::NotEquals,
    Operator::Equals,
    Operator::GreaterThan,
    Operator::LessThan,
];

/// The node checks, which are not comparisons and so are tried last.
///
/// `permission:` is a suffix of `!permission:`, so the negative spelling has to
/// come first for the same reason [`OPERATORS`] is ordered the way it is.
const NOT_PERMISSION: &str = "!permission:";
const PERMISSION: &str = "permission:";

impl Operator {
    /// How the operator is written in a config.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::GreaterThanOrEqual => ">=",
            Self::GreaterThan => ">",
            Self::LessThanOrEqual => "<=",
            Self::LessThan => "<",
            Self::NotContains => "!<-",
            Self::NotStartsWith => "!|-",
            Self::NotEndsWith => "!-|",
            Self::Contains => "<-",
            Self::StartsWith => "|-",
            Self::EndsWith => "-|",
            Self::NotEquals => "!=",
            Self::Equals => "=",
        }
    }

    /// Whether the operator compares numbers rather than text.
    ///
    /// A non-numeric side is `0`, as in TAB: a placeholder that resolved to
    /// nothing is not a number, and refusing the comparison would turn every
    /// such typo into a condition that silently never passes.
    fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::GreaterThanOrEqual
                | Self::GreaterThan
                | Self::LessThanOrEqual
                | Self::LessThan
        )
    }

    /// The comparison, on two already-resolved sides.
    fn evaluate(self, left: &str, right: &str) -> bool {
        if self.is_numeric() {
            let (left, right) = (number(left), number(right));
            return match self {
                Self::GreaterThanOrEqual => left >= right,
                Self::GreaterThan => left > right,
                Self::LessThanOrEqual => left <= right,
                Self::LessThan => left < right,
                _ => unreachable!("is_numeric covered exactly the four above"),
            };
        }
        match self {
            Self::NotContains => !left.contains(right),
            Self::NotStartsWith => !left.starts_with(right),
            Self::NotEndsWith => !left.ends_with(right),
            Self::Contains => left.contains(right),
            Self::StartsWith => left.starts_with(right),
            Self::EndsWith => left.ends_with(right),
            Self::NotEquals => left != right,
            Self::Equals => left == right,
            _ => unreachable!("the numeric arm handled the other four"),
        }
    }
}

/// A side of a comparison, read as a number.
///
/// Thousands separators go first, because TAB strips them and `%online%` in a
/// sidebar is the kind of thing that gets an `&`-formatted comma put in front of
/// it. Anything unparseable is `0`, which is TAB's behaviour and the reason the
/// unresolved report exists.
fn number(side: &str) -> f64 {
    side.replace(',', "").parse().unwrap_or(0.0)
}

/// One condition expression, as written in the config.
///
/// Kept as the author's text rather than a compiled form. Three reasons, all of
/// them about being honest with the admin: `Config::to_yaml` has to write back
/// what it read, a malformed expression has to be *reported* by
/// `Config::problems` rather than refused at load, and the whole thing is a few
/// bytes of string scanning that happens once per refresh per condition - which
/// is not worth a second representation to save.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Expression {
    text: String,
}

impl Expression {
    /// An expression written as `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// The expression as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The first complaint about the expression, or [`None`] if it is usable.
    ///
    /// Shaped like [`crate::groups::name_problem`] and
    /// [`crate::config::BossBar::problem`] so `Config::problems` can collect them
    /// all the same way.
    pub fn problem(&self) -> Option<String> {
        self.compile().is_none().then(|| {
            format!(
                "'{}' is not a condition: expected a placeholder and an operator, such as \
                 '%ping%<50' or 'permission:my.node'",
                self.text
            )
        })
    }

    /// The operator and the two sides, or [`None`] if the expression is malformed.
    ///
    /// Takes the **first** operator whose symbol appears, in [`OPERATORS`] order,
    /// so that list is ordered by specificity rather than alphabetically.
    ///
    /// There is no need to strip placeholders before searching, as TAB does to
    /// work around a bug in *its* placeholder syntax: every operator symbol
    /// contains at least one character [`text::is_placeholder_name`] rejects, and
    /// none of them contains a `%`, so a symbol can be neither inside a `%…%` nor
    /// straddling one.
    /// [`no_operator_can_be_found_inside_a_placeholder`] holds both halves.
    ///
    /// The node checks are matched with [`str::strip_prefix`] rather than
    /// `contains`, so only one at the very start of the expression counts -
    /// `permission:` is all legal placeholder-name characters and would otherwise
    /// match inside `%…%`.
    fn compile(&self) -> Option<Compiled> {
        for operator in OPERATORS {
            if !self.text.contains(operator.symbol()) {
                continue;
            }
            let (left, right) = split(&self.text, operator.symbol())?;
            return Some(Compiled::Compare {
                operator,
                left,
                right,
            });
        }
        for (symbol, held) in [(NOT_PERMISSION, false), (PERMISSION, true)] {
            // `continue`, not `?`: the negative spelling is tried first precisely
            // so a positive one can still be reached, and `?` would give up here.
            let Some(node) = self.text.strip_prefix(symbol) else {
                continue;
            };
            let node = node.trim();
            return (!node.is_empty()).then(|| Compiled::Permission {
                node: node.to_string(),
                held,
            });
        }
        None
    }

    /// Whether the expression holds, or [`None`] if it cannot be decided here.
    ///
    /// [`None`] is a real answer, not a failure: a `permission:` expression has no
    /// player to ask about on a viewerless template such as the bossbar title, and
    /// a comparison on an unknown placeholder has nothing to compare. The caller
    /// leaves the text alone rather than showing half an answer.
    pub fn test(&self, view: &View<'_>) -> Option<bool> {
        let resolve = |side: &str| text::substitute(side, |name| view.resolve(name));
        match self.compile()? {
            Compiled::Permission { node, held } => Some(view.player?.has_permission(&node) == held),
            Compiled::Compare {
                operator,
                left,
                right,
            } => Some(operator.evaluate(&resolve(&left), &resolve(&right))),
        }
    }
}

/// What an [`Expression`] compiles to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Compiled {
    /// `left <operator> right`, both sides as written.
    Compare {
        operator: Operator,
        left: String,
        right: String,
    },
    /// A node check: `permission:<node>`, or `!permission:<node>` when the node
    /// must **not** be held.
    Permission {
        node: String,
        held: bool,
    },
}

/// The two sides of `text` around the first `symbol` outside a `%…%`.
///
/// The percent guard is what keeps `%a%=b` from splitting on an `=` that belongs
/// to nothing, and is also why a `%` in a condition expression has to be
/// balanced - which every real placeholder is.
fn split(text: &str, symbol: &str) -> Option<(String, String)> {
    let mut inside = false;
    for (index, ch) in text.char_indices() {
        if ch == '%' {
            inside = !inside;
        } else if !inside && text[index..].starts_with(symbol) {
            return Some((
                text[..index].trim().to_string(),
                text[index + symbol.len()..].trim().to_string(),
            ));
        }
    }
    None
}

/// A named test, and the two texts it chooses between.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Condition {
    /// The expressions, at least one.
    pub conditions: Vec<Expression>,
    /// Whether all of them or any of them must hold.
    #[serde(rename = "type")]
    pub kind: ConditionKind,
    /// Shown when the condition holds. Defaults to the literal `true`, so a
    /// condition used as a bare test can be nested in another one's expression.
    #[serde(rename = "true", default = "literal_true")]
    pub when_true: String,
    /// Shown when it does not. Defaults to the literal `false`, as above.
    #[serde(rename = "false", default = "literal_false")]
    pub when_false: String,
}

fn literal_true() -> String {
    "true".to_string()
}

fn literal_false() -> String {
    "false".to_string()
}

impl Condition {
    /// Whether the condition holds, or [`None`] if any part of it is undecidable.
    ///
    /// All-or-nothing on purpose, and evaluated in full rather than short
    /// circuited: a condition whose first expression holds and whose second
    /// cannot be read has not been met, and showing the `false:` text for it would
    /// report a fact that is not known.
    ///
    /// A condition with no expressions is [`None`] rather than vacuously true, so
    /// an empty `conditions:` list shows up as an unresolved placeholder instead
    /// of silently picking the `true:` text.
    pub fn test(&self, view: &View<'_>) -> Option<bool> {
        let mut expressions = self.conditions.iter();
        let mut held = expressions.next()?.test(view)?;
        for expression in expressions {
            let met = expression.test(view)?;
            match self.kind {
                ConditionKind::And => held &= met,
                ConditionKind::Or => held |= met,
            }
        }
        Some(held)
    }

    /// The chosen text, resolved as a template one level shallower than `depth`.
    ///
    /// [`None`] at [`MAX_DEPTH`], which is what stops `%condition:a%` whose
    /// `true:` text is `%condition:a%` from spinning inside a refresh.
    pub fn resolve(&self, view: &View<'_>, depth: u8) -> Option<String> {
        if depth == 0 {
            return None;
        }
        let chosen = if self.test(view)? {
            &self.when_true
        } else {
            &self.when_false
        };
        Some(text::substitute(chosen, |inner| view.resolve_at(inner, depth - 1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animation(interval: u64, texts: &[&str]) -> Animation {
        Animation {
            change_interval: interval,
            texts: texts.iter().map(|text| (*text).to_string()).collect(),
        }
    }

    #[test]
    fn the_clock_never_goes_backwards() {
        let clock = Clock::start();
        let first = clock.elapsed_ms();
        let second = clock.elapsed_ms();
        assert!(second >= first);
    }

    #[test]
    fn frames_come_from_the_wall_clock_not_the_refresh() {
        // 999ms, deliberately coprime with the 1000ms refresh. Sampled a second
        // apart that walks 0, 1, 2, 3 - which is the whole point: the frame is
        // chosen from a wall clock, so an animation faster than the refresh loop
        // skips frames instead of being slowed to match it.
        let config = Config::parse(
            "animations:\n  fast:\n    change-interval: 999\n    texts: ['a', 'b', 'c', 'd']\n",
        )
        .expect("valid");
        let animation = &config.animations["fast"];
        let mut seen = std::collections::BTreeSet::new();
        for millis in (0..4).map(|index| index * 1000) {
            if let Some(frame) = animation.frame_at(millis) {
                seen.insert(frame.to_string());
            }
        }
        assert_eq!(
            seen.len(),
            4,
            "every frame must be reachable across four refreshes"
        );
    }

    #[test]
    fn an_unknown_animation_has_no_frame() {
        assert_eq!(frame_text(&animation(100, &["a"]), 0).as_deref(), Some("a"));
        let empty = animation(100, &[]);
        assert_eq!(frame_text(&empty, 0), None);
    }

    #[test]
    fn the_built_in_list_has_no_duplicates() {
        let mut sorted = BUILT_INS.to_vec();
        sorted.sort_unstable();
        let count = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), count, "BUILT_INS contains a duplicate");
    }

    /// An alias that pointed at nothing would be a name in [`BUILT_INS`] that
    /// resolves to [`None`], which is the same as a typo: it would be documented
    /// and it would not work.
    #[test]
    fn the_every_alias_points_at_a_built_in() {
        for (alias, target) in BUILT_IN_ALIASES {
            assert!(BUILT_INS.contains(&target), "%{target}% is not a built-in");
            assert!(
                BUILT_INS.contains(&alias),
                "%{alias}% is an alias but is not in BUILT_INS"
            );
            assert_ne!(alias, target, "an alias to itself is a no-op");
        }
    }

    #[test]
    fn an_alias_that_is_not_one_is_left_alone() {
        assert_eq!(canonical_built_in("time"), "server_time");
        assert_eq!(canonical_built_in("date"), "server_date");
        assert_eq!(canonical_built_in("ping"), "ping");
        assert_eq!(canonical_built_in("nope"), "nope");
    }

    #[test]
    fn the_animation_prefix_is_not_a_built_in() {
        // Otherwise `%animation:logo%` would be ambiguous.
        assert!(!BUILT_INS.contains(&ANIMATION_PREFIX));
    }

    /// `BUILT_INS` is documentation and `View::built_in` is behaviour, and the one
    /// failure that matters is a name in the first with no arm in the second: it
    /// is listed in the shipped `config.yml` as working, and it renders as
    /// `%whatever%`.
    ///
    /// The list below is transcribed from the match arms, so it is a second thing
    /// that can be wrong. What it does buy is that either side changing on its own
    /// is caught, which is the realistic case - a name added to `BUILT_INS` and
    /// the arm forgotten, or an arm removed and the list left behind. A *typo
    /// inside* an arm is not caught here; it is caught by the placeholder showing
    /// up in the unresolved report.
    #[test]
    fn every_built_in_has_an_arm_and_every_arm_is_listed() {
        const ARMED: [&str; 20] = [
            "player", "viewer", "ping", "playtime", "world", "uuid", "permission_level", "health",
            "gamemode", "player_ip", "bedrock", "group", "online", "max_players", "worldonline",
            "server_name", "server_time", "server_date", "tps", "mspt",
        ];
        // The two aliases resolve before the match, so they are not arms of their
        // own - which is exactly why they have to be checked separately.
        let mut expected: Vec<&str> = ARMED.into();
        expected.extend(BUILT_IN_ALIASES.iter().map(|(alias, _)| *alias));
        // `BUILT_INS` is in the reading order of the match, not the alphabet.
        let mut listed = BUILT_INS.to_vec();
        expected.sort_unstable();
        listed.sort_unstable();
        assert_eq!(
            expected, listed,
            "BUILT_INS and the match arms have drifted apart",
        );
    }

    /// Same argument for conditions: `%condition:ping%` must not be readable as a
    /// built-in called `ping`.
    #[test]
    fn the_condition_prefix_is_not_a_built_in() {
        assert!(!BUILT_INS.contains(&CONDITION_PREFIX));
        assert!(!BUILT_IN_ALIASES.iter().any(|(alias, _)| alias.contains(":")));
    }

    #[test]
    fn a_placeholder_is_detected_after_substitution() {
        assert!(has_placeholder("%player%"));
        assert!(!has_placeholder("plain"));
        assert!(!has_placeholder("100%"));
    }

    #[test]
    fn placeholders_are_counted() {
        assert_eq!(count_placeholders("%a% and %b%"), 2);
        assert_eq!(count_placeholders("%a% and %a%"), 2);
        assert_eq!(count_placeholders("none"), 0);
    }

    /// The count and the report have to agree, so they read the same list. If they
    /// ever diverged, the report would name placeholders the count says are not
    /// there, or the other way round.
    #[test]
    fn the_names_and_the_count_agree() {
        for text in ["%a% and %b%", "%a% and %a%", "none", "50% off %a%", "%a%!"] {
            assert_eq!(count_placeholders(text), placeholder_names(text).len(), "{text}");
            assert_eq!(has_placeholder(text), !placeholder_names(text).is_empty(), "{text}");
        }
        assert_eq!(placeholder_names("%a% and %a%"), ["a", "a"]);
        assert_eq!(placeholder_names("50% off %a%"), ["a"]);
    }

    #[test]
    fn the_date_conversion_agrees_with_the_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        // 2024-02-29, the day the whole algorithm exists for.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
        // 2000-03-01, a century leap year.
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[test]
    fn the_date_conversion_handles_dates_before_the_epoch() {
        // The `div_euclid` in the era calculation is what makes this work, and a
        // server with a wrong clock can produce it.
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(-719_468), (0, 3, 1));
    }

    #[test]
    fn a_consecutive_run_of_days_is_monotonic() {
        let mut previous = civil_from_days(0);
        for day in 1..=(365 * 4 + 1) {
            let current = civil_from_days(day);
            let is_next = current.0 == previous.0
                && current.1 == previous.1
                && current.2 == previous.2 + 1;
            assert!(
                is_next || current > previous,
                "day {day}: {current:?} does not follow {previous:?}"
            );
            previous = current;
        }
    }

    #[test]
    fn the_clock_reads_the_real_time() {
        let clock = world_clock();
        assert!((0..86_400).contains(&clock.seconds_since_midnight(0)));
        let (year, month, day) = civil_from_days(clock.days_since_epoch(0));
        assert!(
            year >= 2024,
            "the clock is not reading the real time: {year}-{month}-{day}"
        );
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
    }

    /// The whole reason `utc-offset-hours` exists: the clock is UTC, the server
    /// log is not, and an admin has to be able to line them up.
    ///
    /// Asserted as a *relationship* between two offsets rather than against "now",
    /// because an offset is a shift and a shift is only checkable by comparing
    /// two of them. The day-count check is what catches a `%` that returned a
    /// negative remainder for a zone behind UTC.
    #[test]
    fn an_offset_shifts_the_clock_by_whole_hours() {
        let clock = world_clock();
        const DAY: i64 = 86_400;
        for hours in [-14, -12, -5, -1, 0, 1, 5, 7, 13, 14] {
            let shown = clock.seconds_since_midnight(hours);
            assert!((0..86_400).contains(&shown), "{hours}h produced {shown}");

            let expect = (clock.seconds_since_midnight(0) as i64
                + i64::from(hours) * 3_600)
                .rem_euclid(DAY);
            assert_eq!(shown as i64, expect, "{hours}h is not a whole shift of {hours} hours");
        }
        // A zone ahead is never behind, in either the time of day or the date,
        // and one behind is never ahead. A sign error here is invisible in a
        // screenshot and obvious on a real server.
        assert!(clock.days_since_epoch(7) >= clock.days_since_epoch(0));
        assert!(clock.days_since_epoch(-7) <= clock.days_since_epoch(0));
    }

    /// 12-hour with AM/PM, because that is the shape the server's own log uses and
    /// two clocks that differ only by am/pm read as broken rather than as formats.
    ///
    /// Pinned to exact seconds, which is the point of splitting
    /// [`format_seconds`] out: midnight and noon are the two hours a 12-hour
    /// formatter gets wrong, and neither can be reached by a test that reads the
    /// current time.
    #[test]
    fn a_time_is_twelve_hour_with_a_meridiem() {
        assert_eq!(format_seconds(0), "12:00:00 AM");
        assert_eq!(format_seconds(12 * 3_600), "12:00:00 PM");
        assert_eq!(format_seconds(13 * 3_600 + 5 * 60 + 9), "1:05:09 PM");
        assert_eq!(format_seconds(23 * 3_600 + 59 * 60 + 59), "11:59:59 PM");
        // The minute before noon, and the minute before midnight: the boundaries
        // where an off-by-one changes the hour *and* the meridiem.
        assert_eq!(format_seconds(12 * 3_600 - 1), "11:59:59 AM");
        assert_eq!(format_seconds(86_399), "11:59:59 PM");

        // Every hour of the day is between 1 and 12, and carries a meridiem.
        for hour in 0..24 {
            let shown = format_seconds(hour * 3_600);
            let number: u32 = shown[..shown.find(':').expect("an hour")].parse().expect("a number");
            assert!((1..=12).contains(&number), "hour {hour} printed {number}: {shown}");
            assert!(
                shown.ends_with(" AM") || shown.ends_with(" PM"),
                "hour {hour} printed {shown}"
            );
        }
    }

    #[test]
    fn an_unresolved_optional_prefix_is_blanked_but_a_typo_is_not() {
        // The shipped case: no PumpkinPerm, no PumpkinPIE.
        assert_eq!(
            blank_unresolved_prefix("%perm_prefix%blockrnc  Ping: 42ms"),
            "blockrnc  Ping: 42ms"
        );
        // A resolved one is `§`-coded and never matches, so it survives whole.
        assert_eq!(
            blank_unresolved_prefix("\u{a7}7\u{a7}l[MEMBER] blockrnc"),
            "\u{a7}7\u{a7}l[MEMBER] blockrnc"
        );
        // Every other placeholder keeps the leave-it-as-written rule.
        assert_eq!(
            blank_unresolved_prefix("%perm_prefix%%nope_nope%Steve"),
            "%nope_nope%Steve"
        );
        // Nothing to do is the common case and must not reallocate meaningfully.
        assert_eq!(blank_unresolved_prefix("just a name"), "just a name");
    }

    #[test]
    fn an_unresolved_prefix_is_removed_wherever_the_format_puts_it() {
        assert_eq!(
            blank_unresolved_prefix("Steve %perm_prefix%"),
            "Steve"
        );
        // Not at the start, so there is no leading space to trim.
        assert_eq!(
            blank_unresolved_prefix("Rank: %perm_prefix% - Steve"),
            "Rank: - Steve"
        );
    }

    #[test]
    fn a_confusable_name_is_not_blanked() {
        // `perm_prefix` on its own is not the placeholder, and neither is a
        // placeholder that merely starts with it.
        assert_eq!(blank_unresolved_prefix("perm_prefix is a word"), "perm_prefix is a word");
        assert_eq!(
            blank_unresolved_prefix("%perm_prefix_extra%Steve"),
            "%perm_prefix_extra%Steve"
        );
    }

    // ---- conditions -------------------------------------------------------

    /// The op level as the config and a condition both spell it, so
    /// `%permission_level%>=2` cannot mean something different from
    /// `op-level-groups: { two: … }`.
    #[test]
    fn op_levels_are_the_numbers_the_config_uses() {
        assert_eq!(op_level(PermissionLevel::Zero), 0);
        assert_eq!(op_level(PermissionLevel::One), 1);
        assert_eq!(op_level(PermissionLevel::Two), 2);
        assert_eq!(op_level(PermissionLevel::Three), 3);
        assert_eq!(op_level(PermissionLevel::Four), 4);
    }

    #[test]
    fn game_modes_are_spelled_the_way_the_command_spells_them() {
        assert_eq!(game_mode(GameMode::Survival), "survival");
        assert_eq!(game_mode(GameMode::Creative), "creative");
        assert_eq!(game_mode(GameMode::Adventure), "adventure");
        assert_eq!(game_mode(GameMode::Spectator), "spectator");
    }

    /// `compile` takes the *first* symbol it finds, so for it to pick the right
    /// one, no symbol may be a substring of a symbol tried after it. Checked over
    /// every pair rather than a hand-picked few, because a pair nobody thought of
    /// is exactly the pair that gets added later by mistake.
    #[test]
    fn a_shorter_operator_is_never_found_before_a_longer_one() {
        for (earlier, first) in OPERATORS.iter().enumerate() {
            for later in &OPERATORS[earlier + 1..] {
                assert!(
                    !later.symbol().contains(first.symbol()),
                    "`{}` is tried before `{}`, which contains it, so `{}` would win",
                    first.symbol(),
                    later.symbol(),
                    first.symbol(),
                );
            }
        }
        // The node checks are not in `OPERATORS`, but the same rule: a negative
        // check has to be found before the positive one, or `!permission:x` reads
        // as a check for a node called `!permission:x`.
        assert!(NOT_PERMISSION.ends_with(PERMISSION));
    }

    /// The two documented spellings that a naive first-match scan gets wrong. TAB
    /// registers `>=` before `>` and `!<-` before `<-`, but registers the bare `<`
    /// *before* `!<-`, so it splits `%world%!<-lobby-` at that `<`.
    #[test]
    fn a_negated_comparison_is_not_read_as_the_bare_operator_inside_it() {
        for (text, expected, sides) in [
            ("%world%!<-lobby-", Operator::NotContains, ("%world%", "lobby-")),
            ("%world%<-lobby-", Operator::Contains, ("%world%", "lobby-")),
            ("%ping%>=50", Operator::GreaterThanOrEqual, ("%ping%", "50")),
            ("%ping%<=50", Operator::LessThanOrEqual, ("%ping%", "50")),
            ("%ping%!=50", Operator::NotEquals, ("%ping%", "50")),
            ("%ping%!|-50", Operator::NotStartsWith, ("%ping%", "50")),
            ("%ping%!-|50", Operator::NotEndsWith, ("%ping%", "50")),
            // The bare ones still work when nothing longer is present.
            ("%ping%>50", Operator::GreaterThan, ("%ping%", "50")),
            ("%ping%<50", Operator::LessThan, ("%ping%", "50")),
        ] {
            assert_eq!(
                Expression::new(text).compile(),
                Some(Compiled::Compare {
                    operator: expected,
                    left: sides.0.to_string(),
                    right: sides.1.to_string(),
                }),
                "{text}",
            );
        }
    }

    /// `compile` searches the raw text with `contains`, so this is what makes that
    /// safe: a symbol cannot be inside a `%…%`, because at least one of its
    /// characters is not legal in a placeholder name, and it cannot straddle one,
    /// because no symbol contains a `%`.
    #[test]
    fn no_operator_can_be_found_inside_a_placeholder() {
        for operator in OPERATORS {
            let symbol = operator.symbol();
            assert!(!symbol.contains('%'), "{symbol} contains a percent sign");
            assert!(
                symbol
                    .chars()
                    .any(|ch| !text::is_placeholder_name(&ch.to_string())),
                "every character of {symbol} is legal in a placeholder name, so `{symbol}` could \
                 be found inside one",
            );
        }
    }

    /// The one operator that *is* all legal placeholder-name characters, and the
    /// reason it is matched with `strip_prefix` rather than `contains`.
    #[test]
    fn a_node_check_inside_a_placeholder_is_not_a_node_check() {
        assert!(text::is_placeholder_name(PERMISSION));
        assert_eq!(Expression::new("%xpermission:y%").compile(), None);
    }

    #[test]
    fn an_expression_splits_around_its_operator() {
        assert_eq!(
            split("%ping%>=50", ">="),
            Some(("%ping%".to_string(), "50".to_string()))
        );
        // The percent guard: an operator character inside a placeholder is not
        // the operator.
        assert_eq!(split("%a%=b", "="), Some(("%a%".to_string(), "b".to_string())));
        assert_eq!(split("no operator here", ">="), None);
        // An unbalanced `%` swallows the rest, which is why a condition cannot
        // compare against a literal containing one.
        assert_eq!(split("100%>=50", ">="), None);
    }

    #[test]
    fn every_operator_compiles_and_splits() {
        for operator in OPERATORS {
            let text = format!("%a%{}b", operator.symbol());
            assert_eq!(
                Expression::new(&text).compile(),
                Some(Compiled::Compare {
                    operator,
                    left: "%a%".to_string(),
                    right: "b".to_string(),
                }),
                "{text}",
            );
            assert_eq!(Expression::new(text).problem(), None);
        }
    }

    #[test]
    fn a_node_check_compiles() {
        for (text, held) in [("permission:my.node", true), ("!permission:my.node", false)] {
            assert_eq!(
                Expression::new(text).compile(),
                Some(Compiled::Permission {
                    node: "my.node".to_string(),
                    held,
                }),
                "{text}",
            );
            assert_eq!(Expression::new(text).problem(), None, "{text}");
        }
        // Trailing space is trimmed, because YAML leaves it there often enough.
        assert_eq!(
            Expression::new("permission: my.node ").compile(),
            Some(Compiled::Permission {
                node: "my.node".to_string(),
                held: true,
            }),
        );
    }

    /// A negative check has to be found before the positive one, or
    /// `!permission:x` would read as a check for the node `!permission:x`.
    #[test]
    fn a_negated_node_check_is_not_read_as_a_node_named_with_a_bang() {
        assert_eq!(
            Expression::new("!permission:tab.staff").compile(),
            Some(Compiled::Permission {
                node: "tab.staff".to_string(),
                held: false,
            }),
        );
    }

    #[test]
    fn a_malformed_expression_is_reported_rather_than_guessed_at() {
        for bad in ["", "   ", "just some text", "%ping%", "permission:", "  !permission: "] {
            let expression = Expression::new(bad);
            assert_eq!(expression.compile(), None, "{bad:?} should not compile");
            let problem = expression.problem().expect("a complaint");
            assert!(problem.contains("is not a condition"), "{problem}");
        }
    }

    /// An empty side is legal, and is how TAB spells "is this placeholder empty?".
    /// It compares the empty string, which is the point.
    #[test]
    fn an_empty_side_is_a_side_rather_than_a_mistake() {
        for (text, operator, left, right) in [
            ("%missing%=", Operator::Equals, "%missing%", ""),
            ("%missing%!=", Operator::NotEquals, "%missing%", ""),
            ("=50", Operator::Equals, "", "50"),
            (">=50", Operator::GreaterThanOrEqual, "", "50"),
        ] {
            assert_eq!(
                Expression::new(text).compile(),
                Some(Compiled::Compare {
                    operator,
                    left: left.to_string(),
                    right: right.to_string(),
                }),
                "{text}",
            );
        }
    }

    #[test]
    fn an_expression_survives_a_yaml_round_trip() {
        // `Config::to_yaml` has to write back what it read, so the raw text is
        // the representation and not an accident of the derive.
        let expression = Expression::new("%ping%<50");
        let yaml = serde_norway::to_string(&expression).expect("serialises");
        assert_eq!(yaml.trim(), "'%ping%<50'");
        assert_eq!(serde_norway::from_str::<Expression>(&yaml).unwrap(), expression);
    }

    /// One operator and the outcomes it has to produce, in the order the cases
    /// appear in the message when one is wrong.
    type Outcome<'a> = (&'a str, &'a str, bool);

    /// Every comparator, on both sides of its own symbol, so a swapped pair of
    /// arms cannot pass.
    #[test]
    fn every_comparator_evaluates_both_ways() {
        let cases: [(Operator, &[Outcome<'_>]); 12] = [
            (Operator::GreaterThanOrEqual, &[("2", "2", true), ("3", "2", true), ("1", "2", false)]),
            (Operator::GreaterThan, &[("2", "2", false), ("3", "2", true), ("1", "2", false)]),
            (Operator::LessThanOrEqual, &[("2", "2", true), ("1", "2", true), ("3", "2", false)]),
            (Operator::LessThan, &[("2", "2", false), ("1", "2", true), ("3", "2", false)]),
            (Operator::NotContains, &[("lobby-1", "lobby-", false), ("world", "lobby-", true)]),
            (Operator::NotStartsWith, &[("lobby-1", "lobby-", false), ("x-lobby", "lobby-", true)]),
            (Operator::NotEndsWith, &[("world_nether", "nether", false), ("nether_x", "nether", true)]),
            (Operator::Contains, &[("lobby-1", "lobby-", true), ("world", "lobby-", false)]),
            (Operator::StartsWith, &[("lobby-1", "lobby-", true), ("x-lobby", "lobby-", false)]),
            (Operator::EndsWith, &[("world_nether", "nether", true), ("nether_x", "nether", false)]),
            (Operator::NotEquals, &[("a", "a", false), ("a", "b", true)]),
            (Operator::Equals, &[("a", "a", true), ("a", "b", false)]),
        ];
        // Every operator has to be here, or an arm could be added and never tried.
        assert_eq!(cases.len(), OPERATORS.len());
        for (operator, expectations) in cases {
            assert!(
                !expectations.is_empty(),
                "{operator:?} has no cases at all, so nothing was checked"
            );
            for (left, right, expected) in expectations {
                assert_eq!(
                    operator.evaluate(left, right),
                    *expected,
                    "{left} {} {right}",
                    operator.symbol()
                );
            }
        }
    }

    /// A `=` with nothing on the right is how TAB asks "is this empty?". With
    /// nothing on the left it asks the same of a constant.
    #[test]
    fn an_empty_side_tests_for_emptiness() {
        assert_eq!(
            Expression::new("%missing%=").compile(),
            Some(Compiled::Compare {
                operator: Operator::Equals,
                left: "%missing%".to_string(),
                right: String::new(),
            }),
        );
        assert!(Operator::Equals.evaluate("", ""));
        assert!(!Operator::Equals.evaluate("something", ""));
    }

    #[test]
    fn a_numeric_side_tolerates_thousands_separators() {
        assert_eq!(number("1,234"), 1234.0);
        // TAB's behaviour, and the reason a typo cannot silently fail a
        // comparison: an unreadable number is zero, not an error.
        assert_eq!(number("%ping%"), 0.0);
        assert_eq!(number(""), 0.0);
        assert!(number("50") < 100.0);
    }
}
