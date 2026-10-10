//! `%playtime%`: how long a player has been connected, across every session.
//!
//! ## What is being measured
//!
//! **Time connected, not time on the server.** That is the only thing this
//! plugin can measure honestly on this WIT: a scheduled task fires for whoever
//! is connected ([`crate::restart_refresh`]), so the gap between two refreshes
//! is the time those players were connected. AFK is not detected - there is no
//! movement event wired up, and inferring it from a rising ping would report an
//! estimate where a count is available.
//!
//! ## Why it has to be a file
//!
//! A total that resets on restart is not a total. The plugin already has a
//! preopened data folder - `FS_READ_DATA` and `FS_WRITE_DATA` in the plugin
//! metadata are what make every `std::fs` call in this crate work, and
//! [`crate::storage`] uses the same folder for `config.yml` - so the totals go to
//! `playtime.yml` beside it, as a plain `uuid: seconds` map.
//!
//! The key is the **uuid**, not the name. A name is what an admin can change and
//! what two accounts may share, so keying on one would hand a departing player's
//! hours to whoever next connects under it.
//!
//! ## Why the clock is an argument
//!
//! Every function that moves a number takes `now_ms`, and nothing in the
//! arithmetic reads the real clock. That is what makes this module testable on
//! the host target at all: the plugin WIT does not exist there, so anything that
//! calls the host traps rather than failing ([`crate::pie`] documents the same
//! trap for the provider). The one place a real clock is read is [`now_ms`],
//! which is why the tests can say "a player played 30 seconds" and mean it.
//!
//! ## The one lock held across a host call
//!
//! [`write`] takes [`WRITING`] *before* it takes its snapshot and holds it across
//! the `std::fs` write. That is the exception the header of [`crate::state`]
//! warns about, and it is the right way round for this module:
//!
//! - The hazard a lock across a host call guards against is re-entry, and
//!   `std::fs` on a preopened path cannot deliver an event into this plugin: the
//!   only handlers are join and leave. Nothing can arrive while the write is in
//!   flight, so nothing can be re-entered.
//! - Without the lock the snapshot would be taken *before* the write, and a
//!   `PlayerLeaveEvent` landing in the gap would bank its slice and then have it
//!   overwritten by the older rows this call was carrying. Losing a disconnect is
//!   the one thing here that must not happen, and taking the lock first makes
//!   the later write always hold the later state.
//!
//! Everything else follows the module's other rule: no lock is held while the
//! host is called, and the player list is read before [`STORE`] is taken at all.
//!
//! ## What a new player sees
//!
//! `0s`, from the first refresh onwards. A player who has just connected really
//! has played no time yet, and `0s` is a true answer where "unknown" would leave
//! `%playtime%` sitting on screen looking exactly like a typo.

use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard, OnceLock},
    time::Instant,
};

use pumpkin_plugin_api::player::Player;
use tracing::{error, info};

use crate::{state::recover, storage};

/// Seconds in a minute, an hour and a day, as [`format`] counts them.
const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// How long between writes during a refresh.
///
/// A save rewrites the whole file, so doing it on every refresh is one
/// `std::fs` call per `refresh-interval-ticks` - twenty a second at the fastest
/// setting the config allows, for a few bytes per player. A disconnect saves
/// immediately and so does [`crate::PumpkinTAB::on_unload`], so the only thing
/// this interval can lose is the last half-minute of a crash.
const SAVE_INTERVAL_MS: u64 = 30_000;

/// One player's total, and the mark the session in progress is measured from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Played {
    /// Whole seconds banked from every session so far.
    seconds: u64,
    /// The millisecond mark the last whole second was banked at, or [`None`]
    /// while the player is not connected.
    ///
    /// [`None`] covers both "never seen" and "was here, has gone", and it is
    /// what makes the time spent away from the server impossible to count: a
    /// slice is only ever banked by [`Self::bank`] from a mark that exists, and
    /// a mark only exists while somebody is online.
    mark_ms: Option<u64>,
}

impl Played {
    /// Banks the whole seconds since the last mark and moves the mark on.
    ///
    /// The mark advances by the **whole slice**, not by the second that was
    /// banked, so the sub-second remainder carries into the next call. Without
    /// that, a server whose refresh is faster than once a second banks nothing
    /// at all - every slice truncates on its own - which is a legal
    /// `refresh-interval-ticks` and a completely silent failure.
    ///
    /// A player with no mark - a first sighting, or somebody coming back - banks
    /// nothing and is simply marked. That single branch is both "accumulates
    /// across sessions" and "the time away is not counted".
    fn bank(&mut self, now_ms: u64) {
        let Some(mark) = self.mark_ms else {
            self.mark_ms = Some(now_ms);
            return;
        };
        let slice = now_ms.saturating_sub(mark);
        self.seconds = self.seconds.saturating_add(slice / 1000);
        // The mark advances to the last whole second of the slice, so the
        // sub-second remainder carries - and never *below* where it was. The
        // `max` is only reachable if `now_ms` went backwards, which a monotonic
        // `Instant` should not do; if it ever does, keeping the old mark is what
        // stops the whole interval being counted a second time on the way back
        // up, and `saturating_sub` is what stops it wrapping into the far future.
        self.mark_ms = Some(now_ms.saturating_sub(slice % 1000).max(mark));
    }

    /// Banks the running slice and stops the clock.
    ///
    /// The total is kept and only the mark goes, so a rejoin continues from
    /// where the player left off.
    fn stop(&mut self, now_ms: u64) {
        self.bank(now_ms);
        self.mark_ms = None;
    }
}

/// `seconds` as `45s`, `1m 30s`, `1h 45m 27s` or `4d 3h 17m 48s`.
///
/// **No weeks, and nothing coarser than days.** The largest non-zero unit
/// anchors the string and every unit below it is written even when it is zero,
/// so `1h 0m 5s` does not collapse to `1h 5s` - a unit that is missing reads
/// as a number the player has, and they do not. [`a_week_is_never_printed`] and
/// [`formatting_is_reversible`] hold the two halves of that.
///
/// Zero is `0s`. It is the common case - the first second of every player's
/// first session, and every second of a joiner's first refresh - and anything
/// else would make the tab list look broken on arrival.
pub fn format(seconds: u64) -> String {
    let days = seconds / DAY;
    let hours = seconds % DAY / HOUR;
    let minutes = seconds % HOUR / MINUTE;

    let mut out = String::new();
    if days > 0 {
        push_unit(&mut out, days, 'd');
        push_unit(&mut out, hours, 'h');
        push_unit(&mut out, minutes, 'm');
    } else if hours > 0 {
        push_unit(&mut out, hours, 'h');
        push_unit(&mut out, minutes, 'm');
    } else if minutes > 0 {
        push_unit(&mut out, minutes, 'm');
    }
    push_unit(&mut out, seconds % MINUTE, 's');
    out
}

/// Appends `value` and `unit`, space-separated once there is anything to be
/// separated from.
///
/// The guard is only "is there anything here yet", never "is the value zero".
/// That is what makes the seconds the one unit that always appears: every branch
/// in [`format`] leaves the string empty before the seconds are written, so `0s`
/// prints as `0s` rather than as nothing - while a *zeroed* unit above it still
/// prints, because by then the string is not empty.
fn push_unit(out: &mut String, value: u64, unit: char) {
    use std::fmt::Write as _;

    if !out.is_empty() {
        out.push(' ');
    }
    // `write!` rather than `format!` and pushing twice: this runs for every
    // player on every refresh, so the temporary is worth avoiding.
    let _ = write!(out, "{value}{unit}");
}

/// Everything the store holds, in one slot.
struct Store {
    /// The data folder, or [`None`] before [`install`] has run.
    ///
    /// Held here rather than read back out of `DATA_FOLDER` so that a save has
    /// everything it needs from a single lock, and so that a store which was
    /// never installed writes nothing rather than guessing at a path.
    folder: Option<String>,
    /// The totals, by player uuid.
    played: BTreeMap<String, Played>,
    /// When the file was last written, for [`due`].
    saved_ms: Option<u64>,
    /// The file could not be read, so it is not written either.
    ///
    /// Overwriting a file that failed to parse would destroy the very numbers
    /// the parse was complaining about, and the only symptom would be a
    /// playtime that quietly restarted from zero on every restart - with no
    /// error anywhere after the first. So the store keeps running on zero and
    /// says so exactly once, in [`install`].
    unwritable: bool,
}

static STORE: Mutex<Store> = Mutex::new(Store {
    folder: None,
    played: BTreeMap::new(),
    saved_ms: None,
    unwritable: false,
});

/// Held for the length of one `std::fs` write. See the module header.
static WRITING: Mutex<()> = Mutex::new(());

/// The origin of every `mark_ms`.
///
/// One [`Instant`] rather than a tick count: `refresh-interval-ticks` is a
/// config setting that can be any number of ticks, and only differences are ever
/// taken, so the origin does not have to mean anything.
static TIMELINE: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    TIMELINE.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Reads the stored totals out of `folder`. Called once, from `on_load`.
///
/// **Deliberately not on a `/tab reload`.** The reload path re-reads
/// `config.yml` and nothing else; re-reading this file would throw away every
/// second banked since the plugin loaded, which is exactly the running total the
/// tab list is showing.
///
/// An absent file is not a fault and is not reported: it is a first run, and the
/// store is written on the first save.
pub fn install(folder: &str) {
    let mut store = recover(STORE.lock());
    store.folder = Some(folder.to_string());
    store.played.clear();
    store.saved_ms = None;
    store.unwritable = false;

    match storage::load_playtimes(folder) {
        Ok(totals) => {
            let stored = totals.len();
            store.played = totals
                .into_iter()
                .map(|(uuid, seconds)| (uuid, Played { seconds, mark_ms: None }))
                .collect();
            if stored > 0 {
                info!("{stored} stored playtime(s) read from {folder}");
            }
        }
        Err(problem) => {
            error!(
                "{problem}. %playtime% starts from 0s and the file will not be written again, so \
                 nothing in it is lost - fix it or delete it and restart to pick it up"
            );
            store.unwritable = true;
        }
    }
}

/// Banks the time since `player` was last seen.
///
/// Called once per player per refresh, **before** their templates are resolved,
/// so `%playtime%` shows the slice that has just elapsed rather than the one
/// before it. A join does not call this: it cannot bank anything, because the
/// mark that a slice is counted from does not exist until a refresh has made it.
pub fn bank(player: &Player) {
    let uuid = key(player);
    let now = now_ms();
    // `or_default` rather than `entry(...).or_insert(Played::default())`: a
    // player who has never been seen banks nothing on the first call, so an
    // explicit zero entry would only add a row to the file.
    recover(STORE.lock())
        .played
        .entry(uuid)
        .or_default()
        .bank(now);
    // The lock is released here, by the tail of the statement, and nothing is
    // called on the host while it is held.
}

/// Banks the last slice of a departing player's session and writes the file.
///
/// The total is kept and only the mark is cleared, so a rejoin continues from
/// where they left off rather than from zero.
pub fn player_left(player: &Player) {
    let uuid = key(player);
    let now = now_ms();
    {
        let mut store = recover(STORE.lock());
        // `get_mut`, not `entry`: a player who connected and left inside one
        // refresh interval never earned a second, and has nothing worth a row.
        if let Some(played) = store.played.get_mut(&uuid) {
            played.stop(now);
        }
    }
    // Outside the lock, and after it. `write` takes its own, so this cannot
    // deadlock against a refresh that is already writing.
    write();
}

/// Writes the totals now, whatever [`due`] says.
///
/// For the paths that cannot wait: a disconnect, and [`crate::PumpkinTAB::on_unload`].
pub fn save() {
    recover(STORE.lock()).saved_ms = Some(now_ms());
    write();
}

/// Whether `now_ms` is far enough past the last write to save again.
///
/// Split out of [`save_if_due`] so the throttle is a plain comparison rather
/// than something only the host can exercise.
fn due(saved_ms: Option<u64>, now_ms: u64) -> bool {
    saved_ms.is_none_or(|saved| now_ms.saturating_sub(saved) >= SAVE_INTERVAL_MS)
}

/// Writes the totals if [`due`], for the refresh path.
pub fn save_if_due() {
    let now = now_ms();
    {
        let mut store = recover(STORE.lock());
        if !due(store.saved_ms, now) {
            return;
        }
        store.saved_ms = Some(now);
    }
    write();
}

/// Writes the totals, if there is anywhere to write them to.
fn write() {
    // Taken *before* the snapshot, and held across the write. See the module
    // header: the snapshot is what has to be fresh, not the lock.
    let _writing: MutexGuard<'_, ()> = recover(WRITING.lock());
    let (folder, totals, unwritable) = {
        let store = recover(STORE.lock());
        (
            store.folder.clone(),
            store.rows(),
            store.unwritable,
        )
    };
    if unwritable {
        return;
    }
    let Some(folder) = folder else {
        return;
    };
    if let Err(problem) = storage::save_playtimes(&folder, &totals) {
        // Every `save_if_due` window, not every refresh: the throttle means this
        // repeats about twice a minute at worst, which is a complaint rate an
        // admin can read rather than one that scrolls the answer off the screen.
        error!("{problem}; the playtime totals are still in memory and will be written again");
    }
}

/// The seconds banked for `uuid`, or `0`.
///
/// Zero rather than "unknown" for somebody the store has never seen: a player
/// who has just connected really has played no time, so `0s` is a true answer
/// where [`None`] would leave `%playtime%` on screen looking like a typo.
/// The placeholder still answers [`None`] for a template with **no player** -
/// see `render::View::player` - so a server-wide header cannot borrow one
/// viewer's number.
pub fn seconds(uuid: &str) -> u64 {
    recover(STORE.lock())
        .played
        .get(uuid)
        .map_or(0, |played| played.seconds)
}

/// `%playtime%` for `player`.
///
/// Takes the [`Player`] rather than a uuid because reading the uuid is a host
/// call, and it has to happen before [`STORE`] is locked.
pub fn of(player: &Player) -> String {
    format(seconds(&key(player)))
}

/// The uuid `player` is stored under.
fn key(player: &Player) -> String {
    pumpkin_plugin_api::uuid::to_string(player.get_id())
}

impl Store {
    /// The banked seconds, by uuid.
    ///
    /// Marks are left out on purpose: a session in progress has not earned its
    /// second yet, so writing a number that is about to change buys nothing and
    /// costs the rewrite that [`SAVE_INTERVAL_MS`] is there to avoid.
    fn rows(&self) -> BTreeMap<String, u64> {
        self.played
            .iter()
            .map(|(uuid, played)| (uuid.clone(), played.seconds))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player running through `bank` at `step_ms` intervals for `steps` of
    /// them, which is the shape of a live session.
    fn session(steps: u64, step_ms: u64) -> Played {
        let mut played = Played::default();
        for step in 0..=steps {
            played.bank(step * step_ms);
        }
        played
    }

    // ---- the formatter ---------------------------------------------------

    #[test]
    fn a_new_player_reads_as_zero() {
        // What a player sees in their first refresh, and for the whole first
        // second of their first session.
        assert_eq!(format(0), "0s");
    }

    #[test]
    fn under_a_minute_is_seconds_alone() {
        assert_eq!(format(1), "1s");
        assert_eq!(format(45), "45s");
        assert_eq!(format(MINUTE - 1), "59s");
    }

    #[test]
    fn minutes_and_seconds() {
        assert_eq!(format(MINUTE), "1m 0s");
        assert_eq!(format(90), "1m 30s");
        assert_eq!(format(2 * MINUTE - 1), "1m 59s");
    }

    #[test]
    fn hours_minutes_and_seconds() {
        assert_eq!(format(HOUR), "1h 0m 0s");
        assert_eq!(format(HOUR + 45 * MINUTE + 27), "1h 45m 27s");
    }

    #[test]
    fn days_hours_minutes_and_seconds() {
        let four = 4 * DAY + 3 * HOUR + 17 * MINUTE + 48;
        assert_eq!(format(four), "4d 3h 17m 48s");
    }

    /// A unit that is zero below the anchor is written, not dropped: `1h 0m 5s`
    /// rather than `1h 5s`, because the absent hour is a fact about the player
    /// and dropping it invents a number they do not have.
    #[test]
    fn a_zero_unit_below_the_anchor_is_still_shown() {
        assert_eq!(format(HOUR + 5), "1h 0m 5s");
        assert_eq!(format(HOUR + MINUTE), "1h 1m 0s");
        assert_eq!(format(DAY + 1), "1d 0h 0m 1s");
        assert_eq!(format(DAY), "1d 0h 0m 0s");
    }

    /// The rule with no way to opt out: days is the coarsest unit there is, so a
    /// year of playtime is still days and never `52w 1d`.
    #[test]
    fn a_week_is_never_printed() {
        for (seconds, expected) in [
            (7 * DAY, "7d 0h 0m 0s"),
            (7 * DAY + 3, "7d 0h 0m 3s"),
            (8 * DAY, "8d 0h 0m 0s"),
            (14 * DAY, "14d 0h 0m 0s"),
            (30 * DAY, "30d 0h 0m 0s"),
            (365 * DAY, "365d 0h 0m 0s"),
        ] {
            let rendered = format(seconds);
            assert_eq!(rendered, expected);
            assert!(!rendered.contains('w'), "a week was printed: {rendered}");
        }
    }

    /// The formatter must be able neither to lose a second nor to invent one,
    /// whichever branch it takes. Every unit is parsed back and added up, which
    /// is the only way to catch a transposition - `1h 45m 27s` read back is not
    /// 27 hours and 45 minutes unless it is wrong.
    #[test]
    fn formatting_is_reversible() {
        for seconds in (0..=3 * DAY).step_by(997) {
            let rendered = format(seconds);
            let mut total = 0;
            for part in rendered.split(' ') {
                let (digits, unit) = part.split_at(part.len() - 1);
                let value: u64 = digits.parse().expect("a number");
                total += match unit {
                    "d" => value * DAY,
                    "h" => value * HOUR,
                    "m" => value * MINUTE,
                    "s" => value,
                    other => panic!("'{other}' is not a unit this plugin writes"),
                };
            }
            assert_eq!(total, seconds, "{seconds} rendered as {rendered:?}");
        }
    }

    // ---- accumulating across sessions ------------------------------------

    #[test]
    fn a_first_sighting_banks_nothing_and_only_marks() {
        // The whole of "a brand new player starts near zero": the first call
        // cannot bank a second, because there is no slice to count one from.
        let mut played = Played::default();
        played.bank(1_000);
        assert_eq!(played.seconds, 0);
        assert_eq!(played.mark_ms, Some(1_000));
    }

    #[test]
    fn an_online_session_banks_the_time_it_lasted() {
        assert_eq!(session(45, 1_000).seconds, 45);
        assert_eq!(session(HOUR / 1000, 1_000).seconds, HOUR / 1000);
    }

    /// A refresh faster than once a second is legal - `refresh-interval-ticks: 2`
    /// is the floor the docs invite - and every slice is then shorter than the
    /// unit it is trying to bank. The remainder has to carry or the counter
    /// stands still forever.
    #[test]
    fn a_faster_refresh_still_banks_whole_seconds() {
        // Eight seconds and a bit of wall time, in ten 900 ms slices.
        assert_eq!(session(9, 900).seconds, 8);
        // Twenty ticks at 50 ms each is the default refresh: one second a call,
        // which is the case every other test here is already covering.
        assert_eq!(session(20, 50).seconds, 1);
    }

    /// The requirement in one test: disconnect, stay away for an hour, come
    /// back. The hour is not play, and the total before it is not reset.
    #[test]
    fn time_spent_away_is_not_counted() {
        let mut played = Played::default();
        played.bank(0); // connects
        played.bank(30_000); // plays for thirty seconds
        played.stop(30_500); // leaves half a second later
        assert_eq!(played.seconds, 30);

        // An hour offline. Nothing banks it, because nothing calls `bank`.
        played.bank(3_630_500); // comes back
        assert_eq!(played.seconds, 30, "the hour away was counted as play");
        played.bank(33_630_500); // plays for another eight hours and a bit
        assert_eq!(played.seconds, 30_030);
    }

    /// A restart is the same shape as a long session from the store's point of
    /// view: the file holds whole seconds and nothing is marked, so the next
    /// `bank` starts a session on top of them instead of counting the time the
    /// server was down.
    #[test]
    fn a_reloaded_total_continues_rather_than_restarting() {
        let mut played = Played {
            seconds: 3_723,
            mark_ms: None,
        };
        played.bank(10_000_000);
        assert_eq!(played.seconds, 3_723, "the downtime was counted as play");
        played.bank(10_030_000);
        assert_eq!(played.seconds, 3_753);
    }

    #[test]
    fn many_short_sessions_add_up() {
        // Fifty logins of a minute each, with an hour offline between every one.
        // The only way the total can be 3000 is if each session banked its own
        // sixty seconds and none of the fifty hours in between leaked in.
        let mut played = Played::default();
        let mut at = 0;
        for _ in 0..50 {
            played.bank(at);
            at += MINUTE * 1000;
            played.stop(at);
            at += 3_600_000;
        }
        assert_eq!(played.seconds, 3_000);
        assert_eq!(format(played.seconds), "50m 0s");
    }

    #[test]
    fn a_total_never_moves_backwards() {
        // A clock that jumps, which is what a badly set server clock does.
        // Saturating arithmetic keeps the counter still rather than inverting,
        // and holding the mark still keeps the jump from being counted again on
        // the way back up - so 15s of real time is five seconds, not ten.
        let mut played = Played::default();
        played.bank(10_000);
        played.bank(10_000);
        played.bank(5_000);
        played.bank(5_000);
        assert_eq!(played.seconds, 0);
        played.bank(15_000);
        assert_eq!(played.seconds, 5);
    }

    // ---- the two halves together -----------------------------------------

    /// What the tab list actually shows, since banking the right number and
    /// formatting it are separate claims and only the pair is observable.
    #[test]
    fn what_a_player_sees_after_a_session() {
        let mut played = Played::default();
        played.bank(0);
        played.bank(45_000);
        assert_eq!(format(played.seconds), "45s");

        let mut played = Played::default();
        played.bank(0);
        played.bank(3_627_000);
        assert_eq!(format(played.seconds), "1h 0m 27s");
    }

    // ---- the save throttle ------------------------------------------------

    #[test]
    fn a_save_is_due_before_the_first_one_and_then_on_the_interval() {
        assert!(due(None, 0));
        assert!(due(None, 1));
        // Just short of the interval: a refresh every second must not write.
        assert!(!due(Some(0), SAVE_INTERVAL_MS - 1));
        assert!(due(Some(0), SAVE_INTERVAL_MS));
        assert!(due(Some(5_000), 5_000 + SAVE_INTERVAL_MS));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_make_a_save_permanently_due() {
        // `saturating_sub` rather than `wrapping_sub`, so a backwards jump
        // reads as "no time has passed" instead of "half the range has".
        assert!(!due(Some(SAVE_INTERVAL_MS * 10), 0));
    }
}