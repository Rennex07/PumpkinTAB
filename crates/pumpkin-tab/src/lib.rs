//! `PumpkinTAB`, a tab list plugin for Pumpkin.
//!
//! Header, footer, per-player names, group prefixes, sorting, animations, a
//! shared bossbar and a sidebar - driven from one YAML file, resolved through
//! [PumpkinPIE](https://github.com/Rennex07/PumpkinPIE) when it is there and
//! without it when it is not.
//!
//! ## Groups, and where they come from
//!
//! `groups:` is a plain list of names, **highest first**, and the position in the
//! list *is* the sort order - there is no `order` number to keep in step with
//! anything. A player is shown as the **first group in the list that they
//! qualify for**, and as `default` if none match. See [`config::Config::groups`]
//! and [`groups`].
//!
//! A group entry carries a name and nothing else: no `prefix`, no `suffix`, no
//! `order`, no `permission`. A prefix would then be defined twice - here and in
//! PumpkinPerm - and the two copies would eventually disagree. It belongs to
//! PumpkinPerm and reaches the tab list as `%perm_prefix%`.
//!
//! There is no per-group `permission` field either. The node is **derived from
//! the name**, as `PumpkinPerm:group.<name>`
//! ([`config::Config::node_for`]), so the name is the group's whole address in
//! the permission manager. `op-level-groups:` is the other way in: an op level
//! mapped to a group qualifies for it without anybody having been granted its
//! node.
//!
//! Membership is decided with `has-permission-set`, which answers with three
//! states rather than two, and all three are used:
//!
//! | Answer | Meaning | This plugin |
//! |:--|:--|:--|
//! | `Some(true)` | explicitly granted | in the group |
//! | `Some(false)` | explicitly denied | not in it, keep looking |
//! | `None` | never set, so it would inherit | not a member; `op-level-groups` may still qualify them |
//!
//! Permissions belong to PumpkinPerm, which owns the weights that keep a junior
//! moderator from holding an owner's nodes. The two
//! plugins never call each other: PumpkinTAB's only knowledge of PumpkinPerm is
//! [`config::PERM_PLUGIN`], the constant the node name is built from, and
//! pointing that at a different permission plugin is a one-line change.
//!
//! ## Who owns the refresh loop
//!
//! [`restart_refresh`] and nothing else. It cancels whatever task is live and
//! arms a new one, and it is called from exactly two places: `on_load` and the
//! *success* path of [`reload`].
//!
//! It used to be split in half. `state::install` cancelled the loop - sensibly,
//! because a changed `refresh-interval-ticks` has to be rescheduled - and only
//! `on_load` ever armed a replacement, so the first `/tab reload` left the plugin
//! with no loop at all: the tab list froze at whatever the command's one-shot
//! refresh had pushed, and placeholders, latency and animations only moved again
//! after a restart. Both halves now live in one function, which is why a failed
//! reload is still free to keep the old loop on the old interval: it never
//! reaches the arming call.
//!
//! ## What the host will not let a plugin do
//!
//! All read out of the host's source, all of them shaping the design:
//!
//! - **No per-viewer tab-list name.** `set-tab-list-name`, `set-tab-list-order`
//!   and `set-tab-list-latency` all call `world.broadcast_packet_all`
//!   (`entity/player.rs:1006`, `:1022`, `:1034`). They look per-player and are
//!   not. The header and footer *are* per-viewer
//!   (`entity/player.rs:925`, `try_send_client_packet` and not a broadcast), and
//!   are Java-only.
//! - **`hide-player` does nothing to a client.** `hide_player` only inserts into
//!   `hidden_players` and emits no packet at all
//!   (`entity/player.rs:3848`), so nothing reaches the tab list. The set *is*
//!   read, by `can_see` (`entity/player.rs:3862`), but that only answers a
//!   plugin's own question; it is not what the client draws.
//!   `set-tab-list-listed` (`entity/player.rs:1046`) is the only real way to
//!   hide an entry.
//! - **`get-ping` is 0 for every Bedrock client**, because the Java keep-alive
//!   handler holds the only two writers of `player.ping`
//!   (`net/java/play/keep_alive.rs:17` and `:24`) and a Bedrock connection never
//!   reaches it. It is pushed anyway; the host keeps a separate
//!   `tab_list_latency` field that otherwise stays 0 and leaves every client
//!   showing no bars.
//! - **No fake or offline players.** `CPlayerInfoUpdate` has no arm in the
//!   packet serializer and `send-packet` discards that silently, returning `Ok(())`
//!   (`wasm_host/wit/v0_1/player.rs:3577-3581`).
//! - **A team can hold any string, not just a player.** `add-player-to-team` is
//!   called `player` throughout the WIT, but the host does `team.players.push(name)`
//!   with no lookup of any entity, so a free-form sidebar line is built as a team
//!   with an invisible entry name. See [`scoreboard`].
//!
//! ## Compatibility
//!
//! A component only loads on a server built from a commit with the same
//! `pumpkin-plugin-api` WIT, and **the version number cannot be used to check**:
//! Pumpkin kept `0.2.0+26.3-26.51` across a WIT change. A mismatch shows as
//! `type-checking export func 'handle-event'` and nothing else. This build is
//! pinned to Pumpkin `0.2.0+26.3-26.51`, commit
//! `204a94ed895f041a845630d5a23c094705a0704e`, plugin WIT
//! `e0bb97ea48d9857c181356ba14964d47b20d0f56`.

use std::{collections::BTreeSet, sync::OnceLock};

use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Result, Server,
    permission::PermissionDefault,
    events::{EventData, EventPriority, PlayerJoinEvent, PlayerLeaveEvent},
    player::Player,
};
use tracing::{debug, error, info, warn};

#[cfg(target_arch = "wasm32")]
use pumpkin_plugin_api::register_plugin;

/// Command registration and the handlers behind it.
pub mod commands;
/// The configuration and its models.
pub mod config;
/// Which group a player is shown as.
pub mod groups;
/// The shared bossbar.
pub mod bossbar;
/// Placeholder resolution.
pub mod render;
/// Asking PumpkinPIE for whatever the built-ins could not answer.
pub mod pie;
/// The sidebar.
pub mod scoreboard;
/// The plugin's state.
pub mod state;
mod storage;
/// The refresh, and everything it pushes.
pub mod tab;
/// Tags, the component builder, legacy codes and `%placeholder%` scanning.
pub mod text;

/// The plugin name, and therefore the only permission namespace it may register.
pub const PLUGIN: &str = "PumpkinTAB";

/// `/tab`.
pub const USE: &str = "PumpkinTAB:use";

/// Every node this plugin registers.
///
/// Spelled as a literal because `concat!` needs a literal rather than a const, so
/// it cannot be derived from [`PLUGIN`]. The test below is what ties the two
/// together, and it is why renaming the plugin cannot leave a node stranded in
/// the old namespace.
pub const COMMAND_NODES: [&str; 1] = [USE];

/// The provider's plugin name, and so its IPC address.
///
/// Named nowhere in the plugin metadata's `dependencies`: PIE is a soft
/// dependency, and a hard one would make this plugin refuse to load without it.
const PROVIDER: &str = "PumpkinPIE";

/// The plugin's data folder as seen from inside the WASI sandbox.
///
/// The host returns the guest path of its preopen, which is always `data`
/// (`wasm_host/wit/v0_1/context.rs:1731` hardcodes it); the real directory is
/// `plugins/data/PumpkinTAB` on the host side.
static DATA_FOLDER: OnceLock<String> = OnceLock::new();

/// Re-reads `config.yml`, keeping the previous one if it is broken.
///
/// Returns whether the reload was clean, along with anything that went wrong.
///
/// On the clean path the refresh is re-armed at the new interval, so a changed
/// `refresh-interval-ticks` takes effect without a restart. On the broken path
/// nothing is touched at all - not the configuration, not the sidebar handles,
/// not the bar, and not the running loop, which keeps ticking on the old interval
/// with the old configuration until a reload succeeds.
pub fn reload() -> Option<(bool, Vec<String>)> {
    let folder = DATA_FOLDER.get()?.clone();
    let loaded = storage::load(&folder);

    // A broken config is not swapped in. Reverting to defaults would silently
    // flatten every group and every animation, which is worse than not reloading.
    let Some(problem) = loaded.error else {
        let refresh = loaded.config.refresh_ticks();
        state::install(loaded.config);
        restart_refresh(refresh);
        return Some((true, Vec::new()));
    };
    Some((
        false,
        vec![format!("{}: {problem}", config::CONFIG_FILE)],
    ))
}

/// Cancels the live refresh and arms a new one every `ticks` ticks.
///
/// The only place a refresh task is created, and the reason a `/tab reload` leaves
/// the plugin still updating. See the module header for how the two halves of
/// this used to be split across `state::install` and `on_load`.
///
/// Cancel-then-schedule rather than the other way round: `schedule_repeating_task`
/// sets the first run one whole period out (`server/scheduler.rs:112`), so
/// cancelling afterwards would also be safe, but taking the old id out first means
/// the window in which *no* loop is armed is the length of two host calls rather
/// than a full interval, and nothing else in the plugin has to reason about the
/// order.
///
/// One caveat worth knowing, because it is a host property and not a choice here:
/// `schedule_repeating_task` registers a fresh closure in the API's handler table
/// on every call (`pumpkin-plugin-api/src/scheduler.rs:132`) and `cancel_task`
/// removes only the task, never the handler (`pumpkin-plugin-api/src/scheduler.rs:140`).
/// So a reload strands one dead closure.
/// That is a few bytes per `/tab reload` - an admin action - rather than per tick,
/// so it is not a reason to leave the tab list frozen instead.
pub fn restart_refresh(ticks: u64) {
    if let Some(stale) = state::take_refresh() {
        pumpkin_plugin_api::scheduler::cancel_task(stale);
    }
    // Displacing nothing is the point: the `take` above is what makes it so. If
    // this ever returns a task, something armed a loop without going through here.
    state::arm_refresh(pumpkin_plugin_api::scheduler::schedule_repeating_task(
        ticks, ticks, tick,
    ));
    debug!("the refresh is armed again, every {ticks} tick(s)");
}

/// Creates the bar and the sidebar objective, once, if they are enabled.
///
/// Neither host handle can be rebuilt per refresh: `world.get-scoreboard` pushes a
/// new resource-table entry on every call and nothing reuses one, and a bar is a
/// thing a player can see. Both are created here, once, and then used in place.
pub fn ensure_assets(snapshot: &state::Snapshot, server: &Server) {
    ensure_bar(snapshot, server);
    ensure_scoreboard(snapshot, server);
}

/// The bar, created once and then kept.
fn ensure_bar(snapshot: &state::Snapshot, server: &Server) {
    if !snapshot.config.bossbar.enabled {
        return;
    }
    // A view with no player: the bar has one title for everybody.
    let view = render::View {
        config: &snapshot.config,
        clock: snapshot.clock,
        server,
        player: None,
        group: None,
    };
    match bossbar::create(&snapshot.config.bossbar, &view) {
        Some(bar) => {
            state::set_bar(bar);
            info!("the bossbar is created; it appears as players join");
        }
        // Only reachable when the feature is on but the bar could not be built,
        // which the config's own `problems` list should already have explained.
        None => warn!("the bossbar is enabled but no bar could be created"),
    }
}

/// The sidebar, one board per player, obtained once each and kept.
///
/// A player gets their own scoreboard rather than the world's, because
/// `java-player.get-scoreboard` returns a handle whose provider is that one
/// player. That is what lets a line resolve `%perm_prefix%` or `%player%`
/// against the reader instead of against nobody.
///
/// The teams and the objective are the same *names* on every board, so one build
/// tells us they exist and the rest follow the same shape.
fn ensure_scoreboard(snapshot: &state::Snapshot, server: &Server) {
    if !snapshot.config.scoreboard.enabled {
        // Turning the sidebar off has to take it off the board. Nothing else ever
        // removes the objective, the teams or the rows, so without this a reload to
        // `enabled: false` would leave the last sidebar frozen on every client.
        if let Some(handles) = state::take_scoreboards() {
            for (player, board) in handles {
                for index in 0..state::line_count() {
                    board.remove_score(&scoreboard::line_key(index), scoreboard::OBJECTIVE);
                    board.remove_team(&scoreboard::line_team(index));
                }
                for team in state::teams() {
                    board.remove_team(&team);
                }
                board.clear_display_slot(pumpkin_plugin_api::scoreboard::DisplaySlot::Sidebar);
                board.remove_objective(scoreboard::OBJECTIVE);
                info!("the sidebar is disabled; the objective was removed for {player}");
            }
            state::set_objective_exists(false);
            state::set_line_count(0);
        }
        return;
    }
    if !state::needs_scoreboard() {
        return;
    }
    let view = render::View {
        config: &snapshot.config,
        clock: snapshot.clock,
        server,
        player: None,
        group: None,
    };
    // Read before it is overwritten: the previous line count is what lets a reload
    // that shortened `lines:` remove the rows the old config left behind.
    let previous_lines = state::line_count();
    let mut objective_exists = state::objective_exists();
    let mut teams = BTreeSet::new();
    let line_count = snapshot.config.scoreboard.lines.len();

    for player in server.get_all_players() {
        let name = player.get_name();
        let world = player.get_world().get_name();
        // `as_java` rather than assuming: the sidebar is written through the Java
        // scoreboard API, which Bedrock has a separate and smaller version of.
        let Some(java) = player.as_java() else {
            debug!("{name} is not a Java player; the sidebar is skipped for them");
            continue;
        };
        if !snapshot.config.scoreboard.applies_in(&world) {
            debug!("{name} is in world '{world}', which is under scoreboard.disabled-worlds");
            continue;
        }
        let handle = java.get_scoreboard();
        let made = scoreboard::create(
            &handle,
            &snapshot.config.scoreboard,
            &view,
            previous_lines,
            objective_exists,
        );
        if teams.is_empty() {
            teams = made.teams;
        }
        objective_exists = made.objective_exists;
        state::insert_scoreboard(name.clone(), handle);
    }

    if teams.is_empty() {
        // Either nobody is here, or everybody is on a client or in a world the
        // sidebar is off for. Both are ordinary; the next refresh builds it when
        // that stops being true.
        // The count still has to be recorded, or the next build would compare the
        // new `lines:` against the count from two config versions ago and leave
        // rows behind.
        state::set_line_count(line_count);
        return;
    }
    state::set_teams(teams);
    state::set_line_count(line_count);
    state::set_objective_exists(objective_exists);
    info!(
        "the sidebar objective '{}' is on the sidebar slot for each player, with {} group \
         team(s) and {line_count} fixed line(s)",
        scoreboard::OBJECTIVE,
        state::teams().len()
    );
}

/// Runs the refresh once. The body of the loop armed by [`restart_refresh`].
///
/// Returns without touching anything before the plugin has installed a
/// configuration, which is the state a scheduled task can be in if `on_load`
/// failed after arming: the host does not unwind a failed load, so the task can
/// outlive the error. Doing nothing is the right answer there, and it does not
/// stop the loop from being cancelled by `on_unload`.
///
/// Everything that walks the player list happens in [`tab::refresh_all`], not
/// here, even though this is where the loop lives. It needs the list of players
/// who are online, and `get_all_players` pushes a **fresh** resource-table entry
/// for every player on every call (`wasm_host/wit/v0_1/server.rs:108-122`, into
/// `state.rs:177`, whose `add` is `resource_table.push` and nothing reuses one).
/// Asking twice a refresh would double that once a second per player to
/// re-derive a list `refresh_all` is already walking.
fn tick(server: Server) {
    let Some(snapshot) = state::snapshot() else {
        return;
    };
    ensure_assets(&snapshot, &server);
    tab::refresh_all(&server, &snapshot);
}

/// One player's refresh, immediately rather than on the next tick.
fn apply(server: &Server, player: &Player) {
    let Some(snapshot) = state::snapshot() else {
        return;
    };
    ensure_assets(&snapshot, server);
    tab::apply_player(server, &snapshot, player);
}

/// The expansion plugin. Holds no state of its own; the globals it would keep are
/// in [`state`].
pub struct PumpkinTAB;

impl Plugin for PumpkinTAB {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            authors: vec!["Rennex".to_string()],
            description: "A tab list for Pumpkin: groups, sorting, animations, a header and \
                          footer, a bossbar and a sidebar."
                .to_string(),
            // PumpkinPIE is deliberately absent. It is a soft dependency, and
            // naming it here would make this plugin refuse to load without it.
            dependencies: Vec::new(),
            permissions: vec![
                // The data folder is how the configuration survives a restart.
                // Without these the host does not preopen it and every `std::fs`
                // call fails.
                //
                // Note the host asks for these interactively on first load, and
                // skips the plugin when it cannot ask. See the README.
                pumpkin_plugin_api::permissions::FS_READ_DATA.to_string(),
                pumpkin_plugin_api::permissions::FS_WRITE_DATA.to_string(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> Result<()> {
        let folder = context.get_data_folder();
        // `set` rather than `get_or_init`: on_load runs once, and a second
        // `get_or_init` would be silently wrong if it ever did not.
        let _ = DATA_FOLDER.set(folder.clone());

        let loaded = storage::load(&folder);

        // A tab list that could not read its own configuration must not start.
        // Falling back to defaults would quietly flatten every group.
        if let Some(problem) = &loaded.error {
            return Err(format!(
                "{} in {folder} could not be read ({problem}); the shipped default was NOT used, \
                 because guessing at a tab list is worse than not starting",
                config::CONFIG_FILE
            ));
        }
        if loaded.created {
            info!("wrote the default {} to {folder}", config::CONFIG_FILE);
        }

        for problem in loaded.config.problems() {
            warn!("config.yml: {problem}");
        }

        let group_count = loaded.config.groups_by_order().len();
        let animation_count = loaded.config.animations.len();
        let refresh = loaded.config.refresh_ticks();
        state::install(loaded.config);

        // Just the command node. Groups are *not* registered here: the node that
        // names a group belongs to the permission plugin that owns it
        // (`config::PERM_PLUGIN`), and registering `PumpkinPerm:group.<name>` under
        // this plugin's own name would be a second, conflicting answer.
        let node_count = register_nodes(&context);

        // `USE` already contains a colon, so the host does not prefix it again.
        // It would otherwise become `PumpkinTAB:PumpkinTAB:use` and never resolve.
        context.register_command(commands::command(), USE);

        // Join is blocking so the tab list is right before the player sees it;
        // leave is not, because nothing needs to be in place afterwards.
        context.register_event_handler::<PlayerJoinEvent, _>(OnJoin, EventPriority::Normal, true)?;
        context.register_event_handler::<PlayerLeaveEvent, _>(OnLeave, EventPriority::Low, false)?;

        // The refresh loop. Twenty ticks is one second and is the floor the config
        // can ask for; every setter here broadcasts to the world, so a faster
        // loop spends bandwidth the client cannot use.
        restart_refresh(refresh);

        info!(
            "{PLUGIN} {} ready: {group_count} group(s), {animation_count} animation(s), \
             {node_count} permission node(s), refreshing every {refresh} tick(s). \
             {PROVIDER} is a soft dependency; anything it does not answer is left in the text as \
             written.",
            env!("CARGO_PKG_VERSION")
        );
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<()> {
        // Cancels the refresh and empties the bar. Deliberately does not message
        // PumpkinPIE: plugins unload in load order, so the provider's store is
        // already gone and messaging it traps the guest.
        state::unload();
        info!("{PLUGIN} unloaded");
        Ok(())
    }
}

/// Registers the command node and one node per group.
///
/// Returns how many distinct nodes ended up registered, so the startup line can
/// say so rather than leaving the admin to infer it from silence.
fn register_nodes(context: &Context) -> usize {
    let mut registered: std::collections::BTreeSet<String> = Default::default();

    for node in COMMAND_NODES {
        if register(context, node, "Allows using /tab.", operator_default()) {
            registered.insert(node.to_string());
        }
    }

    registered.len()
}

/// Registers one node, logging a real failure and ignoring a repeat.
///
/// Returns whether the node is registered, so a count can be reported.
///
/// Registration is not idempotent on the host: a second call for the same node
/// returns `Err("Permission … is already registered")`. That is not fatal, so it
/// is logged and dropped, which is what makes a `/tab reload` safe.
fn register(context: &Context, node: &str, description: &str, default: PermissionDefault) -> bool {
    use pumpkin_plugin_api::permission::Permission;

    match context.register_permission(&Permission {
        node: node.to_string(),
        description: description.to_string(),
        default,
        // Always empty on purpose. The host only reads a permission's `children`
        // when walking a player's *attachment* parents
        // (`pumpkin-util/src/permission.rs:363`); the registry-default path reads
        // `default` and nothing else, so declaring children would have no effect
        // on any default and would read as if it did.
        children: Vec::new(),
    }) {
        Ok(()) => {
            tracing::debug!("registered permission {node}");
            true
        }
        Err(message) if message.contains("already registered") => {
            tracing::debug!("permission {node} was already registered");
            true
        }
        Err(message) => {
            // A namespace mistake lands here, and every command behind the node
            // becomes invisible to everybody, operators included.
            error!("could not register {node}: {message}");
            false
        }
    }
}

fn operator_default() -> PermissionDefault {
    pumpkin_plugin_api::permission::PermissionDefault::Op(
        pumpkin_plugin_api::permission::PermissionLevel::Two,
    )
}

/// Puts one freshly-joined player's tab list in place immediately.
struct OnJoin;

impl pumpkin_plugin_api::EventHandler<PlayerJoinEvent> for OnJoin {
    fn handle(&self, server: Server, event: EventData<PlayerJoinEvent>) -> EventData<PlayerJoinEvent> {
        apply(&server, &event.player);
        event
    }
}

/// Clears a departing player's sidebar row.
struct OnLeave;

impl pumpkin_plugin_api::EventHandler<PlayerLeaveEvent> for OnLeave {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerLeaveEvent>,
    ) -> EventData<PlayerLeaveEvent> {
        if let Some(snapshot) = state::snapshot() {
            tab::clear_player(&snapshot, &event.player);
        }
        event
    }
}

// The `init-plugin` export a component needs, and nothing on a host build wants.
// Gating to `wasm32` keeps `pumpkin-pie`'s own copy of the symbol from colliding
// with this one when the test harness links; dropping it entirely would fail the
// link outright with "failed to find export of function `init-plugin`".
#[cfg(target_arch = "wasm32")]
register_plugin!(PumpkinTAB);

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_plugin_api::Plugin;

    #[test]
    fn every_node_this_plugin_registers_is_in_its_own_namespace() {
        // The host enforces this with an error, and the consequence is invisible:
        // the command reports "Unknown command" for everybody, operators included.
        for node in COMMAND_NODES {
            assert!(
                node.starts_with(&format!("{PLUGIN}:")),
                "{node} is outside the {PLUGIN}: namespace"
            );
        }
    }

    #[test]
    fn the_metadata_name_is_the_namespace() {
        // The host matches against `metadata.name`, not against a constant.
        assert_eq!(PumpkinTAB::new().metadata().name, PLUGIN);
    }

    #[test]
    fn pie_is_a_soft_dependency() {
        // Named nowhere in `dependencies`, which is what makes "loaded without
        // PumpkinPIE" a supported configuration rather than a failure.
        assert_eq!(PROVIDER, "PumpkinPIE");
        assert!(PumpkinTAB::new().metadata().dependencies.is_empty());
    }

    #[test]
    fn the_data_folder_falls_back_rather_than_panicking() {
        // Callable before `on_load`, from a test. Must not panic.
        assert_eq!(DATA_FOLDER.get().map(String::as_str), None);
    }

    #[test]
    fn a_reload_before_the_plugin_has_loaded_is_refused_rather_than_guessed() {
        // The `None` arm is what makes `/tab reload` answer "has no state to
        // reload" instead of reading a folder that was never opened. Reachable
        // from a test because the `OnceLock` is still empty here and the early
        // return happens before any host call.
        assert_eq!(crate::reload(), None);
    }

    /// Deliberately absent: a test that calls [`restart_refresh`] itself.
    ///
    /// `schedule_repeating_task` and `cancel_task` reach the host through
    /// wit-bindgen imports, and on a host build those are `unreachable!()` stubs,
    /// so the call panics rather than returning an error - the same trap
    /// `pie.rs` documents for the provider. The bookkeeping the reschedule
    /// performs is in [`state::Refresh`], where it is tested with dummy ids; the
    /// scheduling itself is verified on the server, by running `/tab reload` twice
    /// and then leaving a player connected and watching the tab list keep moving
    /// with no further command.
    #[test]
    fn a_restarted_refresh_is_never_armed_at_zero() {
        // `restart_refresh` hands this straight to `schedule_repeating_task` as
        // both the delay and the period. The host sets the first run one whole
        // period out (`server/scheduler.rs:112`), so a zero would be a task that
        // never fires at all, and the clamp is what keeps a `refresh-interval-ticks`
        // of 0 in config.yml from silently stopping the tab list.
        for asked in [0, 1, 20] {
            let config = crate::config::Config {
                refresh_interval_ticks: asked,
                ..Default::default()
            };
            assert_eq!(config.refresh_ticks(), asked.max(1));
            assert!(config.refresh_ticks() > 0);
        }
        // `Config::default()` is what a config that failed to parse falls back to,
        // so it has to be a usable interval rather than the `0` a derived
        // `Default` would leave in the field. Only `refresh_ticks` would otherwise
        // save it, which means a reload that passed the raw field would arm a task
        // with a zero period.
        assert_eq!(crate::config::DEFAULT_REFRESH_TICKS, 20);
        assert_eq!(crate::config::Config::default().refresh_interval_ticks, 20);
    }
}
