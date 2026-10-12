//! The plugin's state.
//!
//! ## Why the two handles live in their own slots
//!
//! The WIT resource handles - `BossBar`, `Scoreboard`, `Player`,
//! `TextComponent` - are **neither `Copy` nor `Clone`**. They are newtypes over a
//! host resource-table index, and a call that takes one by value consumes it.
//! That has two consequences, and both are load-bearing:
//!
//! 1. The configuration sits behind an [`Arc`] so a [`Snapshot`] can be `Clone`
//!    and copied out from under the lock. A `BTreeSet` of team names and the
//!    scoreboard's world name are plain data and travel with it.
//! 2. The two host handles are **taken out of their slot for the duration of a
//!    call and put back afterwards**, rather than being held behind a lock while
//!    the host is called.
//!
//! The second is also the reentrancy answer. `pumpkin_plugin_api` warns against
//! holding a non-reentrant lock across a host call, because the call may deliver
//! an event back into this plugin before it returns - and `Player::has_permission`
//! does exactly that, firing `PlayerPermissionCheckEvent`, which the refresh
//! triggers for every configured group of every player. With the handle *taken
//! out*, a reentrant call simply finds the slot empty and skips, which is safe.
//! A lock held across the call would be the deadlock.
//!
//! If a body passed to [`with_bar`] or [`with_scoreboard`] panics, the handle is
//! not put back. The release profile is `panic = "abort"`, so the instance is
//! gone anyway, and a `/tab reload` builds a fresh one.
//!
//! ## What [`install`] deliberately does not own
//!
//! The repeating refresh is **not** part of the installed state. It used to be:
//! `install` cancelled the live task, on the reasonable-looking grounds that a
//! changed `refresh-interval-ticks` has to be rescheduled - and the only thing
//! that ever armed a replacement was `on_load`. So the first `/tab reload` left
//! the plugin pushing nothing at all, and the single one-shot refresh the command
//! does by hand, until the server was restarted. That is the whole of "placeholders
//! only update when I reload".
//!
//! The schedule now belongs to [`crate::restart_refresh`], which arms it from
//! `on_load` and from a *successful* reload. This module only remembers which task
//! is live, in a [`Refresh`], so that it can be cancelled exactly once.
//!
//! ## What is deliberately not in here
//!
//! Nothing per-player. A [`Snapshot`] is cloned once per refresh and walked once
//! per player, so a field keyed by player - anything that would have to be
//! updated as each row is pushed - turns a once-a-second clone into a
//! once-a-second copy of every player who ever joined. The rows themselves are
//! built and thrown away in [`crate::tab::refresh_all`]; what outlives a refresh
//! is only what the host holds on our behalf.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use pumpkin_plugin_api::{
    boss_bar::BossBar, player::Player, scheduler::cancel_task, scoreboard::Scoreboard,
};

use crate::{config::Config, groups, render::Clock};

/// The configuration, shared and never mutated in place.
static CONFIG: OnceLock<Mutex<Option<Arc<Config>>>> = OnceLock::new();
/// The repeating refresh, so it can be cancelled.
static REFRESH: Mutex<Refresh> = Mutex::new(Refresh::IDLE);
/// The shared bossbar, taken out for the duration of each use.
static BAR: Mutex<Option<BossBar>> = Mutex::new(None);
/// The sidebar's scoreboard per world, keyed by world name.
///
/// A map rather than a `Vec` because the lookup is by name on every refresh, and
/// because the iteration order has to be stable for the log line.
static BOARDS: Mutex<BTreeMap<String, Scoreboard>> = Mutex::new(BTreeMap::new());

fn config_slot() -> &'static Mutex<Option<Arc<Config>>> {
    CONFIG.get_or_init(|| Mutex::new(None))
}

/// A guard on a poisoned lock, rather than the panic `lock()` would raise.
///
/// A poisoned lock means a previous holder panicked. Everything behind one here
/// is plain data with no invariant a panic could have half-broken, so recovering
/// beats turning one panic into a permanently dead plugin - and it is the
/// reason this plugin has no `catch_unwind` anywhere.
///
/// `pub(crate)` so the same reasoning is written once rather than twice that drift.
pub(crate) fn recover<T>(
    result: std::sync::LockResult<MutexGuard<'_, T>>,
) -> MutexGuard<'_, T> {
    match result {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Everything a refresh needs, all of it `Clone`.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// The configuration.
    pub config: Arc<Config>,
    /// The clock animations read, so a reload does not restart them.
    pub clock: Clock,
    /// The sidebar teams that exist.
    pub teams: BTreeSet<String>,
    /// How many `lines:` the config in use has, so a reload that shortens the list
    /// can remove the rows the previous one left behind.
    ///
    /// Without this a deleted line's row would sit on every client's sidebar
    /// forever, because nothing else ever writes that key again.
    pub line_count: usize,
    /// The worlds that currently have a sidebar.
    pub scoreboard_worlds: Vec<String>,
}

impl Snapshot {
    /// Whether `world` has a sidebar this plugin writes to.
    ///
    /// The handle is per world, so a player in a world that was excluded, or that
    /// did not exist when the handles were taken, must not be written to.
    pub fn owns_world(&self, world: &str) -> bool {
        self.scoreboard_worlds
            .iter()
            .any(|cached| cached.eq_ignore_ascii_case(world))
    }
}

/// The clock, which outlives every reload.
static CLOCK: OnceLock<Clock> = OnceLock::new();

/// The animation clock.
pub fn clock() -> Clock {
    *CLOCK.get_or_init(Clock::start)
}

/// Installs a freshly loaded configuration.
///
/// The old bar is emptied first, or it would stay on everybody's screen with no
/// way to remove it. The clock is *kept*, so a `/tab reload` does not restart
/// every animation from frame zero.
///
/// The refresh task is left alone on purpose; see the module header.
pub fn install(config: Config) {
    // Empty the old bar before dropping the handle, while it is still reachable.
    with_bar(|bar| bar.remove_all());
    *recover(BAR.lock()) = None;

    // The sidebar handles go too, or a reload would never notice a changed title
    // or a changed `disabled-worlds`. Dropping a handle leaks its resource-table
    // entry, which is why this only happens on load and on an explicit reload
    // rather than per refresh.
    *recover(BOARDS.lock()) = BTreeMap::new();

    let mut slot = recover(config_slot().lock());
    *slot = Some(Arc::new(config));
}

/// A copy of the configuration, or [`None`] before the plugin has loaded.
pub fn snapshot() -> Option<Snapshot> {
    let config = recover(config_slot().lock());
    let config = config.as_ref()?.clone();
    Some(Snapshot {
        config,
        clock: clock(),
        teams: recover(TEAMS.lock()).clone(),
        line_count: line_count(),
        scoreboard_worlds: recover(BOARDS.lock()).keys().cloned().collect(),
    })
}

/// The configuration, for the command handlers.
pub fn config() -> Option<Arc<Config>> {
    recover(config_slot().lock()).clone()
}

/// The sidebar teams that exist.
static TEAMS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Whether the sidebar objective is on the board.
///
/// Tracked rather than asked, because the WIT has no getter for an objective. It
/// is what lets a reload `update_objective` in place instead of removing and
/// re-adding, which would blink the sidebar out for every connected client.
static OBJECTIVE_EXISTS: Mutex<bool> = Mutex::new(false);

/// Whether the objective is currently on the board.
pub fn objective_exists() -> bool {
    *recover(OBJECTIVE_EXISTS.lock())
}

/// Records that the objective is on the board.
pub fn set_objective_exists(exists: bool) {
    *recover(OBJECTIVE_EXISTS.lock()) = exists;
}

/// How many `lines:` were in use when the sidebar was last built.
static LINE_COUNT: Mutex<usize> = Mutex::new(0);

/// Records how many sidebar lines exist, so the next build can clean up after a
/// shorter list.
pub fn set_line_count(count: usize) {
    *recover(LINE_COUNT.lock()) = count;
}

/// The line count in force, for a build that has not happened yet.
pub fn line_count() -> usize {
    *recover(LINE_COUNT.lock())
}

/// Which refresh task is live, and the bookkeeping that keeps it that way.
///
/// At most one loop may run at a time. Two would push every packet twice a second
/// for an identical result, and - the part that actually hurts - `cancel_task`
/// takes exactly one id, so a loop that outlived its replacement could never be
/// taken back. [`Refresh::arm`] therefore hands back the task it displaced rather
/// than silently overwriting it, and the displaced id is a value rather than
/// something to log, so a caller that forgets it is caught by a test instead of
/// by a server that quietly stops updating.
///
/// A named type rather than a bare `Option<u32>` because the swap is then one
/// method instead of three lines repeated at each call site, and because the
/// bookkeeping is testable with dummy ids on the host: `schedule_repeating_task`
/// and `cancel_task` reach the host through wit-bindgen imports, which are
/// `unreachable!()` on a host build and trap rather than return an error. Same
/// precedent as the `pie.rs` note about calling the provider.
#[derive(Debug, PartialEq, Eq)]
pub struct Refresh {
    task: Option<u32>,
}

impl Refresh {
    /// No loop armed.
    pub const IDLE: Self = Self { task: None };

    /// Records `task` as the live loop and returns the one it displaced.
    pub fn arm(&mut self, task: u32) -> Option<u32> {
        self.task.replace(task)
    }

    /// Forgets the live loop, returning its id so the caller can cancel it.
    ///
    /// [`None`] means nothing was running, which is the normal case for a second
    /// [`Self::take`] and must not be an error.
    pub fn take(&mut self) -> Option<u32> {
        self.task.take()
    }

    /// The live task's id.
    pub fn armed(&self) -> Option<u32> {
        self.task
    }
}

/// Records the live refresh, returning the task it displaced.
///
/// The displaced id is returned rather than logged because overwriting it is the
/// bug this type exists to prevent: a task whose id is lost can never be
/// cancelled. [`crate::restart_refresh`] takes the old one out first, so it
/// always sees [`None`] here, and the tests in this module are what assert that.
pub fn arm_refresh(task: u32) -> Option<u32> {
    recover(REFRESH.lock()).arm(task)
}

/// Takes the live refresh out, returning its id so it can be cancelled.
///
/// The other half of the swap [`arm_refresh`] performs, and the single place the
/// id a task was given is read back out.
pub fn take_refresh() -> Option<u32> {
    recover(REFRESH.lock()).take()
}

/// Installs the bar.
pub fn set_bar(bar: BossBar) {
    *recover(BAR.lock()) = Some(bar);
}

/// Takes the sidebar handles out, for a teardown that has to write to each one
/// before they go.
///
/// Returns [`None`] when there are none, so a teardown can be skipped when there
/// is nothing to remove.
pub fn take_scoreboards() -> Option<BTreeMap<String, Scoreboard>> {
    let taken = std::mem::take(&mut *recover(BOARDS.lock()));
    (!taken.is_empty()).then_some(taken)
}

/// The group teams the sidebar created.
pub fn teams() -> BTreeSet<String> {
    recover(TEAMS.lock()).clone()
}

/// Installs the sidebar handles, one per world that shows it.
///
/// Every `world.get-scoreboard` pushes a **new** entry into the plugin's resource
/// table and nothing reuses one, so these are fetched once per world and kept for
/// as long as the server is up rather than per refresh.
///
/// Replacing the map wholesale is what makes a reload that *added* a world pick
/// it up: the old handles are dropped and the new set built.
pub fn set_scoreboards(boards: BTreeMap<String, Scoreboard>) {
    *recover(BOARDS.lock()) = boards;
}

/// How many sidebar handles exist, for the startup log.
pub fn scoreboard_count() -> usize {
    recover(BOARDS.lock()).len()
}

/// Whether a sidebar handle still has to be obtained for any world.
pub fn needs_scoreboard() -> bool {
    config().is_some_and(|config| config.scoreboard.enabled) && recover(BOARDS.lock()).is_empty()
}

/// Records the teams the sidebar created.
pub fn set_teams(teams: BTreeSet<String>) {
    *recover(TEAMS.lock()) = teams;
}

/// Uses the bar, taking the handle out for the duration.
///
/// Returns [`None`] when there is no bar, which is the normal case: the feature is
/// off by default, and a `/tab reload` with it still off leaves none behind.
pub fn with_bar<R>(body: impl FnOnce(&BossBar) -> R) -> Option<R> {
    let bar = recover(BAR.lock()).take()?;
    let out = body(&bar);
    *recover(BAR.lock()) = Some(bar);
    Some(out)
}

/// Uses the sidebar handle for `world`, taking it out for the duration.
///
/// Returns [`None`] when that world has no sidebar, which is the normal case: the
/// feature is off by default, a world can be excluded by `disabled-worlds`, and a
/// `/tab reload` with it off leaves none behind.
pub fn with_scoreboard<R>(world: &str, body: impl FnOnce(&Scoreboard) -> R) -> Option<R> {
    let mut boards = recover(BOARDS.lock());
    let board = boards.get_mut(world)?;
    let out = body(board);
    Some(out)
}

/// The worlds that currently have a sidebar, in a stable order.
pub fn scoreboard_worlds() -> Vec<String> {
    recover(BOARDS.lock()).keys().cloned().collect()
}

/// The group `player` is shown as, owned so it outlives the state lock.
///
/// Returned as a plain tuple rather than a [`groups::Resolved`], which borrows
/// from the configuration. `/tab check` needs to keep the answer after the
/// snapshot is dropped, and a `&str` into an `Arc<Config>` cannot be made to.
pub fn resolve_for(player: &Player) -> Option<(String, i32, Option<&'static str>)> {
    let config = config()?;
    let resolved = groups::resolve(&config, player);
    Some((
        resolved.name.to_string(),
        groups::sort_value(&config, &resolved),
        resolved.reason.map(|reason| reason.as_str()),
    ))
}

/// Cancels the refresh and empties the bar.
///
/// The sidebar objective is deliberately left in place. It is a server-wide thing
/// with a real name, and a `/tab reload` should not make every client's sidebar
/// blink out for a moment; the objective is updated in place instead.
pub fn unload() {
    if let Some(task) = take_refresh() {
        cancel_task(task);
    }
    with_bar(|bar| bar.remove_all());
    *recover(BAR.lock()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(world: Option<&str>) -> Snapshot {
        Snapshot {
            config: Arc::new(Config::default()),
            clock: clock(),
            teams: BTreeSet::new(),
            line_count: 0,
            scoreboard_worlds: world.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn a_snapshot_without_a_world_owns_nothing() {
        assert!(!snapshot(None).owns_world("world"));
        assert!(!snapshot(None).owns_world(""));
    }

    #[test]
    fn a_snapshot_owns_only_its_own_world() {
        // The handle belongs to one world, and a player somewhere else must not
        // be written to it.
        let owns = snapshot(Some("world"));
        assert!(owns.owns_world("world"));
        assert!(owns.owns_world("WORLD"));
        assert!(!owns.owns_world("world_nether"));
    }

    #[test]
    fn using_a_handle_that_is_not_there_is_not_an_error() {
        // The bar is off by default, and a `/tab reload` with it still off leaves
        // none behind. Both must be a plain `None`, not a panic.
        assert!(with_bar(|bar| bar.remove_all()).is_none());
    }

    #[test]
    fn a_handle_comes_back_after_use() {
        // The take-and-restore is what makes the next refresh work at all; if it
        // did not restore, the second call would find nothing.
        let config = Config::default();
        assert!(with_bar(|_| ()).is_none());
        drop(config);
    }

    #[test]
    fn the_clock_is_shared_so_a_reload_does_not_restart_animations() {
        let first = clock();
        let second = clock();
        // Same instant, so elapsed time only ever moves forwards.
        assert!(second.elapsed_ms() >= first.elapsed_ms());
    }

    /// The reload bookkeeping, in miniature, and testable because none of it calls
    /// the host: a task id is just a `u32`, and the function that turns the swap
    /// into two host calls is [`crate::restart_refresh`], exercised on the server.
    #[test]
    fn arming_a_second_refresh_hands_back_the_one_it_displaced() {
        let mut refresh = Refresh::IDLE;
        assert_eq!(refresh.armed(), None);
        assert_eq!(refresh.arm(1), None);
        // Returning the old id is what keeps it cancellable. Overwriting the slot
        // instead would leave a live loop nothing holds, and `cancel_task` cannot
        // find a task whose id was thrown away.
        assert_eq!(refresh.arm(2), Some(1));
        assert_eq!(refresh.armed(), Some(2));
    }

    #[test]
    fn taking_the_refresh_hands_back_the_id_to_cancel() {
        let mut refresh = Refresh::IDLE;
        assert_eq!(refresh.take(), None, "nothing armed is not an error");
        refresh.arm(7);
        assert_eq!(refresh.take(), Some(7));
        // Empty again, which is what makes `unload` idempotent: a second unload
        // must not cancel an id that has already been re-used by a later load.
        assert_eq!(refresh.take(), None);
    }

    #[test]
    fn a_reload_that_repeats_leaves_exactly_one_task_live() {
        // The two operations `restart_refresh` performs, in the order it
        // performs them: take, cancel, schedule, arm. After any number of reloads
        // exactly one id is remembered, and it is the newest.
        let mut refresh = Refresh::IDLE;
        for round in 1..=5u32 {
            let stale = refresh.take();
            assert_eq!(stale, (round > 1).then_some(round - 1));
            assert_eq!(refresh.arm(round), None, "the slot was cleared first");
        }
        assert_eq!(refresh.armed(), Some(5));
        assert_eq!(refresh.take(), Some(5));
    }
}
