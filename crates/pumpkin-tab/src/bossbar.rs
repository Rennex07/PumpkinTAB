//! The shared bossbar.
//!
//! One bar, everybody sees it. The alternative, a bar per player, is what lets
//! each player get their own text, and it is not what is here: the bar's *title*
//! is one string, so a per-player bar would be one host resource per player
//! multiplied by the number of refreshes, for no benefit on a server where the
//! text is server-wide anyway.
//!
//! ## Why the bar is created once and kept
//!
//! `BossBar::new` pushes a new entry into the plugin's resource table every call
//! (`state.rs:325` for the scoreboard equivalent, and the bossbar's `new` does
//! the same). Nothing reuses one. A bar rebuilt every refresh would grow that
//! table for as long as the server is up, so it is created once and its handle
//! kept. `remove_all` on unload, and the guest handle drop takes the table entry
//! with it.
//!
//! ## The colour is a template, which is the point
//!
//! `color` goes through the same resolution as everything else, so an animation
//! whose frames are the bare words `RED`, `BLUE`, `GREEN` cycles the bar's
//! colour. That is the one place a placeholder resolves to a value rather than
//! to display text, which is why the result is matched against the colour names
//! instead of being trusted.

use pumpkin_plugin_api::{
    boss_bar::{BossBar, BossBarColor, BossBarDivision},
    player::Player,
    text::TextComponent,
};
use tracing::warn;

use crate::{
    config::BossBar as BossBarConfig, render::View, text,
};
/// Every colour the host accepts, lower case, for matching and for the error
/// message.
///
/// The host's enum has exactly these seven (`boss-bar.wit:9`). There is no black
/// bossbar in the protocol, so a name that is not in this list can never be
/// reached, whatever the config asks for.
pub const COLOURS: [&str; 7] = ["pink", "blue", "red", "green", "yellow", "purple", "white"];

/// The host colour for a name, or [`None`] if the name is not one.
pub fn colour(name: &str) -> Option<BossBarColor> {
    // Trimmed and case-folded, because the value arrives from a template that may
    // have resolved an animation frame, and an animation's frames are written by
    // hand.
    match name.trim().to_ascii_lowercase().as_str() {
        "pink" => Some(BossBarColor::Pink),
        "blue" => Some(BossBarColor::Blue),
        "red" => Some(BossBarColor::Red),
        "green" => Some(BossBarColor::Green),
        "yellow" => Some(BossBarColor::Yellow),
        "purple" => Some(BossBarColor::Purple),
        "white" => Some(BossBarColor::White),
        _ => None,
    }
}

/// The host division for a configured name, defaulting to no division.
pub fn division(name: &str) -> BossBarDivision {
    match name.trim().to_ascii_lowercase().as_str() {
        "notches6" => BossBarDivision::Notches6,
        "notches10" => BossBarDivision::Notches10,
        "notches12" => BossBarDivision::Notches12,
        "notches20" => BossBarDivision::Notches20,
        _ => BossBarDivision::NoDivision,
    }
}

/// The title component for a bar, from an already-resolved title.
///
/// [`text::component`], like every other piece of text this plugin pushes, so
/// the bar's title is built the same way as the tab-list name and the sidebar -
/// and so a `<gradient:…>` in `title:` reaches the bar rather than being
/// flattened away by a `&`-coded string.
pub fn title_component(resolved: &str) -> TextComponent {
    text::component(resolved)
}

/// Creates the bar described by `config`, or [`None`] if it is disabled.
pub fn create(config: &BossBarConfig, view: &View<'_>) -> Option<BossBar> {
    if !config.enabled {
        return None;
    }
    let title = crate::render::render_locally(view, &config.title);
    let bar = BossBar::new(
        title_component(&title),
        // The colour is resolved but deliberately *not* applied here: a colour
        // template is normally an animation, and an animation's first frame would
        // otherwise be baked in and then never move.
        BossBarColor::Pink,
        division(&config.division),
    );
    bar.set_health(config.health);
    Some(bar)
}

/// Pushes the current title and colour onto an existing bar.
pub fn update(bar: &BossBar, config: &BossBarConfig, view: &View<'_>) {
    if !config.enabled {
        return;
    }

    let title = crate::render::render_locally(view, &config.title);
    bar.set_title(title_component(&title));

    let resolved = crate::render::render_locally(view, &config.color);
    match colour(&resolved) {
        Some(colour) => bar.set_color(colour),
        None => {
            // Once, not once a second: a bad colour template is a config mistake
            // and repeating it every refresh would bury everything else in the
            // log.
            static ONCE: std::sync::atomic::AtomicBool = const { std::sync::atomic::AtomicBool::new(false) };
            if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                warn!(
                    "bossbar colour template resolved to '{resolved}', which is not a bossbar \
                     colour; expected one of {}. The bar keeps its last colour.",
                    COLOURS.join(", ")
                );
            }
        }
    }
}

/// Whether `player` should be shown the bar, given the worlds it is off in.
pub fn applies(config: &BossBarConfig, player: &Player) -> bool {
    config.applies_in(&player.get_world().get_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_colour_has_a_name() {
        // A name the host accepts but this list does not would be a colour an
        // animation could never reach.
        assert_eq!(COLOURS.len(), 7);
        for name in COLOURS {
            assert!(colour(name).is_some(), "{name} is listed but not matched");
        }
    }

    #[test]
    fn colour_names_are_matched_without_regard_to_case_or_space() {
        // The value arrives from a template, which may have resolved an
        // animation frame written by hand.
        assert_eq!(colour("RED"), Some(BossBarColor::Red));
        assert_eq!(colour("  blue  "), Some(BossBarColor::Blue));
        assert_eq!(colour("Light_Purple"), None);
    }

    #[test]
    fn an_unknown_colour_is_refused() {
        // A config mistake, and the bossbar feature says so rather than guessing.
        assert_eq!(colour("&cred"), None);
        assert_eq!(colour(""), None);
        assert_eq!(colour("orange"), None);
    }

    #[test]
    fn divisions_map_onto_the_host_enum() {
        assert_eq!(division("no-division"), BossBarDivision::NoDivision);
        assert_eq!(division("notches6"), BossBarDivision::Notches6);
        assert_eq!(division("notches20"), BossBarDivision::Notches20);
        assert_eq!(division("NOTCHES10"), BossBarDivision::Notches10);
    }

    #[test]
    fn an_unknown_division_falls_back_rather_than_failing() {
        // `Config::problems` already reports a bad division; the feature must not
        // also refuse to build a bar over it.
        assert_eq!(division("notches7"), BossBarDivision::NoDivision);
    }
}
