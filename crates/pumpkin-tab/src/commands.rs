//! The `/tab` command tree.
//!
//! ## Where the permission checks live
//!
//! `context.register_command` attaches one permission node to the whole tree, so
//! the coarse check is [`USE`] on the root. The per-node alternative,
//! `command-node.require-with-handler-id`, is a stub on this version of the host
//! that always returns `Err("require_with_handler_id not implemented")`
//! (`wasm_host/wit/v0_1/commands/mod.rs:763`), and calling it traps the command.
//!
//! ## Why every subcommand is reachable from the console
//!
//! The console sender always passes permission checks on the host
//! (`CommandSender::has_permission` returns `true` for `Console` and `Rcon`
//! before any registry lookup), and that is the whole test harness: Minecraft
//! cannot be played here, so the console is how these commands are exercised.
//! It is also why `/tab preview` exists at all - it renders a template and prints
//! the result, which is the only way to see what a placeholder resolves to
//! without a client.

use pumpkin_plugin_api::{
    Server,
    command::{Command, CommandError, CommandNode, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
};

use crate::{
    render::{self, Clock, View},
    state, tab, text,
};

/// `/tab`.
pub fn command() -> Command {
    Command::new(&["tab".to_string()], "Inspect and reload the PumpkinTAB tab list")
        .then(CommandNode::literal("reload").execute(Reload))
        .then(CommandNode::literal("groups").execute(Groups))
        .then(CommandNode::literal("placeholders").execute(Placeholders))
        .then(
            CommandNode::literal("preview")
                .then(CommandNode::argument("template", greedy()).execute(Preview)),
        )
        .then(
            CommandNode::literal("check").then(
                CommandNode::argument("player", single_word()).execute(Check),
            ),
        )
}

fn single_word() -> &'static pumpkin_plugin_api::command::ArgumentType {
    use pumpkin_plugin_api::command::{ArgumentType, StringType};
    static TYPE: std::sync::OnceLock<ArgumentType> = std::sync::OnceLock::new();
    TYPE.get_or_init(|| ArgumentType::String(StringType::SingleWord))
}

fn greedy() -> &'static pumpkin_plugin_api::command::ArgumentType {
    use pumpkin_plugin_api::command::{ArgumentType, StringType};
    static TYPE: std::sync::OnceLock<ArgumentType> = std::sync::OnceLock::new();
    TYPE.get_or_init(|| ArgumentType::String(StringType::Greedy))
}

fn reply(sender: &CommandSender, message: &str) {
    sender.send_message(TextComponent::text(message));
}

/// Reports a problem and yields a non-zero exit code.
///
/// Every user-facing failure goes through here rather than through
/// `CommandError::CommandFailed`. The host maps that variant onto
/// `DISPATCHER_PARSE_EXCEPTION`, which renders as
/// `Syntax error: Unexpected "<the whole message>"` with the text repeated and
/// quote marks around it (`wasm_host/wit/v0_1/commands/executor.rs:44`).
fn soft_error(sender: &CommandSender, message: impl Into<String>) -> Result<i32, CommandError> {
    sender.send_error(TextComponent::text(&message.into()));
    Ok(1)
}

/// `/tab reload`
struct Reload;

impl CommandHandler for Reload {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some((clean, problems)) = crate::reload() else {
            return soft_error(&sender, "PumpkinTAB has no state to reload.");
        };
        if !clean {
            return soft_error(
                &sender,
                format!(
                    "config.yml could not be reloaded: {}. The previous configuration and the \
                     previous refresh interval are still in effect.",
                    problems.join("; ")
                ),
            );
        }

        // The snapshot has to be taken *after* `ensure_assets`, not before.
        // `install` dropped every sidebar handle, and `ensure_assets` is what
        // builds the new ones; a snapshot from before it lists no worlds, so the
        // refresh that follows writes to nothing and the edit appears not to have
        // taken until the next tick.
        let snapshot = match state::snapshot() {
            Some(snapshot) => snapshot,
            None => return soft_error(&sender, "PumpkinTAB has no state after reloading."),
        };
        crate::ensure_assets(&snapshot, &server);
        let snapshot = match state::snapshot() {
            Some(snapshot) => snapshot,
            None => return soft_error(&sender, "PumpkinTAB has no state after reloading."),
        };
        // `reload` has already re-armed the loop at the new interval, so this is
        // only so the admin sees the edit on their own client without waiting out
        // a tick. Everything from here on is the loop's job.
        tab::refresh_all(&server, &snapshot);

        let warnings = snapshot.config.problems();
        let mut message = format!(
            "PumpkinTAB reloaded: {} group(s), {} animation(s), refreshing every {} tick(s).",
            snapshot.config.groups_by_order().len(),
            snapshot.config.animations.len(),
            snapshot.config.refresh_ticks()
        );
        for warning in warnings.iter().chain(problems.iter()) {
            message.push_str(&format!("\n! {warning}"));
        }
        reply(&sender, &message);
        Ok(if warnings.is_empty() { 0 } else { 1 })
    }
}

/// `/tab groups`
struct Groups;

impl CommandHandler for Groups {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(snapshot) = state::snapshot() else {
            return soft_error(&sender, "PumpkinTAB has no state; it did not finish loading.");
        };
        let config = &snapshot.config;

        if config.groups_by_order().is_empty() {
            reply(&sender, "No groups are configured; everyone is in the default group.");
            return Ok(0);
        }

        let mut message = format!(
            "PumpkinTAB groups ({}), highest first:",
            config.groups_by_order().len()
        );
        for (index, name) in config.groups_by_order().iter().enumerate() {
            let node = config
                .node_for(name)
                .unwrap_or_else(|| "<unusable name>".to_string());
            let op_levels: Vec<String> = config
                .op_level_groups
                .iter()
                .filter(|(_, group)| group.as_str() == name.as_str())
                .map(|(level, _)| level.as_str().to_string())
                .collect();
            let via = if op_levels.is_empty() {
                String::new()
            } else {
                format!(", or op level {}", op_levels.join("/"))
            };
            message.push_str(&format!(
                "\n{:>4}. {:<12} sort {:>4}  via '{node}'{via}",
                index + 1,
                name,
                config.sort_value(index)
            ));
        }
        message.push_str(&format!("\nfallback: {}", crate::config::DEFAULT_GROUP));
        reply(&sender, &message);
        Ok(0)
    }
}

/// `/tab placeholders`
struct Placeholders;

impl CommandHandler for Placeholders {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(snapshot) = state::snapshot() else {
            return soft_error(&sender, "PumpkinTAB has no state; it did not finish loading.");
        };

        let mut message = format!(
            "PumpkinTAB answers {} built-in placeholder(s):",
            render::BUILT_INS.len()
        );
        for name in render::BUILT_INS {
            message.push_str(&format!("\n  %{name}%"));
        }
        if snapshot.config.animations.is_empty() {
            message.push_str("\n\nNo animations are configured.");
        } else {
            message.push_str(&format!(
                "\n\n{} animation(s), by %animation:<name>%:",
                snapshot.config.animations.len()
            ));
            for (name, animation) in &snapshot.config.animations {
                message.push_str(&format!(
                    "\n  {name}: every {}ms, {} frame(s)",
                    animation.interval_ms(),
                    animation.texts.len()
                ));
            }
        }
        message.push_str(
            "\n\nAnything else is passed to PumpkinPIE. Whatever neither answers is left in the \
             text as written.",
        );
        reply(&sender, &message);
        Ok(0)
    }
}

/// `/tab preview <template>`
///
/// Renders a template with no player in view, which is what a shared header or
/// the bossbar title looks like, and prints the result **after** the colour
/// conversion, so the legacy codes that will actually be sent are visible.
struct Preview;

impl CommandHandler for Preview {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(snapshot) = state::snapshot() else {
            return soft_error(&sender, "PumpkinTAB has no state; it did not finish loading.");
        };
        let template = match args.get_value("template") {
            pumpkin_plugin_api::command::Arg::Simple(value) if !value.is_empty() => value,
            _ => return soft_error(&sender, "give a template to preview, in quotes if it has spaces"),
        };

        let view = View {
            config: &snapshot.config,
            clock: Clock::start(),
            server: &server,
            player: None,
            group: None,
        };
        let rendered = render::render_locally(&view, &template);
        let legacy = text::to_legacy(&rendered);
        let left = render::count_placeholders(&rendered);

        let mut message = format!("template: {template}");
        message.push_str(&format!("\nrendered: {rendered}"));
        message.push_str(&format!("\nlegacy:   {legacy}"));
        if left == 0 {
            message.push_str("\nevery placeholder was answered locally.");
        } else {
            message.push_str(&format!(
                "\n{left} placeholder(s) are still in the text. This preview has no player in \
                 view, so %player%, %ping% and %group% have nothing to resolve \
                 against and a foreign placeholder needs PumpkinPIE. Both are expected here and \
                 are answered during a real refresh."
            ));
        }
        reply(&sender, &message);
        Ok(0)
    }
}

/// `/tab check <player>`
struct Check;

impl CommandHandler for Check {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(snapshot) = state::snapshot() else {
            return soft_error(&sender, "PumpkinTAB has no state; it did not finish loading.");
        };
        let name = match args.get_value("player") {
            pumpkin_plugin_api::command::Arg::Simple(value) if !value.is_empty() => value,
            _ => return soft_error(&sender, "name a player"),
        };

        let Some(handle) = server.get_player_by_name(&name) else {
            return soft_error(
                &sender,
                format!(
                    "{name} is not online. Group resolution is per connected player, because the \
                     permission manager is asked about that player, so there is nothing to show."
                ),
            );
        };

        let Some((group, sort, reason)) = state::resolve_for(&handle) else {
            return soft_error(&sender, "PumpkinTAB has no state.");
        };
        let world = handle.get_world().get_name();
        let view = View {
            config: &snapshot.config,
            clock: snapshot.clock,
            server: &server,
            player: Some(&handle),
            group: None,
        };

        let mut message = format!("{name} is shown as '{group}'");
        message.push_str(match reason {
            Some("granted") => " because its node is granted.",
            Some("op level") => " because no node is granted but the op level qualifies.",
            _ => " because no group matched, so this is the fallback.",
        });
        message.push_str(&format!("\nsort order: {sort}"));
        message.push_str(&format!("\nworld: {world}"));
        message.push_str(&format!(
            "\nping: {}",
            describe_ping(handle.get_ping())
        ));
        message.push_str(&format!(
            "\nheader/footer here: {}",
            yes_no(snapshot.config.header_footer.applies_in(&world))
        ));
        message.push_str(&format!(
            "\ntab name here: {}",
            yes_no(snapshot.config.tab_list.applies_in(&world))
        ));
        let rendered = render::render_locally(&view, &snapshot.config.tab_list.format);
        message.push_str(&format!(
            "\ntab name renders as: {}",
            tab::component(&rendered).to_pretty_console()
        ));
        reply(&sender, &message);
        Ok(0)
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn describe_ping(millis: u32) -> String {
    if millis == 0 {
        // The host never writes a value for a Bedrock connection, and starts at
        // zero before the first Java keep-alive, so zero means "unknown" rather
        // than "instant".
        "unknown (0 is also what a client with no keep-alive yet reports)".to_string()
    } else {
        format!("{millis}ms")
    }
}

