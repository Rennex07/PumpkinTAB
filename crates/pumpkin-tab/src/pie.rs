//! Asking PumpkinPIE for whatever the built-ins could not answer.
//!
//! ## Why a batcher and not a per-placeholder call
//!
//! Every placeholder PIE resolves is an inter-plugin message. A server with 40
//! players and a header, a footer, a tab name and a sidebar row each is over 120
//! of them per refresh, and at one refresh a second that is a lot of IPC to spend
//! on messages that would have been one.
//!
//! [`Batcher`] collects every line of a refresh, sends them in **one**
//! `set_placeholders_batch`, and hands the answers back by index.
//!
//! ## It is rebuilt every time, and that is the point
//!
//! A [`Batcher`] holds a refresh's worth of lines and is dropped at the end of
//! it: `tab::refresh_all` and `tab::apply_player` each make their own and neither
//! keeps one. So no resolved value, and no PIE answer, survives a refresh - the
//! only thing that crosses a refresh boundary is the rate limiter below, which
//! holds no values by design.
//!
//! ## The two ways a batch can come back short
//!
//! - **The whole batch is refused.** More than 8192 lines, or the provider is not
//!   loaded. Both arrive as an error, and both are handled by leaving every line
//!   exactly as it was: the tab list then shows `%whatever%`, which is honest.
//! - **One line is missing.** A line whose text is over 32768 bytes is *dropped*,
//!   not refused (`pumpkin-pie-plugin/src/plugin.rs:292` filters rather than
//!   failing), so `results` can be shorter than `lines`. Indexing past the end is
//!   therefore a real case and is handled by keeping the original text.
//!
//! ## Nothing here is required
//!
//! With no PIE installed, [`Batcher::resolve`] reports that it could not run,
//! every line keeps its built-in text, and the tab list still renders. The
//! `dependencies` list in the plugin metadata is empty for the same reason.

use std::{
    collections::BTreeSet,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use pumpkin_pie::{PieClient, PieError};
use tracing::{debug, warn};

/// How many resolved batches with something left in them may pass before a name
/// is worth naming again.
///
/// Long enough that a config with three stale placeholders does not put three
/// lines a minute into the log, short enough that fixing a typo and reloading is
/// visible without waiting. Counted in *batches*, not ticks: one per refresh plus
/// one per join, which is the only rate at which this is ever asked.
pub const REPORT_EVERY: u64 = 60;

/// How many batches have been resolved *with something left in them*.
///
/// The input to the window, and nothing else. Not a tick count and not a clock,
/// because the only question the limiter asks is "how many times has this been
/// asked", and the answer is the same however often that happens. A batch that
/// resolved everything does not count: there is nothing to report, and an idle
/// server should not be able to roll the window by doing less.
static BATCHES: AtomicU64 = AtomicU64::new(0);

/// The current reporting window, and the names already named inside it.
///
/// A fixed window rather than a per-name timestamp, for one reason: the set is
/// bounded by the number of distinct placeholders a config mentions, not by how
/// long the server has been up, so a set of typos cannot grow without limit.
///
/// [`None`] until the first report, so the first name is never swallowed.
static REPORTS: Mutex<Option<(u64, BTreeSet<String>)>> = Mutex::new(None);

/// One line waiting to be sent.
#[derive(Debug)]
struct Line {
    /// Whose placeholders these are resolved for. Always a player name, because
    /// the client API has no viewerless batch line: `set_placeholders_batch`
    /// takes `&[(&str, &str)]` and sets `viewer: Some(..)` unconditionally
    /// (`pumpkin-pie/src/client.rs:186`).
    viewer: String,
    /// The text, with the built-ins already substituted.
    text: String,
}

/// Collects a refresh's worth of lines and resolves them in one message.
#[derive(Debug, Default)]
pub struct Batcher {
    lines: Vec<Line>,
}

impl Batcher {
    /// An empty batcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many lines are queued.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether anything is queued.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Queues `text` to be resolved for `viewer`, and returns the index its
    /// answer will land at.
    pub fn add(&mut self, viewer: &str, text: String) -> usize {
        self.lines.push(Line {
            viewer: viewer.to_string(),
            text,
        });
        self.lines.len() - 1
    }

    /// Resolves everything queued, in one message.
    ///
    /// Returns one entry per queued line, in the same order, each the text with
    /// whatever PIE could answer replaced. An entry is the line's original text
    /// when PIE could not be reached or refused.
    ///
    /// Whatever is still holding a placeholder afterwards is named by
    /// [`report_unresolved`], rate limited, and once for the whole batch rather
    /// than once per line.
    pub fn resolve(&self) -> Vec<String> {
        if self.lines.is_empty() {
            return Vec::new();
        }

        let batch: Vec<(&str, &str)> = self
            .lines
            .iter()
            .map(|line| (line.viewer.as_str(), line.text.as_str()))
            .collect();

        let resolved: Vec<String> = match PieClient::new().set_placeholders_batch(&batch) {
            Ok(results) => {
                // Not an equality check: a line over 32 KiB is dropped by the
                // provider rather than refused, so the two lengths legitimately
                // differ and the answer for a dropped line is its original text.
                self.lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        results
                            .get(index)
                            .map_or_else(|| line.text.clone(), |result| result.text.clone())
                    })
                    .collect()
            }
            Err(error) => {
                report(&error);
                self.lines.iter().map(|line| line.text.clone()).collect()
            }
        };

        report_unresolved(&resolved);
        resolved
    }

    /// Whether a line was left with a placeholder in it after resolution.
    pub fn unresolved(index: usize, resolved: &[String]) -> usize {
        resolved
            .get(index)
            .map(|text| crate::render::count_placeholders(text))
            .unwrap_or(0)
    }
}

/// Names the placeholders nothing could answer, at most once per [`REPORT_EVERY`]
/// batches each.
///
/// This is the difference between "the prefix silently vanished" and something
/// an admin can grep for. The failure is silent by nature - the text on screen
/// still reads `%perm_prefix%` and looks like a styling choice - so it has to be
/// said out loud, and said *bounded*, because a stale config would otherwise
/// write the same line once per line per refresh forever.
fn report_unresolved(resolved: &[String]) {
    // A set, so a placeholder used in forty lines is one entry and one log line,
    // and sorted, so the report reads the same way twice.
    let names: BTreeSet<String> = resolved
        .iter()
        .flat_map(|line| crate::render::placeholder_names(line))
        .collect();
    if names.is_empty() {
        return;
    }

    let generation = BATCHES.fetch_add(1, Ordering::Relaxed) / REPORT_EVERY;
    // Not `.unwrap()`: a panic while the lock was held would otherwise turn every
    // later report into a panic of its own, which is a worse outcome than a
    // duplicated log line.
    let mut state = REPORTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fresh = fresh_names(&mut state, generation, &names);
    if fresh.is_empty() {
        return;
    }

    let listed: Vec<String> = fresh.iter().map(|name| format!("%{name}%")).collect();
    warn!(
        "nothing could answer {}, so {} left in the text as written. Check the spelling, and \
         check whether the placeholder is meant to come from PumpkinPIE - a %perm_% placeholder \
         needs PumpkinPerm loaded as well. Each is named at most once every {REPORT_EVERY} \
         refreshes.",
        listed.join(", "),
        if listed.len() == 1 { "it is" } else { "they are" },
    );
}

/// The names in `names` that have not been named in `generation`'s window, and
/// records them as named.
///
/// Split out of [`report_unresolved`] so the rate limiting can be tested without
/// touching the process-wide state, which the real call does. `state` is the
/// window [`REPORTS`] holds; a `generation` that is not the current one starts a
/// fresh window, which is what bounds the set.
fn fresh_names(
    state: &mut Option<(u64, BTreeSet<String>)>,
    generation: u64,
    names: &BTreeSet<String>,
) -> Vec<String> {
    if names.is_empty() {
        // Nothing left over, so nothing to say - and the window must not roll
        // either, or an idle server would wake up into a fresh window and re-n
        // every name it ever had.
        return Vec::new();
    }
    if !matches!(state, Some((window, _)) if *window == generation) {
        *state = Some((generation, BTreeSet::new()));
    }
    let Some((_, seen)) = state else {
        return Vec::new();
    };
    names
        .iter()
        .filter(|name| seen.insert((*name).clone()))
        .cloned()
        .collect()
}

/// Reports a provider problem, once per distinct cause.
///
/// The failure modes are all "the tab list will show `%whatever%`", which is
/// silent by nature, so it has to be said out loud. But it is said once: a
/// missing provider produces the same line a hundred times a second otherwise.
fn report(error: &PieError) {
    match error {
        PieError::Unreachable { .. } => {
            static ONCE: std::sync::atomic::AtomicBool =
                const { std::sync::atomic::AtomicBool::new(false) };
            if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                warn!(
                    "PumpkinPIE is not loaded, so placeholders this plugin does not answer itself \
                     are left in the text as written. The built-ins still work; groups, sorting, \
                     animations, the header and footer are all unaffected."
                );
            }
        }
        other => {
            debug!("PumpkinPIE refused a batch: {other}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_batch_resolves_to_nothing() {
        let batcher = Batcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.len(), 0);
        // Must not message the provider at all, or an idle server would still pay
        // for an IPC round trip every refresh.
        assert_eq!(batcher.resolve(), Vec::<String>::new());
    }

    #[test]
    fn lines_keep_their_order() {
        let mut batcher = Batcher::new();
        assert_eq!(batcher.add("Steve", "a".to_string()), 0);
        assert_eq!(batcher.add("Alex", "b".to_string()), 1);
        assert_eq!(batcher.add("Steve", "c".to_string()), 2);
        assert_eq!(batcher.len(), 3);
    }

    /// Deliberately absent: a test that calls the provider cannot run on the host
    /// target.
    ///
    /// `PieClient` reaches the host through wit-bindgen imports, and on a host
    /// build those are `unreachable!()` stubs, so the call panics rather than
    /// returning an error. The "provider is absent" path is therefore verified on
    /// the server instead, by `/tab reload` on a server with no PumpkinPIE
    /// installed, and here only the part that does not touch the provider.
    #[test]
    fn lines_are_held_in_the_order_they_were_added() {
        let mut batcher = Batcher::new();
        batcher.add("Steve", "%a%".to_string());
        batcher.add("Alex", "%b%".to_string());
        assert_eq!(batcher.len(), 2);
    }
    #[test]
    fn unresolved_counts_what_is_left() {
        let resolved = vec![
            "42".to_string(),
            "%still_here%".to_string(),
            "%a% and %b%".to_string(),
        ];
        assert_eq!(Batcher::unresolved(0, &resolved), 0);
        assert_eq!(Batcher::unresolved(1, &resolved), 1);
        assert_eq!(Batcher::unresolved(2, &resolved), 2);
    }

    #[test]
    fn an_index_past_the_results_is_zero_rather_than_a_panic() {
        // The provider drops an over-long line rather than refusing the batch, so
        // the results really can be shorter than the request.
        let resolved = vec!["only one".to_string()];
        assert_eq!(Batcher::unresolved(5, &resolved), 0);
    }

    // ---- the unresolved report -------------------------------------------

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    /// The whole point: a name is named once, however many lines and refreshes it
    /// turns up in.
    #[test]
    fn a_name_is_named_once_per_window() {
        let mut state = None;
        assert_eq!(fresh_names(&mut state, 0, &names(&["a", "b"])), ["a", "b"]);
        // The same window again, with the same names plus a new one.
        assert_eq!(fresh_names(&mut state, 0, &names(&["a", "b", "c"])), ["c"]);
        assert_eq!(fresh_names(&mut state, 0, &names(&["a", "b", "c"])), Vec::<String>::new());
    }

    /// The next window names them again, which is the other half: a name that has
    /// been broken for an hour still gets said once an hour, and a name that was
    /// fixed and broken again is not silent for ever.
    #[test]
    fn a_name_is_named_again_in_the_next_window() {
        let mut state = None;
        assert_eq!(fresh_names(&mut state, 0, &names(&["a"])), ["a"]);
        assert_eq!(fresh_names(&mut state, 1, &names(&["a"])), ["a"]);
    }

    /// The window is a fixed one, not a timestamp per name, so the set cannot grow
    /// with the number of times a name is seen.
    #[test]
    fn a_repeated_name_does_not_grow_the_record() {
        let mut state = None;
        for _ in 0..1_000 {
            fresh_names(&mut state, 0, &names(&["a", "b"]));
        }
        let Some((window, seen)) = state.as_ref() else {
            panic!("a report happened");
        };
        assert_eq!(*window, 0);
        assert_eq!(seen.len(), 2, "{seen:?}");
    }

    /// Nothing unresolved is nothing to say, and the window must not roll either -
    /// an idle server should not wake up into a fresh window and re-n everything.
    #[test]
    fn a_nothing_left_batch_names_nothing() {
        let mut state = None;
        assert_eq!(fresh_names(&mut state, 0, &names(&[])), Vec::<String>::new());
        assert!(state.is_none(), "an empty batch touched the window");
    }

    /// The names the report would print are the ones the resolver could not
    /// answer, which is exactly what `render` says is still a placeholder.
    #[test]
    fn the_names_are_the_placeholders_still_in_the_text() {
        let resolved = [
            "&a%perm_prefix%Steve".to_string(),
            "%ping%ms".to_string(),
            "clean".to_string(),
        ];
        let found: BTreeSet<String> = resolved
            .iter()
            .flat_map(|line| crate::render::placeholder_names(line))
            .collect();
        assert_eq!(found, names(&["perm_prefix", "ping"]));
    }
}
