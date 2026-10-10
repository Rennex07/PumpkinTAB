//! The refresh: everything this plugin pushes, once per interval.
//!
//! ## The shape of a refresh
//!
//! 1. **Bank the elapsed time.** Once per player, [`playtime::bank`], before
//!    anything is resolved - so `%playtime%` shows the slice that has just ended
//!    rather than the one before it.
//! 2. **Resolve locally.** For every player, build the header, the footer, their
//!    tab-list name and their sidebar row's right-hand text, substituting only the
//!    built-ins and animations. No inter-plugin messages yet.
//! 3. **Resolve in one batch.** Every line from step 2 that still holds a
//!    placeholder goes to PumpkinPIE in a single message.
//! 4. **Push.** Sidebar row, header and footer, tab-list name, sort order and
//!    latency, and last the bar, which consumes the player handle.
//!
//! Splitting it that way is what keeps the cost at one message per refresh rather
//! than one per placeholder, which is why [`pie::Batcher`] takes a whole
//! refresh's worth of lines at once.
//!
//! Step 1 is here and not in `lib::tick` for one reason, spelled out on
//! [`crate::tick`]: this loop is already walking the connected players, and
//! `get_all_players` mints a fresh host resource entry for each of them on every
//! call, so asking a second time would cost once a second per player for nothing.
//!
//! ## Nothing is carried over
//!
//! The batcher is built here, per call, and dropped at the end of it -
//! [`refresh_all`] and [`apply_player`] each make their own. No rendered text, no
//! resolved placeholder and no PIE answer survives a refresh, which is the
//! property that lets a `reload` change the config and have the next refresh
//! show it. The single declared lag is not ours: PumpkinPerm registers its `perm`
//! namespace with PumpkinPIE as `Cache::Ttl { ms: 5000 }`, so a `%perm_*%` value
//! may be up to five seconds behind. That is a choice PumpkinPerm made about its
//! own namespace, and it is why `%perm_group%` can disagree with `%group%` for
//! a few seconds after an assignment.
//!
//! ## The player handle is consumed by the bar
//!
//! The WIT resource types are neither `Copy` nor `Clone`, and
//! `boss-bar.add-player` takes a `player` **by value**. So the bar is the last
//! thing a player is used for: everything else takes `&Player`, and by the time
//! `add_player` is called there is nothing left to do with that handle anyway.
//! Ordering it the other way round would make the tab list unreachable for
//! everybody in the bar.
//!
//! ## What is world-wide and what is not
//!
//! Read out of the host, not assumed:
//!
//! - `set-tab-list-name`, `set-tab-list-order` and `set-tab-list-latency` all call
//!   `world.broadcast_packet_all` (`entity/player.rs:1006`, `:1022`, `:1034`).
//!   They look per-player and are not: every Java player in the world is sent
//!   every setter, so there is no per-viewer tab-list name on this WIT.
//! - `set-tab-list-header-footer` sends to one socket (`entity/player.rs:925`)
//!   and is Java-only. Bedrock drops it, which is vanilla behaviour.
//! - `set-tab-list-listed` is the only way to actually hide an entry
//!   (`entity/player.rs:1046`). `hide-player` looks like the answer and is not:
//!   `hide_player` only inserts into `hidden_players` and emits no packet at all
//!   (`entity/player.rs:3848`). The set *is* read, by `can_see`
//!   (`entity/player.rs:3862`), but that answers a plugin's own question and
//!   never reaches the client.

use pumpkin_plugin_api::{Server, player::Player, scoreboard::Scoreboard, text::TextComponent};
use tracing::debug;

use crate::{
    bossbar, groups, pie, playtime,
    render::{self, View},
    scoreboard as board,
    state::{self, Snapshot},
    text,
};

/// One player's work, with the batcher slots its texts occupy.
struct Row {
    name: String,
    world: String,
    group: String,
    /// Batcher indices for the header, the footer and the tab-list name.
    slots: [usize; 3],
    /// Batcher index for the sidebar's right-hand text, if the sidebar is on.
    sidebar: Option<usize>,
}

/// Resolves locally, batches through PIE, then pushes, for everybody.
pub fn refresh_all(server: &Server, snapshot: &Snapshot) {
    let players = server.get_all_players();
    let mut batcher = pie::Batcher::new();
    let lines = line_slots(snapshot, &mut batcher, server);

    if players.is_empty() {
        // The fixed lines are still written. They belong to the world scoreboard,
        // not to a player, so a server that has gone empty must not leave whatever
        // it last showed frozen on the board for whoever joins next.
        let resolved = batcher.resolve();
        push_lines(snapshot, &resolved, &lines, 0);
        return;
    }

    let mut rows: Vec<Row> = Vec::with_capacity(players.len());

    for player in &players {
        let name = player.get_name();
        let world = player.get_world().get_name();
        let group = groups::resolve(&snapshot.config, player).name.to_string();
        // Before the `View`, so `%playtime%` resolves to the slice that has just
        // elapsed rather than the one before it. This is the *whole* of the live
        // playtime: it is the only thing that banks time, it runs once per player
        // per refresh, and it reads no handle but this one.
        playtime::bank(player);
        let view = view_for(snapshot, player, server);

        let slots = [
            batcher.add(&name, local_block(&view, &snapshot.config.header_footer.header)),
            batcher.add(&name, local_block(&view, &snapshot.config.header_footer.footer)),
            batcher.add(&name, local_line(&view, &snapshot.config.tab_list.format)),
        ];
        let sidebar = sidebar_slot(snapshot, &mut batcher, &view, &name, &world);

        rows.push(Row {
            name,
            world,
            group,
            slots,
            sidebar,
        });
    }

    // Once for the whole refresh rather than once per player: the write is
    // throttled, so this is at most one `std::fs` call every thirty seconds.
    playtime::save_if_due();

    // The bar's title is one string for everybody, so it is pushed once, before
    // the per-player work consumes the handles it needs.
    push_bar_title(snapshot, server);

    let resolved = batcher.resolve();
    let total = rows.len();

    push_lines(snapshot, &resolved, &lines, total);

    // Zipped rather than indexed, so each `Player` is borrowed for the tab list
    // and the sidebar and then *moved* into the bar, which is the only thing that
    // takes it by value. `Player` is neither `Copy` nor `Clone`, so there is no
    // second handle to fall back on.
    for (index, (player, row)) in players.into_iter().zip(rows).enumerate() {
        let right = row.sidebar.map(|slot| at(&resolved, slot));
        push_sideboard(snapshot, &player, &row, right.as_deref(), index, total);
        push_player(&player, &row, &resolved, &snapshot.config);
        // Last, because this consumes the handle.
        push_bar_membership(snapshot, &row.world, player);
    }
}

/// Batches the fixed `lines:`, once per refresh.
///
/// Each line contributes two slots, its text and its right-hand value, because
/// they are separate parts of TAB's syntax and either may be absent.
fn line_slots(
    snapshot: &Snapshot,
    batcher: &mut pie::Batcher,
    server: &Server,
) -> Vec<[usize; 2]> {
    if !snapshot.config.scoreboard.enabled {
        return Vec::new();
    }
    let view = shared_view(snapshot, server);
    snapshot
        .config
        .scoreboard
        .lines
        .iter()
        .map(|raw| {
            let line = board::parse_line(raw);
            [
                batcher.add("", local_line(&view, line.text)),
                batcher.add("", local_line(&view, line.number_format)),
            ]
        })
        .collect()
}

/// Writes the fixed lines, above the player rows.
fn push_lines(snapshot: &Snapshot, resolved: &[String], lines: &[[usize; 2]], players: usize) {
    if lines.is_empty() {
        return;
    }
    let configured: Vec<board::Line<'_>> = snapshot
        .config
        .scoreboard
        .lines
        .iter()
        .map(|raw| board::parse_line(raw))
        .collect();

    for world in &snapshot.scoreboard_worlds {
        state::with_scoreboard(world, |board: &Scoreboard| {
            for (index, slots) in lines.iter().enumerate() {
                let line = &configured[index];
                let text = at(resolved, slots[0]);
                let right = at(resolved, slots[1]);
                if !board::line_is_visible(line, &text, &right) {
                    board::hide_line(board, index);
                    continue;
                }
                board::set_line(
                    board,
                    index,
                    board::line_score(line, index, lines.len() + players),
                    &text,
                    &right,
                );
            }
        });
    }
}

/// Resolves locally, batches through PIE, then pushes, for one player.
///
/// Used by the join event, so somebody who has just connected is not looking at a
/// default tab list until the next tick.
pub fn apply_player(server: &Server, snapshot: &Snapshot, player: &Player) {
    let name = player.get_name();
    let world = player.get_world().get_name();
    let group = groups::resolve(&snapshot.config, player).name.to_string();
    let view = view_for(snapshot, player, server);

    let mut batcher = pie::Batcher::new();
    let slots = [
        batcher.add(&name, local_block(&view, &snapshot.config.header_footer.header)),
        batcher.add(&name, local_block(&view, &snapshot.config.header_footer.footer)),
        batcher.add(&name, local_line(&view, &snapshot.config.tab_list.format)),
    ];
    let sidebar = sidebar_slot(snapshot, &mut batcher, &view, &name, &world);

    push_bar_title(snapshot, server);

    let resolved = batcher.resolve();
    let row = Row {
        name,
        world,
        group,
        slots,
        sidebar,
    };

    let right = sidebar.map(|slot| at(&resolved, slot));
    push_sideboard(
        snapshot,
        player,
        &row,
        right.as_deref(),
        0,
        // One player on the board right now. The line count still shifts the
        // value, because the lines are already there and the two ranges have to
        // stay distinct.
        1,
    );
    push_player(player, &row, &resolved, &snapshot.config);
    // The bar is deliberately not touched here. `add-player` takes the player by
    // value, and this path only has a borrow, so bar membership waits for the
    // next refresh - at most a second away, and a bar that appears a moment late
    // is not worth consuming a handle for.
}

/// The sidebar's right-hand text, if the sidebar is on for this world and the
/// config asks for one.
///
/// [`None`] is the whole of "`entry: ''` hides the per-player row", and it is what
/// [`push_sideboard`] acts on: with nothing to put on the right there is no row,
/// rather than a row with a blank value. See the comment there, because writing
/// one with `NumberFormat::Blank` looks identical here and is not the same thing
/// on a client.
fn sidebar_slot(
    snapshot: &Snapshot,
    batcher: &mut pie::Batcher,
    view: &View<'_>,
    name: &str,
    world: &str,
) -> Option<usize> {
    if !snapshot.owns_world(world) {
        return None;
    }
    let entry = snapshot.config.scoreboard.entry.as_deref()?;
    if entry.trim().is_empty() {
        return None;
    }
    Some(batcher.add(name, local_line(view, entry)))
}

/// The [`View`] one player's templates are resolved against.
fn view_for<'a>(snapshot: &'a Snapshot, player: &'a Player, server: &'a Server) -> View<'a> {
    let group_name = groups::resolve(&snapshot.config, player).name;
    View {
        config: &snapshot.config,
        clock: snapshot.clock,
        server,
        player: Some(player),
        group: Some(group_name),
    }
}

/// A [`View`] with no player, for server-wide text such as the bar's title.
fn shared_view<'a>(snapshot: &'a Snapshot, server: &'a Server) -> View<'a> {
    View {
        config: &snapshot.config,
        clock: snapshot.clock,
        server,
        player: None,
        group: None,
    }
}

/// Local resolution of a multi-line block.
fn local_block(view: &View<'_>, lines: &[String]) -> String {
    let rendered: Vec<String> = lines.iter().map(|line| local_line(view, line)).collect();
    text::join_lines(&rendered.iter().map(String::as_str).collect::<Vec<_>>())
}

/// Local resolution of a single line, built-ins and animations only.
fn local_line(view: &View<'_>, line: &str) -> String {
    // Only the built-ins and animations. Anything still holding a placeholder
    // after this is batched for PIE a moment later, for the whole refresh at once.
    text::substitute(line, |name| view.resolve(name))
}

/// Everything pushed for one player, other than the sidebar and the bar.
fn push_player(player: &Player, row: &Row, resolved: &[String], config: &crate::config::Config) {
    if config.header_footer.applies_in(&row.world) {
        player.set_tab_list_header_footer(
            component(&at(resolved, row.slots[0])),
            component(&at(resolved, row.slots[1])),
        );
    }

    if !config.tab_list.applies_in(&row.world) {
        // The one silent path in a refresh: a disabled world skips the tab-list
        // name, the sort order *and* the latency push together. That is what the
        // config asked for, but `/tab check` reporting it once is a long way from
        // three setters quietly not happening every second. `debug` and not `warn`
        // because a disabled world is a deliberate admin choice, not a fault.
        debug!(
            "{} is in world '{}', which is under tab-list.disabled-worlds, so no tab-list name, \
             sort order or latency was pushed for them",
            row.name,
            row.world
        );
        return;
    }

    let group = groups::resolve(config, player);
    player.set_tab_list_order(groups::sort_value(config, &group));

    // `get_ping` is 0 for anybody whose keep-alive has not been seen, and for
    // every Bedrock connection, because the Java keep-alive handler holds the only
    // two writers of `player.ping` (`net/java/play/keep_alive.rs:17` and `:24`).
    // It is pushed anyway: the host keeps a separate `tab_list_latency` field that
    // starts at 0, so not pushing it leaves every client showing no bars at all.
    player.set_tab_list_latency(player.get_ping() as i32);
    let entry = at(resolved, row.slots[2]);
    let entry = if config.tab_list.prefix_optional {
        render::blank_unresolved_prefix(&entry)
    } else {
        entry
    };
    player.set_tab_list_name(Some(component(&entry)));
}

/// One sidebar row: the player's team, their score, and the text beside it.
fn push_sideboard(
    snapshot: &Snapshot,
    player: &Player,
    row: &Row,
    right: Option<&str>,
    index: usize,
    total: usize,
) {
    if !snapshot.owns_world(&row.world) {
        return;
    }
    let score = board::player_value(index, snapshot.config.scoreboard.lines.len(), total);
    state::with_scoreboard(&row.world, |board: &Scoreboard| {
        board::assign_team(board, &snapshot.teams, player, &row.group);
        match right.filter(|text| !text.trim().is_empty()) {
            Some(text) => board::set_row(board, &row.name, score, Some(text)),
            // **Removed, not written with a blank value.** `update_score` creates
            // a row whatever the number format is, and a sidebar row's left-hand
            // side is the entry name - which here is the player's own name. So
            // writing this row with `NumberFormat::Blank` did not hide it, it drew
            // the player's name under `play.yourserver.com` with nothing beside
            // it. `entry: ''` is documented as "hide the per-player row", and the
            // only way to honour that is to have no row at all.
            //
            // Removing rather than skipping is also what makes the *transition*
            // work: a `/tab reload` that empties `entry` has no other moment at
            // which the row the old config left behind could be taken off, and it
            // would otherwise sit there for the rest of the server's life.
            None => board::clear_row(board, &row.name),
        }
    });
}

/// Removes a departing player's sidebar row, so a stale entry is not left behind.
///
/// Only in the world they were in. A player who walked through a portal on the way
/// out has already had their row written to the new world's sidebar, and removing
/// it here would blank a row that belongs to somebody else.
pub fn clear_player(snapshot: &Snapshot, player: &Player) {
    let name = player.get_name();
    let world = player.get_world().get_name();
    if !snapshot.owns_world(&world) {
        return;
    }
    state::with_scoreboard(&world, |board| board::clear_row(board, &name));
}

/// The bar's title and colour, pushed once per refresh.
fn push_bar_title(snapshot: &Snapshot, server: &Server) {
    if !snapshot.config.bossbar.enabled {
        return;
    }
    let view = shared_view(snapshot, server);
    state::with_bar(|bar| bossbar::update(bar, &snapshot.config.bossbar, &view));
}

/// Bar membership for one player.
///
/// `add-player` and `remove-player` take the player by value, so this consumes
/// the handle and must be the last thing done with it.
fn push_bar_membership(snapshot: &Snapshot, world: &str, player: Player) {
    if !snapshot.config.bossbar.enabled {
        return;
    }
    let wanted = snapshot.config.bossbar.applies_in(world);
    let name = player.get_name();
    state::with_bar(|bar| {
        // Membership only. A player already in the bar is not re-added every
        // refresh, because the host keeps a list and re-adding would grow it.
        let present = bar.get_players().iter().any(|seen| seen.get_name() == name);
        match (wanted, present) {
            (true, false) => bar.add_player(player),
            (false, true) => bar.remove_player(player),
            _ => {}
        }
    });
}

/// The text at `index`, or an empty string if the batch came back short.
fn at(resolved: &[String], index: usize) -> String {
    resolved.get(index).cloned().unwrap_or_default()
}

/// A component from a rendered, MiniMessage-flavoured string.
///
/// The tab-list name and the header/footer both come through here, and this is
/// [`text::component`] rather than a legacy string: the host's
/// `text-component` resource has `gradient`, `rainbow`, `click-open-url` and
/// `hover-show-text` (`crates/pumpkin-plugin-wit/v0.1/text.wit:34-65`) and none
/// of them can be written into a `&`-coded string, so building the tree is the
/// only way they reach a client. It is also what makes a closing tag close:
/// `</bold>` in a legacy string cannot end bold without ending everything
/// (`text`, "Legacy: the rules, and what they cost"), and on the tab list that
/// is the difference between a name that picks up the prefix's bold and one that
/// does not.
///
/// A fresh one every time: `TextComponent` is neither `Copy` nor `Clone`, and
/// every call below takes it by value.
pub fn component(rendered: &str) -> TextComponent {
    text::component(rendered)
}
