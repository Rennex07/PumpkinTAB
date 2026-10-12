//! `config.yml`, and nothing else.
//!
//! One file, written on first run from the shipped copy, and rewritten again
//! **only** when what is on disk is still a byte-for-byte copy of a default
//! some earlier build shipped. That exception is the whole of it, and it exists
//! because of one failure mode: the file is written once, so a fix made only to
//! the shipped default is a no-op on every server that already ran the plugin
//! once, and the admin's copy is the one they are actually running.
//!
//! Anything an admin has touched is never rewritten. Overwriting it would throw
//! away the comments, which are the documentation, so the condition is an exact
//! comparison and not a "does it parse" or "does it look close" judgement. The
//! one exception is line endings, which no edit implies; see [`classify`].
//!
//! Unlike PumpkinPerm there is nothing here a command has to write back:
//! assignments live in PumpkinPerm, and every setting here belongs to the admin.
//! So there is nothing to gain from a second file and everything to lose from
//! rewriting this one.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The file name inside the plugin's data folder.
pub const CONFIG_FILE: &str = "config.yml";

/// The shipped `config.yml`, comments and all.
///
/// The file is both the default and the reference, so there is one source of
/// truth rather than a constant in Rust and a file that can disagree.
pub const DEFAULT_CONFIG: &str = include_str!("../config.yml");

/// A `config.yml` that an earlier build of this plugin shipped.
///
/// Kept so [`classify`] can recognise an install that has never been edited.
/// Nothing ever reads one as a fallback: falling back to an old default is
/// exactly the bug they exist to fix, and they are a comparison, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Superseded {
    /// What that default was, for the log line that says what was replaced.
    ///
    /// A description rather than a version number: the crate version that
    /// shipped a given default is not recorded anywhere this module can read,
    /// and a number that cannot be checked is worse than a sentence.
    pub label: &'static str,
    /// What that build wrote, byte for byte, comments included.
    pub text: &'static str,
}

/// Every default this crate has shipped, oldest first.
///
/// **One entry per released default, and never an edited one.** An entry that
/// does not match, byte for byte, what a build actually wrote is worse than no
/// entry at all: it either never fires, or it fires on somebody's own file.
///
/// Adding one means adding the file as it was, not a description of it. A test
/// below asserts that each one still parses, so a stale or mangled copy fails
/// the build rather than sitting here looking plausible.
pub const SUPERSEDED_CONFIGS: &[Superseded] = &[
    Superseded {
        label: "the default whose tab-list.format started with %perm_prefix%",
        // The file as it was before %perm_prefix% was taken out of tab-list.format.
        // PumpkinPerm is not on the market any more, so the shipped default has to
        // work on a server with nothing else installed - and an unanswered placeholder
        // is deliberately left as written rather than blanked, so without this entry
        // every untouched install would show a literal %perm_prefix% in front of
        // every name.
        text: r####"# PumpkinTAB - the tab list, header, footer, bossbar and sidebar.
#
# Placeholders (all work with no other plugin):
#   %player% %viewer% %ping% %playtime% %world% %uuid% %health% %gamemode%
#   %player_ip% %bedrock% %permission_level% %group%
#   %online% %max_players% %worldonline% %server_name% %tps% %mspt%
#   %server_time% %time% %server_date% %date%
#   %animation:<name>%  %condition:<name>%
# Anything else goes to PumpkinPIE (optional). Nothing that cannot answer is
# blanked - it is left as written, so a typo shows up.
#
# Colours `<red>`..`<white>`, `<#e0b11e>`; styles `<bold>` `<italic>` `<reset>` and
# `<bold:false>`; `<gradient:#a:#b>` `<rainbow>`. `&7` codes work too.
#
# Written once. Rewritten only if still a byte-for-byte copy of an older default
# that shipped - edit anything, even a comment, and it is yours.

refresh-interval-ticks: 20

# Whole hours to add to UTC for %time% / %server_time% / %date% /
# %server_date%. `7` for UTC+7, `-5` for UTC-5, `0` to leave it on UTC.
#
# This has to be set by hand: there is no clock, timezone or config read
# anywhere in the plugin API - `server.wit` offers `get-motd` and `get-tps` and
# not one time call - and the only clock a WASI plugin has reads UTC. So the
# sidebar would otherwise show a different hour from the server log sitting
# right next to it. Whole hours only, and no daylight saving: both would need a
# timezone database, which is the same thing that cannot be read.
#
# Times print as `7:04:11 PM`.
utc-offset-hours: 0

# The overlay has a fixed height: overflow it and the tab list is drawn on top of
# the header instead of below it, which looks like a doubled name and a bar
# cutting through the text. This design spends eight rows on header + footer, so
# trim the two '' spacers first if the list ever collides with the header.
header-footer:
  enabled: true
  header:
    # A rule across the top, drawn by striking through a run of spaces. Widen it
    # if the widest row below outgrows it.
    - '<dark_gray>&m                                      &r'
    - '<aqua><bold>✦ YOURSERVER <reset>✦'
    # Small caps, not plain capitals: these are Unicode small-capital letters, the
    # same trick Java TAB's shipped config uses. Substitute your own domain.
    - '<white>ᴘʟᴀʏ.ʏᴏᴜʀꜱᴇʀᴠᴇʀ.ᴄᴏᴍ'
    - ''
  # No name and no %ping% here: the entry row between the header and the footer
  # already carries all three, and repeating them prints each one twice in one
  # glance. %ping% never resolves on Bedrock either - the host only measures
  # latency on the Java keep-alive - and %bedrock% is what tells you so.
  footer:
    - ''
    - '<aqua>/ᴅɪꜱᴄᴏʀᴅ /ꜱᴛᴏʀᴇ /ᴠᴏᴛᴇ'
    - ''
    - '<#D4D4D4>ᴄᴜʀʀᴇɴᴛʟʏ <white>%online%<#D4D4D4> %condition:players% ᴀʀᴇ ᴄᴏɴɴᴇᴄᴛᴇᴅ'
  disabled-worlds: []

tab-list:
  enabled: true
  # %perm_prefix% needs PumpkinPerm AND PumpkinPIE, or it stays as written.
  format: '%perm_prefix%%player% <gray>Ping: <white>%ping%ms'
  disabled-worlds: []

# Highest first, and the position IS the sort order. Keep it the same order as
# PumpkinPerm's weights - nothing reads their weights, so nothing catches a drift.
groups:
  - owner
  - admin
  - moderator
  - vip
  - default

# Lets op count as a group with nobody running /pumpkinperm set. Delete the
# section and op means nothing here.
op-level-groups:
  four: owner
  two: admin

# change-interval is in ms and a frame may itself be a template. It must be a
# whole multiple of the refresh or the client skips frames out of order: a frame
# is picked once per refresh and never in between.
animations:
  logo:
    change-interval: 1000
    texts:
      - '&d-&3--------------'
      - '&3-&d-&3-------------'
      - '&3--&d-&3------------'
      - '&3---&d-&3-----------'
      - '&3----&d-&3----------'
      - '&3-----&d-&3---------'
      - '&3------&d-&3--------'
      - '&3-------&d-&3-------'
      - '&3--------&d-&3------'
      - '&3---------&d-&3-----'
      - '&3----------&d-&3----'
      - '&3-----------&d-&3---'
      - '&3------------&d-&3--'
      - '&3-------------&d-&3-'
      - '&3--------------&d-'
      - '&3-------------&d-&3-'
      - '&3------------&d-&3--'
      - '&3-----------&d-&3---'
      - '&3----------&d-&3----'
      - '&3---------&d-&3-----'
      - '&3--------&d-&3------'
      - '&3-------&d-&3-------'
      - '&3------&d-&3--------'
      - '&3-----&d-&3---------'
      - '&3----&d-&3----------'
      - '&3---&d-&3-----------'
      - '&3--&d-&3------------'
      - '&3-&d-&3-------------'

  clock:
    change-interval: 3000
    texts:
      - '&7Time &b%server_time%'
      - '&7Date &b%server_date%'

  barcolors:
    change-interval: 1000
    texts:
      - 'RED'
      - 'BLUE'
      - 'GREEN'
      - 'YELLOW'

# A named test used as %condition:<name>%. Same shape as Java TAB's, except the
# two answers are `true:` / `false:` where TAB writes `yes:` / `no:`.
# `conditions:` is a list; two or more need `type: AND` (the default) or `type:
# OR`. Comparators: >= > <= < = != <- !<- |- !|- -| !-| plus permission:<node>; a
# non-number side counts as 0. A condition sees only the placeholders above, so a
# test on %perm_group% compares that literal text.
#
# Used by the footer's player/players wording.
conditions:
  players:
    conditions:
      - '%online%<2'
    true: 'ᴘʟᴀʏᴇʀ'
    false: 'ᴘʟᴀʏᴇʀꜱ'

bossbar:
  enabled: false
  title: '&b%server_name% &8| &7%online% online'
  # pink blue red green yellow purple white, or a template.
  color: '%animation:barcolors%'
  # 0.0-1.0, then no-division notches6 notches10 notches12 notches20.
  health: 1.0
  division: 'no-division'
  disabled-worlds: []
"####,
    },

    Superseded {
        label: "the default whose clock was 24-hour UTC with no offset",
        // The file as it was before `utc-offset-hours` was added and the clock
        // switched to 12-hour with an AM/PM meridiem. Without this entry the
        // change reaches nobody: an install that has run this plugin once has
        // this exact file on disk, and `classify` only rewrites a default it
        // recognises byte for byte - so every untouched install would keep
        // printing a 24-hour UTC clock, which is the bug being fixed.
        text: r####"# PumpkinTAB - the tab list, header, footer, bossbar and sidebar.
#
# Placeholders (all work with no other plugin):
#   %player% %viewer% %ping% %playtime% %world% %uuid% %health% %gamemode%
#   %player_ip% %bedrock% %permission_level% %group%
#   %online% %max_players% %worldonline% %server_name% %tps% %mspt%
#   %server_time% %time% %server_date% %date%
#   %animation:<name>%  %condition:<name>%
# Anything else goes to PumpkinPIE (optional). Nothing that cannot answer is
# blanked - it is left as written, so a typo shows up.
#
# Colours `<red>`..`<white>`, `<#e0b11e>`; styles `<bold>` `<italic>` `<reset>` and
# `<bold:false>`; `<gradient:#a:#b>` `<rainbow>`. `&7` codes work too.
#
# Written once. Rewritten only if still a byte-for-byte copy of an older default
# that shipped - edit anything, even a comment, and it is yours.

refresh-interval-ticks: 20

# The overlay has a fixed height: overflow it and the tab list is drawn on top of
# the header instead of below it, which looks like a doubled name and a bar
# cutting through the text. This design spends eight rows on header + footer, so
# trim the two '' spacers first if the list ever collides with the header.
header-footer:
  enabled: true
  header:
    # A rule across the top, drawn by striking through a run of spaces. Widen it
    # if the widest row below outgrows it.
    - '<dark_gray>&m                                      &r'
    - '<aqua><bold>✦ YOURSERVER <reset>✦'
    # Small caps, not plain capitals: these are Unicode small-capital letters, the
    # same trick Java TAB's shipped config uses. Substitute your own domain.
    - '<white>ᴘʟᴀʏ.ʏᴏᴜʀꜱᴇʀᴠᴇʀ.ᴄᴏᴍ'
    - ''
  # No name and no %ping% here: the entry row between the header and the footer
  # already carries all three, and repeating them prints each one twice in one
  # glance. %ping% never resolves on Bedrock either - the host only measures
  # latency on the Java keep-alive - and %bedrock% is what tells you so.
  footer:
    - ''
    - '<aqua>/ᴅɪꜱᴄᴏʀᴅ /ꜱᴛᴏʀᴇ /ᴠᴏᴛᴇ'
    - ''
    - '<#D4D4D4>ᴄᴜʀʀᴇɴᴛʟʏ <white>%online%<#D4D4D4> %condition:players% ᴀʀᴇ ᴄᴏɴɴᴇᴄᴛᴇᴅ'
  disabled-worlds: []

tab-list:
  enabled: true
  # %perm_prefix% needs PumpkinPerm AND PumpkinPIE, or it stays as written.
  format: '%perm_prefix%%player% <gray>Ping: <white>%ping%ms'
  disabled-worlds: []

# Highest first, and the position IS the sort order. Keep it the same order as
# PumpkinPerm's weights - nothing reads their weights, so nothing catches a drift.
groups:
  - owner
  - admin
  - moderator
  - vip
  - default

# Lets op count as a group with nobody running /pumpkinperm set. Delete the
# section and op means nothing here.
op-level-groups:
  four: owner
  two: admin

# change-interval is in ms and a frame may itself be a template. It must be a
# whole multiple of the refresh or the client skips frames out of order: a frame
# is picked once per refresh and never in between.
animations:
  logo:
    change-interval: 1000
    texts:
      - '&d-&3--------------'
      - '&3-&d-&3-------------'
      - '&3--&d-&3------------'
      - '&3---&d-&3-----------'
      - '&3----&d-&3----------'
      - '&3-----&d-&3---------'
      - '&3------&d-&3--------'
      - '&3-------&d-&3-------'
      - '&3--------&d-&3------'
      - '&3---------&d-&3-----'
      - '&3----------&d-&3----'
      - '&3-----------&d-&3---'
      - '&3------------&d-&3--'
      - '&3-------------&d-&3-'
      - '&3--------------&d-'
      - '&3-------------&d-&3-'
      - '&3------------&d-&3--'
      - '&3-----------&d-&3---'
      - '&3----------&d-&3----'
      - '&3---------&d-&3-----'
      - '&3--------&d-&3------'
      - '&3-------&d-&3-------'
      - '&3------&d-&3--------'
      - '&3-----&d-&3---------'
      - '&3----&d-&3----------'
      - '&3---&d-&3-----------'
      - '&3--&d-&3------------'
      - '&3-&d-&3-------------'

  clock:
    change-interval: 3000
    texts:
      - '&7Time &b%server_time%'
      - '&7Date &b%server_date%'

  barcolors:
    change-interval: 1000
    texts:
      - 'RED'
      - 'BLUE'
      - 'GREEN'
      - 'YELLOW'

# A named test used as %condition:<name>%. Same shape as Java TAB's, except the
# two answers are `true:` / `false:` where TAB writes `yes:` / `no:`.
# `conditions:` is a list; two or more need `type: AND` (the default) or `type:
# OR`. Comparators: >= > <= < = != <- !<- |- !|- -| !-| plus permission:<node>; a
# non-number side counts as 0. A condition sees only the placeholders above, so a
# test on %perm_group% compares that literal text.
#
# Used by the footer's player/players wording.
conditions:
  players:
    conditions:
      - '%online%<2'
    true: 'ᴘʟᴀʏᴇʀ'
    false: 'ᴘʟᴀʏᴇʀꜱ'

bossbar:
  enabled: false
  title: '&b%server_name% &8| &7%online% online'
  # pink blue red green yellow purple white, or a template.
  color: '%animation:barcolors%'
  # 0.0-1.0, then no-division notches6 notches10 notches12 notches20.
  health: 1.0
  division: 'no-division'
  disabled-worlds: []

# <text>||<number format>||<score>, any part optional. '' is a blank row, a line
# whose placeholders all come back empty is hidden, a client shows at most 15.
#
# THE ONE PLACE WITH NO VIEWER. One set of rows serves the whole world, so
# %player% %ping% %playtime% %group% print as written - which reads as a broken
# row rather than a typo. Every server-wide placeholder works here. That is why
# Rank, Balance, Kills, Deaths, Claim and Playtime all carry no value, and why
# there is no per-player row either - `entry` below is empty on purpose.
#
# A plugin that answers any of those rows does so by name: whatever it registers
# goes here as `%<its-name>%`.
#
# Ping is on the tab entry, which does have a viewer. Showing it here as well
# reads as a repeat rather than a convenience.
scoreboard:
  enabled: true
  title: '<aqua><bold>✦ YOURSERVER <reset>✦'
  lines:
    # 24-hour UTC. Java TAB's `%server_time_h:mm a%` is not available here: the
    # clock is `HH:MM:SS` in Rust, not a configurable format.
    - '<gray>%time% <#7F7F7F>|<gray> %date%'
    - ''
    # Rank carries no value: %perm_prefix% and %group% both need a viewer and a
    # sidebar line has none, so either one sits here as written text. The prefix
    # is on the tab entry above instead, where it does resolve.
    - '<aqua>| <gray>Rank:'
    - '<aqua>| <gray>Balance:'
    - '<aqua>| <gray>Kills:'
    - '<aqua>| <gray>Deaths:'
    - '<aqua>| <gray>Claim'
    - '<aqua>| <gray>Playtime'
    - ''
    - '<white>ᴘʟᴀʏ.ʏᴏᴜʀꜱᴇʀᴠᴇʀ.ᴄᴏᴍ'
  # integer or hearts.
  render-type: 'integer'
  # Empty on purpose, and it is not the default: an empty entry hides the
  # per-player row entirely, which is what leaves `blockrnc <playtime>` off the
  # bottom right. Playtime already has a row above.
  entry: ''
  disabled-worlds: []"####,
    },
    Superseded {
        label: "the default whose tab-list.format was '%player%'",
        // The file as it was before `%perm_prefix%` was added to `tab-list.format`
        // and to the header and footer, which is the shipped default that could
        // never reach a server that had already run the plugin once. Those three
        // lines are the whole difference.
        text: r#"# PumpkinTAB - the tab list, header, footer, bossbar and sidebar.
#
# Placeholders: %player% %viewer% %ping% %world% %uuid% %online%
# %max_players% %server_name% %server_time% %server_date% %tps% %mspt% %group%
# Anything else goes to PumpkinPIE, which is optional. A placeholder nothing can
# answer is left on screen exactly as written, so a typo shows up.

# How often to redraw. 20 ticks is one second.
refresh-interval-ticks: 20

header-footer:
  enabled: true
  header:
    - '%animation:logo%'
    - '&7Welcome, &f%player%'
  footer:
    - '&7Online: &f%online%&7/&f%max_players%   &7Ping: %ping%ms   &7%player%'
  disabled-worlds: []

tab-list:
  enabled: true
  # %perm_prefix% comes from PiePerm via PumpkinPIE. Use it here if you run
  # both, or just put the colours in this line.
  format: '%player%'
  disabled-worlds: []

# Highest first. This is the sort order, and it is always on. `default` is the
# bottom of the ladder and belongs last. A player is shown as the first group
# here that they hold, which PiePerm decides by granting `PiePerm:group.<name>`.
groups:
  - owner
  - admin
  - moderator
  - vip
  - default

# Lets an operator count as a group without anyone running `/pieperm set` on them.
# Delete the whole section and op means nothing here.
op-level-groups:
  four: owner
  two: admin

# `%animation:<name>%`. change-interval is in milliseconds and a frame can itself
# be a template.
#
# It has to equal refresh-interval-ticks, or the client skips frames: at 20 ticks
# a 100 ms animation of 14 frames draws 7 of them, out of order. Faster animation
# means a lower refresh-interval-ticks, which broadcasts more.
animations:
  logo:
    change-interval: 1000
    texts:
      - '<#ff5555>-<#ffaa00>----------'
      - '--<#ffaa00>-<#ffff55>---------'
      - '----<#ffff55>-<#55ff55>--------'
      - '------<#55ff55>-<#55ffff>-----'
      - '--------<#55ffff>-<#5555ff>---'
      - '----------<#5555ff>-<#ff55ff>-'
      - '------------<#ff55ff>-<#ff5555>'

  clock:
    change-interval: 1000
    texts:
      - '&7Time &f%server_time%'

bossbar:
  enabled: false
  title: '&b%server_name% &8| &7%online% online'
  color: 'PINK'
  # Any health 0.0 to 1.0. Ignored by a bar with no divisions.
  health: 1.0
  # `no-division`, `notches6`, `notches10`, `notches12`, `notches20`.
  division: 'no-division'
  disabled-worlds: []

# `<text>||<number format>||<score>`, split on `||`, any part optional. The list
# order is the display order. `''` is a blank row. A line whose placeholders all
# come back empty is hidden. A client draws at most 15.
#
# The sidebar is one set of rows for everybody, so %player%, %ping% and %group%
# have nobody to resolve against here. `entry` below is the per-player one.
scoreboard:
  enabled: true
  title: '<#55ffff>%server_name%'
  lines:
    - '&8&m                             '
    - '&eOnline&7:||%online%&7/&4%max_players%'
    - '&eTPS&7:||%tps%'
    - '&eMSPT&7:||%mspt%'
    - '&eTime&7:||%server_time%'
    - '&8&m                             '
  # The right-hand value of each *player* row. `integer` or `hearts`.
  render-type: 'integer'
  entry: '&7%ping%ms'
  disabled-worlds: []
"#,
    },
    Superseded {
        label: "the default whose header was the striped bar",
        // The file as it was before the header, the footer and the sidebar were
        // rebuilt around the server name, a viewer's own ping and their
        // playtime. Without this entry the rebuild would reach nobody: an
        // install that has run this plugin once has this exact file on disk, and
        // `classify` only rewrites a default this crate recognises byte for
        // byte.
        text: r#"# PumpkinTAB - the tab list, header, footer, bossbar and sidebar.
#
# Placeholders, all of which work with no other plugin installed:
#
#   %player% %viewer% %ping% %world% %uuid% %health% %gamemode% %player_ip%
#   %bedrock% %permission_level% %group%
#   %online% %max_players% %worldonline% %server_name% %tps% %mspt%
#   %server_time% %time% %server_date% %date%
#   %animation:<name>%   one frame of an entry in `animations:` below
#   %condition:<name>%   one of the `true:` / `false:` texts of `conditions:` below
#
# Anything else - %perm_prefix% and its siblings - goes to PumpkinPIE, which is
# optional. A placeholder nothing can answer is left on screen exactly as
# written, so a typo shows up instead of turning into a blank.
#
# Written once, on the first start, and rewritten only if what is on disk is
# still a byte-for-byte copy of an older default that shipped. Edit anything -
# a value, or even a comment - and the file is yours from then on.

# How often to redraw. 20 ticks is one second.
refresh-interval-ticks: 20

header-footer:
  enabled: true
  header:
    - '<#FFFFFF>&m                                                </#FFFF00>'
    - '&3&l%server_name%'
    - '&r&7&l>> &7Welcome, &3&l%player%&7&l!'
    - ''
  footer:
    - '%animation:clock%'
    - '&r&7Online players&7: &f%online%'
  disabled-worlds: []

tab-list:
  enabled: true
  # %perm_prefix% needs both PumpkinPerm and PumpkinPIE. Without them it is left
  # on screen as written, so drop it if you are not running them.
  format: '%perm_prefix%%player%'
  disabled-worlds: []

# Highest first, and the position is the sort order. `default` belongs last and is
# the fallback. A player is shown as the first entry here that they hold, which
# PumpkinPerm decides by granting `PumpkinPerm:group.<name>`. Keep the order the
# same as theirs; nothing reads their weights.
#
# Renaming a group an install already uses would move everybody in it to
# `default` without a word.
groups:
  - owner
  - admin
  - moderator
  - vip
  - default

# Lets an operator count as a group without anyone running `/pumpkinperm set` on
# them. Delete the whole section and op means nothing here.
op-level-groups:
  four: owner
  two: admin

# `%animation:<name>%`. change-interval is in milliseconds and a frame can itself
# be a template.
#
# It has to be a whole multiple of the refresh, or the client skips frames: a
# frame is picked once per refresh and never in between, so at 20 ticks a 100 ms
# animation of 28 frames draws 14 of them, out of order. Slower is fine; faster
# means a lower refresh-interval-ticks, which broadcasts more.
animations:
  logo:
    change-interval: 1000
    texts:
      - '&d-&3--------------'
      - '&3-&d-&3-------------'
      - '&3--&d-&3------------'
      - '&3---&d-&3-----------'
      - '&3----&d-&3----------'
      - '&3-----&d-&3---------'
      - '&3------&d-&3--------'
      - '&3-------&d-&3-------'
      - '&3--------&d-&3------'
      - '&3---------&d-&3-----'
      - '&3----------&d-&3----'
      - '&3-----------&d-&3---'
      - '&3------------&d-&3--'
      - '&3-------------&d-&3-'
      - '&3--------------&d-'
      - '&3-------------&d-&3-'
      - '&3------------&d-&3--'
      - '&3-----------&d-&3---'
      - '&3----------&d-&3----'
      - '&3---------&d-&3-----'
      - '&3--------&d-&3------'
      - '&3-------&d-&3-------'
      - '&3------&d-&3--------'
      - '&3-----&d-&3---------'
      - '&3----&d-&3----------'
      - '&3---&d-&3-----------'
      - '&3--&d-&3------------'
      - '&3-&d-&3-------------'

  clock:
    change-interval: 3000
    texts:
      - '&7Time &b%server_time%'
      - '&7Date &b%server_date%'

  # `bossbar.color` points at this, which is why a colour can be a template.
  barcolors:
    change-interval: 1000
    texts:
      - 'RED'
      - 'BLUE'
      - 'GREEN'
      - 'YELLOW'

# A named test, used as `%condition:<name>%`. Same shape as Java TAB's, so a
# block written for it loads here unchanged. Shipped empty, so nothing changes
# behaviour until you add one.
#
# `conditions:` is a list of expressions; two or more also need `type: AND` (the
# default) or `type: OR`. `true:` and `false:` are templates themselves and
# default to the literal words, which is what lets one condition be tested
# inside another's expression. Comparators: >= > <= <  = !=  <- !<-  |- !|-  -|
# !-| , plus `permission:<node>` and `!permission:<node>`; a side that is not a
# number counts as 0. A condition sees the placeholders above and nothing else,
# so a test on `%perm_group%` compares the literal text `%perm_group%`.
#
# conditions:
#   low_ping:
#     conditions:
#       - '%ping%<50'
#     true: '&a%ping%ms'
#     false: '&c%ping%ms'
conditions: {}

bossbar:
  # Off by default, as in TAB. One bar everybody sees: the title is one string.
  enabled: false
  title: '&b%server_name% &8| &7%online% online'
  # pink blue red green yellow purple white, or a template.
  color: '%animation:barcolors%'
  # 0.0 to 1.0. `no-division`, `notches6`, `notches10`, `notches12`, `notches20`.
  health: 1.0
  division: 'no-division'
  disabled-worlds: []

# `<text>||<number format>||<score>`, split on `||`, any part optional. The list
# order is the display order, `''` is a blank row, a line whose placeholders all
# come back empty is hidden, and a client draws at most 15.
#
# The sidebar is one set of rows for everybody, so %player%, %ping% and %group%
# have nobody to resolve against here. `entry` below is the per-player one.
scoreboard:
  enabled: true
  title: '<#E0B11E>%server_name%'
  lines:
    - '&7%server_date%'
    - '%animation:logo%'
    - ''
    - '&6Online:'
    - '* &eOnline&7:||%online%&7/&f%max_players%'
    - '* &eTPS&7:||%tps%'
    - '* &eMSPT&7:||%mspt%'
    - ''
    - '&6Server info:'
    - '* &bTime&7:||%server_time%'
  # `integer` or `hearts`.
  render-type: 'integer'
  # The right-hand value of each *player* row.
  #
  # The host reports no ping for a Bedrock connection at all, so `%ping%` is left
  # exactly as written there rather than shown as a number. Put `%group%` here
  # instead if you would rather see something.
  entry: '&7%ping%ms'
  disabled-worlds: []

# From TAB's defaults and deliberately not shipped: `%staffonline%` and
# `%memory-used%` (no placeholder here answers them), animations faster than the
# refresh, and `layout:` (needs a per-viewer tab-list name this host lacks).
"#,
    },
];

/// What an on-disk `config.yml` is, relative to what this build ships.
///
/// The point of the enum is that there is no fourth case. Anything that is not
/// an exact copy of a default this crate wrote out is the admin's file, and the
/// only thing to do with it is nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDisk {
    /// The default this build ships, byte for byte. Nothing to do.
    Current,
    /// An older default this crate shipped, byte for byte, so nobody has edited
    /// it and replacing it cannot lose anything.
    Superseded(&'static Superseded),
    /// Anything else, which is the admin's own file. Never written to.
    Edited,
}

/// Classifies the bytes read from disk.
///
/// This function is the whole safety argument for rewriting a config, so it is
/// the narrowest thing that could work: an exact comparison against a default
/// some earlier build of this crate wrote out unchanged. Not "parses", not "same
/// keys", not "same values" - a comment an admin edited, one space, one
/// trailing newline, and the file is theirs. `classify(DEFAULT_CONFIG)` is
/// [`OnDisk::Current`], which is what a file this build wrote itself comes back
/// as, and nothing else may be rewritten.
///
/// The one liberty taken is line endings, and only line endings. See
/// [`same_document`].
pub fn classify(on_disk: &str) -> OnDisk {
    // The exact comparison is checked first and is not redundant. [`same_document`]
    // normalises only its right-hand side, so it is **not reflexive**:
    // `same_document(x, x)` is false for any `x` carrying CRLF, because one side
    // keeps the `\r` and the other has it stripped - and `include_str!` hands
    // back `config.yml` exactly as checked out. A CRLF default therefore failed to
    // match itself and was reported as `Edited`. Being byte-identical to the
    // shipped default *is* untouched, so this cannot rewrite an admin's file.
    //
    // The reference must also be the **right**-hand argument. `same_document`
    // normalises only that side, because it is the crate-owned default whose line
    // endings are a property of the checkout, while the left side is the admin's
    // file and CRLF there means an editor saved it. Passed the other way round, an
    // admin's CRLF file was normalised and matched a superseded default - so the
    // one file this module must never rewrite was the one it rewrote.
    if on_disk == DEFAULT_CONFIG || same_document(on_disk, DEFAULT_CONFIG) {
        return OnDisk::Current;
    }
    SUPERSEDED_CONFIGS
        .iter()
        .find(|old| same_document(on_disk, old.text))
        .map_or(OnDisk::Edited, OnDisk::Superseded)
}

/// Whether two files hold the same document, ignoring line endings.
///
/// **This is the only relaxation in the rule, and it is needed.** A Windows text
/// editor rewrites a whole file with CRLF when the admin saves it without
/// touching a character, this crate's own sources disagree with each other about
/// which it writes, and `include_str!` hands back whatever `config.yml` happens
/// to be checked out as. A raw byte comparison therefore calls the same untouched
/// file "shipped" on one machine and "the admin's own" on the next, and a
/// feature that fires on some checkouts and not others is worse than none.
///
/// Every other byte still has to match, character for character, so this cannot
/// turn a file holding an admin's edit into one to be overwritten. Split on the
/// newline rather than using [`str::lines`] so a missing trailing newline is
/// still a difference.
///
/// **Only the right-hand side is normalised, and that asymmetry is the point.**
/// The right side is always a crate-owned default, whose line endings are a
/// property of the checkout; the left side is the admin's file, and CRLF there
/// means a Windows editor saved it, which is exactly the "somebody has touched
/// this" signal that must stop a rewrite. Normalising both sides would make that
/// file match a default and get overwritten.
/// `line_endings_alone_do_not_make_a_file_the_admin_s` holds all three halves.
fn same_document(a: &str, b: &str) -> bool {
    a.split('\n')
        .eq(b.split('\n').map(|line| line.trim_end_matches('\r')))
}

/// The default refresh, in ticks. 20 ticks is one second.
pub const DEFAULT_REFRESH_TICKS: u64 = 20;

fn default_refresh_ticks() -> u64 {
    DEFAULT_REFRESH_TICKS
}

/// The lowest a refresh interval may be configured to.
///
/// One tick is already faster than any client can usefully redraw, and the host
/// broadcasts every setter to the whole world, so a smaller value only spends
/// bandwidth.
pub const MIN_REFRESH_TICKS: u64 = 1;

/// How many `scoreboard.lines` a client will actually draw.
pub const VISIBLE_LINES: usize = 15;

/// The group everybody is in when no group above matches.
pub const DEFAULT_GROUP: &str = "default";

/// The plugin whose group nodes identify a group here. PumpkinPerm's metadata
/// name, which is also its permission namespace.
pub const PERM_PLUGIN: &str = "PumpkinPerm";

/// How many of an animation's frames a refresh interval will ever show.
///
/// An animation is sampled once per refresh, so the frame on screen at refresh
/// `k` is `(k * refresh / change_interval) % frames`. That sequence advances by a
/// fixed step each refresh and wraps, so the number of distinct frames it can
/// reach is `frames / gcd(frames, step)` - and when the step shares a factor with
/// the frame count, some frames are never shown at all.
///
/// This is the whole reason a scrolling logo looks like a stuttering bar: at the
/// default 1000 ms refresh, a 100 ms animation of 14 frames advances 10 frames per
/// refresh, so only 7 of the 14 are ever drawn, in a scrambled order.
///
/// Returns [`None`] when every frame is reachable.
fn aliased_frames(change_ms: u64, frames: usize, refresh_ticks: u64) -> Option<usize> {
    let refresh_ms = refresh_ticks * 50;
    if refresh_ms == 0 || change_ms == 0 || frames < 2 {
        return None;
    }
    let gcd = |a: u64, b: u64| -> u64 {
        let (mut a, mut b) = (a, b);
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        a.max(1)
    };
    let step = refresh_ms / gcd(refresh_ms, change_ms);
    let reachable = frames / gcd(frames as u64, step) as usize;
    (reachable < frames).then_some(reachable)
}

/// How many `scoreboard.lines` can be given a distinct row.
///
/// A line's entry name encodes its position in two digits, so the 101st line would
/// reuse the first one's key and overwrite it. TAB throws at the same point.
pub const MAX_LINES: usize = 99;

/// The whole configuration.
///
/// `Default` is written out by hand rather than derived: the derived one leaves
/// `refresh_interval_ticks` at `0`, and that value is the fallback for a
/// `config.yml` which failed to parse.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// How often the tab list is recomputed and pushed.
    ///
    /// `serde`'s `#[serde(default)]` would use `0` for an absent key, and `0` is
    /// clamped up to [`MIN_REFRESH_TICKS`] rather than treated as "unset" - so a
    /// config that simply omits the key would run at twenty refreshes a second
    /// instead of one, and report the clamp as a problem the admin never asked
    /// about. The default is [`DEFAULT_REFRESH_TICKS`] instead.
    #[serde(default = "default_refresh_ticks")]
    pub refresh_interval_ticks: u64,

    /// Accepted and ignored. The clock is gone; this remains so a `config.yml`
    /// written while it existed still loads.
    ///
    /// There was no way to keep the clock honest on this WIT. The plugin API
    /// exposes no time call at all (`server.wit` has `get-motd` and `get-tps`),
    /// `wasi:clocks/timezone` is marked `@unstable` in the wasmtime revision
    /// Pumpkin pins and the host does not implement it anyway, and the only clock
    /// a WASI guest has reads UTC. So the offset had to be typed in by hand, and
    /// every value of it was a guess the admin had to keep current across daylight
    /// saving. A wrong guess reads as a broken plugin rather than a wrong setting,
    /// so the feature went rather than ship that.
    ///
    /// Serde still accepts the key because removing it would make every config
    /// written for an earlier build fail to load - `deny_unknown_fields` means an
    /// unknown key is an error, and these configs exist.
    #[serde(default)]
    pub utc_offset_hours: i32,

    /// The header and footer above and below the list.
    pub header_footer: HeaderFooter,

    /// Each player's own entry in the list.
    pub tab_list: TabList,

    /// The group names, **highest first**, exactly as LuckPerms and TAB write
    /// them. `default` belongs last and is the fallback.
    ///
    /// The position *is* the sort order, so there is no `order` number to keep in
    /// step with anything. A player is shown as the first entry here that they
    /// qualify for.
    pub groups: Vec<String>,

    /// Op levels that count as a group without anybody being assigned.
    ///
    /// Keyed by op level, valued by a group name that must also appear in
    /// [`Self::groups`]. This is how an operator shows up as an admin on a server
    /// where op is how power is actually handed out.
    pub op_level_groups: BTreeMap<MinOpLevel, String>,

    /// Animated text, by name, referenced as `%animation:<name>%`.
    pub animations: BTreeMap<String, Animation>,

    /// Named tests, referenced as `%condition:<name>%`.
    ///
    /// The same shape Java TAB ships, so a `conditions:` block written for it
    /// loads here. Absent is not the same as empty: an empty map is a server that
    /// deliberately has no conditions, and `problems()` says so about any that are
    /// referenced but not defined.
    pub conditions: crate::render::Conditions,

    /// The shared bossbar.
    pub bossbar: BossBar,

    /// The sidebar.
    pub scoreboard: Scoreboard,
}

/// Written out rather than derived, because `derive(Default)` would leave
/// `refresh_interval_ticks` at `0` and `Config::default()` is the fallback for a
/// `config.yml` that failed to parse. Every other field is spelled out with the
/// same value its own `Default` produces, so nothing here is a second opinion.
impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval_ticks: DEFAULT_REFRESH_TICKS,
            utc_offset_hours: 0,
            header_footer: HeaderFooter::default(),
            tab_list: TabList::default(),
            groups: Vec::new(),
            op_level_groups: BTreeMap::new(),
            animations: BTreeMap::new(),
            conditions: crate::render::Conditions::new(),
            bossbar: BossBar::default(),
            scoreboard: Scoreboard::default(),
        }
    }
}

impl Config {
    /// Parses a `config.yml`.
    ///
    /// # Errors
    /// Returns the parser's message. For [`deny_unknown_fields`] it names the
    /// offending key, which is the point: a mistyped key that parsed into nothing
    /// would be a setting that silently does not apply.
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_norway::from_str(text).map_err(|error| error.to_string())
    }

    /// Renders back to YAML.
    ///
    /// # Errors
    /// Only if serialisation fails, which for a tree of strings and integers it
    /// does not.
    pub fn to_yaml(&self) -> Result<String, String> {
        serde_norway::to_string(self).map_err(|error| error.to_string())
    }

    /// The refresh interval, clamped to something usable.
    pub fn refresh_ticks(&self) -> u64 {
        self.refresh_interval_ticks.max(MIN_REFRESH_TICKS)
    }

    /// Every user-visible template, as `(where in the config, the text)`.
    ///
    /// One list so [`Self::problems`] can check them all the same way. A template
    /// added to the config and forgotten here is one whose typos are never
    /// reported, so this deliberately spans every section rather than the one that
    /// seemed worth checking.
    ///
    /// The location is owned because an animation's name is only known at runtime.
    pub fn templates(&self) -> Vec<(String, &str)> {
        fn block<'a>(out: &mut Vec<(String, &'a str)>, where_: &str, lines: &'a [String]) {
            for line in lines {
                out.push((where_.to_string(), line.as_str()));
            }
        }

        let mut out: Vec<(String, &str)> = Vec::new();
        block(&mut out, "header", &self.header_footer.header);
        block(&mut out, "footer", &self.header_footer.footer);
        out.push(("tab-list.format".to_string(), &self.tab_list.format));

        for (name, animation) in &self.animations {
            block(&mut out, &format!("animations.{name}"), &animation.texts);
        }

        if let Some(entry) = &self.scoreboard.entry {
            out.push(("scoreboard.entry".to_string(), entry.as_str()));
        }
        block(&mut out, "scoreboard.lines", &self.scoreboard.lines);
        if !self.scoreboard.title.is_empty() {
            out.push(("scoreboard.title".to_string(), &self.scoreboard.title));
        }
        if self.bossbar.enabled && !self.bossbar.title.is_empty() {
            out.push(("bossbar.title".to_string(), &self.bossbar.title));
        }
        out
    }

    /// The groups a player can qualify for, highest first, in configured order.
    pub fn groups_by_order(&self) -> &[String] {
        &self.groups
    }

    /// The permission node that identifies `name`.
    ///
    /// PumpkinPerm owns group membership, and it names a group by
    /// `PumpkinPerm:group.<name>`. That string is the whole interface between the two
    /// plugins: PumpkinPerm grants the node, PumpkinTAB asks about it, and neither calls
    /// the other.
    ///
    /// Returns [`None`] for an unusable name rather than a node nobody could ever
    /// be granted, which would fail silently.
    pub fn node_for(&self, name: &str) -> Option<String> {
        if crate::groups::name_problem(name).is_some() {
            return None;
        }
        Some(format!("{}:group.{name}", crate::config::PERM_PLUGIN))
    }

    /// The sort value for the group at `index`.
    ///
    /// Descending from the number of groups, so the first entry is the largest
    /// value and the client sorts it to the top. Always on: a tab list in group
    /// order is the point of having groups.
    pub fn sort_value(&self, index: usize) -> i32 {
        self.groups.len().max(1) as i32 - index as i32
    }

    /// Everything wrong with the configuration.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();

        // Every template, checked for a tag that does not exist. Cheap, and it
        // catches the one mistake that is otherwise invisible: a colour spelled
        // slightly wrong is left on screen as literal `<gray8>`.
        for (where_, template) in self.templates() {
            for tag in crate::text::unknown_tags(template) {
                problems.push(format!("{where_}: '<{tag}>' is not a tag, so it shows as text"));
            }
        }

        // A condition that is referenced but not defined is the one dangling
        // reference that is invisible in game: the placeholder is left on screen
        // looking exactly like any other mistyped name, and the section that would
        // explain it may be several hundred lines further down the file.
        for (where_, template) in self.templates() {
            for name in crate::render::placeholder_names(template) {
                let Some(wanted) = name.strip_prefix(crate::render::CONDITION_PREFIX) else {
                    continue;
                };
                if !self.conditions.contains_key(wanted) {
                    problems.push(format!(
                        "{where_}: '%condition:{wanted}%' is not a condition. Add a \
                         `conditions:` entry called '{wanted}', or fix the spelling."
                    ));
                }
            }
        }

        if self.refresh_interval_ticks < MIN_REFRESH_TICKS {
            problems.push(format!(
                "refresh-interval-ticks is {}, raised to {MIN_REFRESH_TICKS}",
                self.refresh_interval_ticks
            ));
        }

        // A line's entry name encodes its position in two digits, so past 99 the keys
        // repeat and two lines would collapse into one row. TAB refuses there too.
        if self.scoreboard.lines.len() > MAX_LINES {
            problems.push(format!(
                "scoreboard.lines has {} entries; at most {MAX_LINES} can be given distinct rows",
                self.scoreboard.lines.len()
            ));
        }

        // A client shows 15 and drops the rest, so a longer list is a mistake worth
        // naming rather than a limit to discover in game.
        if self.scoreboard.lines.len() > VISIBLE_LINES {
            problems.push(format!(
                "scoreboard.lines has {} entries, but a client only shows the top {VISIBLE_LINES}; \
                 the rest will never appear",
                self.scoreboard.lines.len()
            ));
        }

        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut defaults = 0;
        for name in &self.groups {
            if !seen.insert(name.as_str()) {
                problems.push(format!("'{name}' is listed in `groups` twice"));
            }
            if let Some(problem) = crate::groups::name_problem(name) {
                problems.push(format!("group '{name}': {problem}"));
            }
            if name.eq_ignore_ascii_case(DEFAULT_GROUP) {
                defaults += 1;
                if name != DEFAULT_GROUP {
                    problems.push(format!(
                        "'{name}' should be written '{DEFAULT_GROUP}': that is the name every \
                         permission plugin uses for the bottom of the ladder"
                    ));
                }
            }
        }
        if defaults == 0 {
            problems.push(format!(
                "`groups` has no '{DEFAULT_GROUP}', so a player in no group has nothing to fall \
                 back to"
            ));
        }

        for (level, name) in &self.op_level_groups {
            if !self.groups.iter().any(|listed| listed == name) {
                problems.push(format!(
                    "op-level-groups maps op level {} to '{name}', which is not in `groups`, \
                     so nobody would ever qualify through it",
                    level.as_str()
                ));
            }
        }

        for (name, animation) in &self.animations {
            if animation.texts.is_empty() {
                problems.push(format!(
                    "animation '{name}' has no `texts`, so `%animation:{name}%` will render as \
                     nothing"
                ));
            }
            if animation.change_interval == 0 {
                problems.push(format!(
                    "animation '{name}' has change-interval 0, so it cannot advance; 1ms is the \
                     floor"
                ));
            } else if let Some(seen) = aliased_frames(
                animation.change_interval,
                animation.texts.len(),
                self.refresh_ticks(),
            ) {
                problems.push(format!(
                    "animation '{name}' changes every {}ms but the refresh is {}ms, so only {seen} \
                     of its {} frame(s) are ever drawn; make change-interval equal the refresh, or \
                     lower refresh-interval-ticks",
                    animation.change_interval,
                    self.refresh_ticks() * 50,
                    animation.texts.len()
                ));
            }
        }

        if let Some(problem) = self.bossbar.problem() {
            problems.push(format!("bossbar: {problem}"));
        }
        if self.scoreboard.enabled
            && !matches!(self.scoreboard.render_type.as_str(), "integer" | "hearts")
        {
            problems.push(format!(
                "scoreboard render-type is '{}'; expected integer or hearts",
                self.scoreboard.render_type
            ));
        }

        problems
    }
}

/// The header above and the footer below the list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct HeaderFooter {
    /// Whether to push a header and footer at all.
    pub enabled: bool,
    /// The header, one entry per line.
    pub header: Vec<String>,
    /// The footer, one entry per line.
    pub footer: Vec<String>,
    /// Worlds to leave alone. Empty means every world.
    pub disabled_worlds: Vec<String>,
}

impl Default for HeaderFooter {
    fn default() -> Self {
        Self {
            enabled: true,
            header: Vec::new(),
            footer: Vec::new(),
            disabled_worlds: Vec::new(),
        }
    }
}

impl HeaderFooter {
    /// Whether `world` should get a header and footer.
    pub fn applies_in(&self, world: &str) -> bool {
        self.enabled
            && !self
                .disabled_worlds
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(world))
    }
}

/// Placeholders that are removed from the rendered output when nothing answers
/// them, rather than left on screen as written.
///
/// The default is `["perm_prefix"]`, and that default is the whole reason the
/// key exists. `%perm_prefix%` needs PumpkinPerm and PumpkinPIE, and an optional
/// plugin that is not installed is not a typo - so a server running neither would
/// otherwise show a literal `%perm_prefix%` in front of every name. Every other
/// placeholder is still left as written, so a typo stays visible.
///
/// Anything listed here is dropped from the tab-list entry, the header and the
/// footer. An entry is a placeholder name, and the `%` signs are optional: both
/// `perm_prefix` and `%perm_prefix%` name the same thing, because an admin
/// copying a placeholder out of `format:` writes the wrapped one.
pub const DEFAULT_AUTOHIDE: [&str; 1] = ["perm_prefix"];

/// Each player's own entry in the list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct TabList {
    /// Whether to set anyone's tab list name.
    pub enabled: bool,
    /// The template for it.
    pub format: String,
    /// Placeholders removed when nothing answers them. See [`DEFAULT_AUTOHIDE`].
    #[serde(default = "default_autohide")]
    pub autohide: Vec<String>,
    /// Accepted and ignored, for a config written before `autohide` existed.
    ///
    /// `autohide` with its default already blanked `%perm_prefix%`, so this key
    /// has no separate meaning left. It is still read so an older config does not
    /// fail to load under `deny_unknown_fields`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_optional: Option<bool>,
    /// Worlds to leave alone. Empty means every world.
    pub disabled_worlds: Vec<String>,
}

fn default_autohide() -> Vec<String> {
    DEFAULT_AUTOHIDE.iter().map(|name| (*name).to_string()).collect()
}

impl Default for TabList {
    fn default() -> Self {
        Self {
            enabled: true,
            format: "%player%".to_string(),
            autohide: default_autohide(),
            prefix_optional: None,
            disabled_worlds: Vec::new(),
        }
    }
}

impl TabList {
    /// Whether `world` should get tab list names.
    pub fn applies_in(&self, world: &str) -> bool {
        self.enabled
            && !self
                .disabled_worlds
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(world))
    }
}

/// The op level a group falls back to when its node is not set.
///
/// Serialised as the vanilla names, because that is what a config should read
/// like. The host's `permission-level` enum is `zero` to `four`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MinOpLevel {
    /// Anyone, including nobody in particular.
    #[default]
    Zero,
    /// Op level one.
    One,
    /// Op level two.
    Two,
    /// Op level three.
    Three,
    /// Op level four.
    Four,
}

impl MinOpLevel {
    /// The host's enum value, without importing it here.
    ///
    /// Kept as a `u8` so [`crate::config`] stays free of the plugin API and its
    /// unit tests run on the host target. The mapping is `zero`=0 … `four`=4 in
    /// both the WIT enum and the host's `PermissionLvl`
    /// (`wasm_host/wit/v0_1/player.rs:576`); `the_metadata_name_is_the_namespace`
    /// in the sibling plugin asserts the equivalent there.
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }

    /// The name this level is written as in the config.
    ///
    /// A message that says `One` when the key is `one` sends the admin looking
    /// for something that is not there.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::One => "one",
            Self::Two => "two",
            Self::Three => "three",
            Self::Four => "four",
        }
    }
}

/// Animated text.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Animation {
    /// How long each frame lasts, in **milliseconds**.
    pub change_interval: u64,
    /// The frames, cycled in order.
    ///
    /// Each frame is itself a template, resolved after it is selected.
    pub texts: Vec<String>,
}

impl Animation {
    /// The frame interval, with zero floored to 1 ms.
    ///
    /// Zero would mean a division by zero in the frame calculation, and one
    /// millisecond is already faster than any refresh can show.
    pub fn interval_ms(&self) -> u64 {
        self.change_interval.max(1)
    }

    /// The frame to show at `elapsed_ms`, if there is one.
    ///
    /// `elapsed_ms / interval % len`, against a wall clock rather than a tick
    /// count, so an animation faster than the refresh loop skips frames instead of
    /// being slowed to match it.
    pub fn frame_at(&self, elapsed_ms: u64) -> Option<&str> {
        if self.texts.is_empty() {
            return None;
        }
        let index = (elapsed_ms / self.interval_ms()) % self.texts.len() as u64;
        self.texts.get(index as usize).map(String::as_str)
    }
}

/// The shared bossbar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct BossBar {
    /// Whether to create one at all.
    pub enabled: bool,
    /// Its title. A template.
    pub title: String,
    /// Its colour, as a template: an animation that cycles `RED`, `BLUE` is the
    /// intended use, because a frame here is a value rather than display text.
    pub color: String,
    /// Any health from 0.0 to 1.0. Only visible on a bar with divisions.
    pub health: f32,
    /// `no-division`, `notches6`, `notches10`, `notches12` or `notches20`.
    pub division: String,
    /// Worlds to leave alone. Empty means every world.
    pub disabled_worlds: Vec<String>,
}

impl Default for BossBar {
    fn default() -> Self {
        Self {
            enabled: false,
            title: String::new(),
            color: "PINK".to_string(),
            health: 1.0,
            division: "no-division".to_string(),
            disabled_worlds: Vec::new(),
        }
    }
}

impl BossBar {
    /// Whether `world` should get a bar.
    pub fn applies_in(&self, world: &str) -> bool {
        self.enabled
            && !self
                .disabled_worlds
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(world))
    }

    /// The division, or the first complaint about it.
    pub fn problem(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        if !(0.0..=1.0).contains(&self.health) {
            return Some(format!(
                "health is {}, and a bar's health is 0.0 to 1.0",
                self.health
            ));
        }
        if !matches!(
            self.division.as_str(),
            "no-division" | "notches6" | "notches10" | "notches12" | "notches20"
        ) {
            return Some(format!(
                "division is '{}'; expected no-division, notches6, notches10, notches12 or \
                 notches20",
                self.division
            ));
        }
        None
    }

    /// The colour, or the first complaint about it.
    pub fn colour_problem(&self) -> Option<String> {
        let colour = self.color.trim();
        if crate::bossbar::COLOURS.contains(&colour.to_ascii_lowercase().as_str()) {
            None
        } else {
            Some(format!(
                "colour template resolved to '{colour}', which is not a bossbar colour; expected \
                 one of {}",
                crate::bossbar::COLOURS.join(", ")
            ))
        }
    }
}

/// The sidebar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Scoreboard {
    /// Whether to create the objective at all.
    pub enabled: bool,
    /// The objective's title. A template.
    pub title: String,
    /// `integer` or `hearts`.
    pub render_type: String,
    /// Fixed lines above the player rows, highest first.
    ///
    /// Each is a template, so placeholders work. They render through the same
    /// pipeline as everything else, built-ins first and then PumpkinPIE in one
    /// batch per refresh.
    ///
    /// They are rendered **once for everybody**, because the sidebar is the world
    /// scoreboard. So `%player%`, `%ping%` and `%group%` have no viewer to
    /// resolve against and are left in the text as written. Every viewerless
    /// placeholder works: `%online%`, `%tps%`, `%world%`, and the rest.
    ///
    /// An empty line is a blank row, which is how you space a sidebar out.
    ///
    /// Rows are sorted by score, so lines sit above the players and this is the
    /// order they appear in. There is no per-line number to keep in step.
    #[serde(default)]
    pub lines: Vec<String>,
    /// The text on the right of each player row, resolved per player, or
    /// [`None`] for no number at all.
    #[serde(default)]
    pub entry: Option<String>,
    /// Worlds to leave alone. Empty means every world.
    pub disabled_worlds: Vec<String>,
}

impl Default for Scoreboard {
    fn default() -> Self {
        Self {
            enabled: false,
            title: String::new(),
            render_type: "integer".to_string(),
            lines: Vec::new(),
            entry: None,
            disabled_worlds: Vec::new(),
        }
    }
}

impl Scoreboard {
    /// Whether `world` should get the sidebar.
    pub fn applies_in(&self, world: &str) -> bool {
        self.enabled
            && !self
                .disabled_worlds
                .iter()
                .any(|disabled| disabled.eq_ignore_ascii_case(world))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_parses() {
        let config = Config::parse(DEFAULT_CONFIG).expect("the shipped config.yml is valid");
        assert!(config.header_footer.enabled);
        assert_eq!(config.refresh_ticks(), DEFAULT_REFRESH_TICKS);
        assert!(config.animations.contains_key("logo"), "the shipped config has no logo animation");
        assert!(
            config.animations.contains_key("barcolors"),
            "the shipped config has no barcolors animation, which bossbar.color points at"
        );
    }

    #[test]
    fn the_shipped_config_has_no_problems() {
        // The default that ships has to be clean, or every user starts with
        // warnings they did nothing to earn.
        let config = Config::parse(DEFAULT_CONFIG).expect("valid");
        assert_eq!(config.problems(), Vec::<String>::new());
    }

    /// What [`SUPERSEDED_CONFIGS`]'s own doc comment promises, and the only thing
    /// that notices a mangled literal.
    ///
    /// A copy that does not parse is not *wrong* in any way `classify` can see:
    /// it is a `&'static str` that simply never matches, so the feature it exists
    /// for silently stops firing for every server already running the older
    /// default. Nothing logs, nothing fails, and the symptom is "the fix I
    /// shipped did not reach me".
    #[test]
    fn every_superseded_default_still_parses() {
        for old in SUPERSEDED_CONFIGS {
            Config::parse(old.text)
                .unwrap_or_else(|error| panic!("{} no longer parses: {error}", old.label));
        }
    }

    /// The inverse of [`the_shipped_config_has_no_problems`]: the current default
    /// must not be mistaken for one of its own predecessors, or the very first
    /// load would classify the file it just wrote as "replace me".
    #[test]
    fn no_superseded_default_is_the_one_that_ships() {
        for old in SUPERSEDED_CONFIGS {
            assert_eq!(
                classify(old.text),
                OnDisk::Superseded(old),
                "{} is recognised as itself, which cannot distinguish it from the current default",
                old.label,
            );
        }
        assert_eq!(classify(DEFAULT_CONFIG), OnDisk::Current);
    }

    /// The case the list exists for, end to end: an install that has never been
    /// edited is rewritten; one that has been edited in any way is not.
    #[test]
    fn an_untouched_install_is_upgraded_and_an_edited_one_is_not() {
        for old in SUPERSEDED_CONFIGS {
            assert!(
                matches!(classify(old.text), OnDisk::Superseded(_)),
                "{} should be recognised",
                old.label,
            );
            // One edited comment is enough. This is the whole safety argument, so
            // it is checked against a real superseded file rather than a stub.
            let edited = old.text.replace("# Placeholders", "# placeholders");
            assert_ne!(edited, old.text, "the edit did not apply");
            assert_eq!(
                classify(&edited),
                OnDisk::Edited,
                "{}: an edited comment must be enough to keep the admin's file",
                old.label,
            );

            // The two calls above pass the superseded text on *both* sides, so
            // neither can tell which argument `same_document` was given first -
            // and that order is the whole safety property. The real cases are
            // asymmetric: the plugin writes `old.text` verbatim, so an untouched
            // install has whatever line endings the build shipped, while the two
            // files that must never be rewritten are the admin's.
            //
            // An admin's LF file against a CRLF default has to be recognised: it
            // is the untouched default, so it is the one that should be replaced.
            let admin_lf = old.text.replace("\r\n", "\n");
            assert!(
                matches!(classify(&admin_lf), OnDisk::Superseded(_)),
                "{}: an untouched default written with LF must still be recognised when this \
                 build ships CRLF",
                old.label,
            );

            // The reverse must NOT be recognised: a file a Windows editor saved
            // carries CRLF, and rewriting it would destroy whatever the admin did
            // to it - the one thing this module must never do. Note there is no
            // added or removed newline here: only the line endings differ.
            let admin_crlf = old.text.replace('\n', "\r\n");
            assert_eq!(
                admin_crlf.replace("\r\n", "\n"),
                old.text,
                "this must differ from the default in line endings only"
            );
            assert_eq!(
                classify(&admin_crlf),
                OnDisk::Edited,
                "{}: a Windows editor saved this, so it is the admin's file and must not be \
                 rewritten",
                old.label,
            );
        }
    }

    /// Line endings alone do not make a file the admin's - and the asymmetry
    /// here is deliberate, so it is worth stating rather than "tidying".
    ///
    /// [`same_document`] normalises the **reference** side only. That is the side
    /// that can carry CRLF for reasons nobody typed: this crate's own
    /// `config.yml` may be checked out with CRLF on Windows, while the file the
    /// plugin *wrote* into the data folder is always LF.
    ///
    /// The on-disk side is left strict on purpose. An admin's file carrying CRLF
    /// means a Windows editor saved it, which means it is the admin's file - and
    /// the one thing this module must never do is rewrite one of those.
    #[test]
    fn line_endings_alone_do_not_make_a_file_the_admin_s() {
        let lf = "# a comment\nrefresh-interval-ticks: 20\n";
        let crlf = "# a comment\r\nrefresh-interval-ticks: 20\r\n";

        // The default's line endings are a property of the checkout, so whichever
        // way they arrive it still matches an admin's LF file.
        assert!(same_document(lf, crlf));
        assert!(same_document(lf, lf));

        // The reverse is deliberately **false**, and that asymmetry is the whole
        // reason the function exists: CRLF on the *admin's* side means a Windows
        // editor saved the file, which is precisely the "somebody has touched
        // this" signal that must stop a rewrite. Asserting symmetry here would
        // defeat the guard - see `same_document`.
        assert!(!same_document(crlf, lf));

        // A difference in anything else still separates them.
        assert!(!same_document(lf, &lf.replace("20", "40")));
    }

    /// `classify` has to work whichever line endings the checkout produced,
    /// because `DEFAULT_CONFIG` is an `include_str!` of a file git may have
    /// checked out with CRLF. It does not compare the default to itself through
    /// [`same_document`] alone - that function normalises only one side, so it
    /// is not reflexive and a CRLF default failed to match itself.
    ///
    /// This is checked against the real `DEFAULT_CONFIG` rather than a stub,
    /// because the bug only appears when the shipped file itself carries CRLF.
    #[test]
    fn the_default_matches_itself_however_it_was_checked_out() {
        // Normalise first, so this test is about the conversion rather than
        // about how this particular checkout happens to have `config.yml`.
        let lf = DEFAULT_CONFIG.replace("\r\n", "\n");
        let checked_out_as_crlf = lf.replace('\n', "\r\n");
        assert_eq!(
            classify(&checked_out_as_crlf),
            OnDisk::Current,
            "a default checked out with CRLF must still be the current default, not the \
             admin's own file"
        );
        assert_eq!(classify(&lf), OnDisk::Current);

        // The guard still holds when CRLF is on the admin's side of a real
        // comparison, which is the case the rewrite path actually depends on.
        assert_eq!(
            classify(&format!("{DEFAULT_CONFIG}# mine\n")),
            OnDisk::Edited
        );
    }

    /// A config that leaves the interval out gets the documented one, not `0`.
    ///
    /// `0` is not inert here: `refresh_ticks` clamps it up to
    /// [`MIN_REFRESH_TICKS`], so an absent key silently became twenty refreshes a
    /// second, and `problems()` reported the clamp as a fault the admin never
    /// made. Both the parse path and the fallback path are checked, because they
    /// are separate defaults - `#[serde(default = ...)]` and `Config::default()`
    /// - and only fixing one leaves the other at `0`.
    #[test]
    fn an_omitted_refresh_interval_is_the_default_and_not_zero() {
        let parsed = Config::parse("groups: [default]\n").expect("valid");
        assert_eq!(parsed.refresh_interval_ticks, DEFAULT_REFRESH_TICKS);
        assert!(
            parsed.problems().is_empty(),
            "a config that simply omits the key should have nothing to report: {:?}",
            parsed.problems()
        );

        // The fallback for a config that failed to parse, which never goes
        // through serde at all.
        assert_eq!(
            Config::default().refresh_ticks(),
            DEFAULT_REFRESH_TICKS,
            "the fallback must not be twenty refreshes a second"
        );

        // An explicit `0` is still clamped and still reported, so this change
        // has not removed the check for a number an admin really did type.
        let explicit = Config::parse("groups: [default]\nrefresh-interval-ticks: 0\n")
            .expect("valid");
        assert_eq!(explicit.refresh_ticks(), MIN_REFRESH_TICKS);
        assert!(
            !explicit.problems().is_empty(),
            "a typed 0 is still the admin's mistake to hear about"
        );
    }

    #[test]
    fn the_shipped_groups_are_a_ladder() {
        let config = Config::parse(DEFAULT_CONFIG).expect("valid");
        // The list is already in order, highest first, so there is nothing to
        // sort and nothing to keep in step.
        assert_eq!(
            config.groups_by_order(),
            ["owner", "admin", "moderator", "vip", "default"]
        );
    }

/// The two plugins agree on one string, and it is the only thing between them.
///
/// PumpkinPerm grants `<its own name>:group.<name>` and PumpkinTAB asks the permission
/// manager about exactly that. Nothing connects the two at compile time, they are
/// separate builds, and the failure is invisible: a mismatch means no player ever
/// matches a group, so **everybody falls back to `default` with no error in the
/// log**. The host matches node names case-sensitively too, so a case difference
/// breaks it the same way.
///
/// This is the assertion that would have caught a rename applied to one plugin and
/// not the other. It lives in both repos for the same reason.
#[test]
fn the_permission_plugin_name_is_the_one_pumpkinperm_uses() {
    assert_eq!(PERM_PLUGIN, "PumpkinPerm");
    assert_eq!(
        Config::default().node_for("admin").as_deref(),
        Some("PumpkinPerm:group.admin")
    );
    // The namespace the host will accept, character for character.
    assert!(PERM_PLUGIN.starts_with(char::is_uppercase), "the host is case sensitive");
    assert!(!PERM_PLUGIN.contains(char::is_whitespace));
}

#[test]
fn the_shipped_groups_point_at_pumpkinperm() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    for name in config.groups_by_order() {
        assert_eq!(
            config.node_for(name).as_deref(),
            Some(format!("{}:group.{name}", PERM_PLUGIN).as_str()),
            "{name} does not resolve to the group node of {PERM_PLUGIN}"
        );
    }
}

/// The shipped config sets no `op-level-groups`, deliberately.
///
/// The key still works - it is parsed and `groups::resolve` still honours it - but
/// an operator is not given a group for free here. Op is a server-wide switch
/// rather than a per-player rank, so mapping it onto the top of this ladder means
/// everybody who can fly gets the owner tag. A server that wants it writes:
/// `op-level-groups: {4: owner, 2: admin}`.
#[test]
fn the_shipped_config_maps_no_op_level() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    assert!(
        config.op_level_groups.is_empty(),
        "the shipped config should not hand a group to an operator by default: {:?}",
        config.op_level_groups
    );
    // An operator still resolves to the fallback rather than to nothing.
    assert_eq!(config.groups_by_order().last().map(String::as_str), Some(DEFAULT_GROUP));
}

/// A group prefix would be defined twice - here and in PumpkinPerm - and the two
/// copies would eventually disagree. PumpkinTAB carries names and nothing else.
#[test]
fn a_group_carries_a_name_and_nothing_else() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    let text = serde_norway::to_string(&config.groups).expect("serialises");
    for banned in ["prefix", "suffix", "order", "permission", "min-op-level"] {
        assert!(!text.contains(banned), "a group entry carries `{banned}`");
    }
    // A bare list of strings, not a list of objects.
    assert!(!text.contains('{'), "{text}");
}

/// Sorting is not optional, so there is nothing to configure.
#[test]
fn there_is_no_sorting_section() {
    assert!(Config::parse("sorting:\n  enabled: true\n").is_err());
    assert!(Config::parse("sorting:\n  reversed: true\n").is_err());
}

#[test]
fn an_unknown_key_is_rejected_rather_than_ignored() {
    let error = Config::parse("header-footer:\n  enabld: true\n").unwrap_err();
    assert!(error.contains("enabld"), "{error}");
}

#[test]
fn the_old_group_shape_is_rejected() {
    // An upgrading admin has the previous list-of-maps in their config.yml.
    // It must be refused rather than silently read as a group named
    // something empty, so the error has to name the offending entry.
    let error = Config::parse("groups:\n  - name: vip\n    order: 1\n").unwrap_err();
    assert!(error.contains("groups[0]"), "{error}");
    assert!(error.contains("string"), "{error}");
}

#[test]
fn a_zero_refresh_is_raised_rather_than_dividing_by_nothing() {
    let config = Config::parse("refresh-interval-ticks: 0").expect("valid");
    assert_eq!(config.refresh_ticks(), MIN_REFRESH_TICKS);
    assert!(config.problems().iter().any(|problem| problem.contains("raised")));
}

#[test]
fn two_groups_with_one_name_is_reported() {
    let config = Config::parse("groups: [vip, vip]\n").expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("listed in `groups` twice")),
        "{:?}",
        config.problems()
    );
}

/// `default` is now just a listed group, so a second one under another name is a
/// mistake rather than a shadow.
#[test]
fn two_spellings_of_default_are_reported() {
    let config = Config::parse("groups: [vip, default, Default]\n").expect("valid");
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
fn an_empty_animation_is_reported() {
    let config = Config::parse("animations:\n  logo:\n    texts: []\n").expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("no `texts`")),
        "{:?}",
        config.problems()
    );
}

#[test]
fn a_zero_interval_is_reported_and_clamped() {
    let config = Config::parse(
        "animations:\n  logo:\n    change-interval: 0\n    texts: ['a']\n",
    )
    .expect("valid");
    assert_eq!(config.animations["logo"].interval_ms(), 1);
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("cannot advance")),
        "{:?}",
        config.problems()
    );
}

#[test]
fn an_unknown_render_type_is_reported() {
    let config =
        Config::parse("scoreboard:\n  enabled: true\n  render-type: pixels\n").expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("render-type")),
        "{:?}",
        config.problems()
    );
}

/// The shipped config has no `scoreboard:` section, because a sidebar line has
/// no viewer and every placeholder worth showing needs one. Per-viewer rows wait
/// on Pumpkin's v0.2 plugin API.
#[test]
fn the_shipped_config_ships_no_sidebar_and_that_is_deliberate() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    assert!(
        !config.scoreboard.enabled,
        "a shipped sidebar full of unresolvable placeholders is worse than none"
    );

    // A `Config` with no `scoreboard:` key is valid, and must not be reported as
    // a problem.
    assert!(
        config.problems().iter().all(|p| !p.contains("scoreboard")),
        "{:?}",
        config.problems()
    );

    // The rest of the shipped config is unaffected - this guards the section's
    // removal, not the sidebar itself.
    assert!(config.header_footer.enabled);
    assert!(config.tab_list.enabled);
    assert!(config.refresh_ticks() > 0);
}

/// Turning one on by hand still works and is not reported as a problem.
#[test]
fn a_sidebar_can_still_be_switched_on_by_hand() {
    let config = Config::parse(
        "groups: [default]\n\
         scoreboard:\n  enabled: true\n  title: '%server_name%'\n  lines:\n    - 'online %online%'\n",
    )
    .expect("valid");
    assert!(config.scoreboard.enabled);
    assert_eq!(config.scoreboard.lines, ["online %online%"]);
    assert!(
        config.problems().iter().all(|p| !p.contains("scoreboard")),
        "a hand-written sidebar is legitimate and must not be reported: {:?}",
        config.problems()
    );
}

/// The shipped tab format works with or without the permission plugin.
///
/// `%perm_prefix%` needs PumpkinPerm and PumpkinPIE, and an optional plugin that
/// is not installed is not a typo. `autohide` removes it when nothing answers it,
/// so a server with neither reads `blockrnc` rather than a literal
/// `%perm_prefix%`.
///
/// That is the one placeholder hidden rather than left as written. Every other
/// unresolved placeholder is still left on screen, because a typo has to be
/// visible - and the log line naming `%perm_prefix%` unanswered is what points at
/// a missing plugin rather than a spelling mistake.
#[test]
fn the_shipped_tab_format_works_with_or_without_the_permission_plugin() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    assert!(
        config.tab_list.format.contains("%player%"),
        "a tab entry that loses its name to a missing optional plugin is worse than no \
         prefix: {}",
        config.tab_list.format
    );
    // The shipped format carries the prefix, so the key is not a separate feature
    // the admin has to find.
    assert!(
        config.tab_list.format.contains("%perm_prefix%"),
        "the shipped format should carry the prefix, because it blanks when nothing \
         answers it"
    );
    // And the shipped `autohide` really does cover the prefix the format asks
    // for. Compared through the renderer rather than as a string, because the
    // config spells it `%perm_prefix%` and the built-in default is `perm_prefix`;
    // both are the same entry, and what matters is that the shipped pair works
    // together on a server with neither plugin installed.
    let format = config.tab_list.format.clone();
    let unresolved = format.clone();
    assert_eq!(
        crate::render::hide_unresolved(&unresolved, &config.tab_list.autohide),
        "%player%",
        "the shipped format and autohide must leave a bare name behind"
    );
}

/// The key defaults to on rather than off, because `serde`'s `#[serde(default)]`
/// would use `false`, and a config that never wrote it would otherwise start
/// showing the literal placeholder.
#[test]
fn a_config_written_before_autohide_still_hides_the_prefix() {
    let config = Config::parse(
        "groups: [default]\ntab-list:\n  format: '%perm_prefix%%player%'\n",
    )
    .expect("valid");
    assert_eq!(
        config.tab_list.autohide,
        ["perm_prefix"],
        "configs written before the key existed must not start showing the literal \
         placeholder"
    );
    // And the old spelling is accepted rather than refused under
    // `deny_unknown_fields`, so a config written for the earlier key still loads.
    let older = Config::parse(
        "groups: [default]\ntab-list:\n  format: '%player%'\n  prefix-optional: false\n",
    )
    .expect("valid");
    assert_eq!(older.tab_list.prefix_optional, Some(false));
}

/// At the default 1000 ms refresh a 100 ms animation of 14 frames advances 10
/// frames per refresh, so only 7 are ever drawn. That is the stuttering-bar
/// bug, and it has to be reported rather than discovered in game.
#[test]
fn a_partly_visible_animation_is_reported() {
    let frames: Vec<String> = (0..14).map(|_| "    - 'x'".to_string()).collect();
    let config = Config::parse(&format!(
        "refresh-interval-ticks: 20\nanimations:\n  logo:\n    change-interval: 100\n    texts:\n{}\n",
        frames.join("\n")
    ))
    .expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("only 7 of its 14")),
        "{:?}",
        config.problems()
    );
}

/// Five frames at 100 ms against a 1000 ms refresh walks a clean step of 10
/// modulo 5, which is zero, so the client sees frame 0 forever.
#[test]
fn an_animation_that_never_moves_is_reported() {
    let frames: Vec<String> = (0..5).map(|_| "    - 'x'".to_string()).collect();
    let config = Config::parse(&format!(
        "refresh-interval-ticks: 20\nanimations:\n  logo:\n    change-interval: 100\n    texts:\n{}\n",
        frames.join("\n")
    ))
    .expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("only 1 of its 5")),
        "{:?}",
        config.problems()
    );
}

/// One frame per refresh is the only setting that always shows every frame.
#[test]
fn one_frame_per_refresh_shows_everything() {
    assert_eq!(aliased_frames(1000, 7, 20), None);
    assert_eq!(aliased_frames(1000, 14, 20), None);
    assert_eq!(aliased_frames(100, 7, 2), None);
}

/// The shipped logo is one frame per refresh, so it steps smoothly.
#[test]
fn the_shipped_animations_do_not_skip_frames() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    for (name, animation) in &config.animations {
        assert_eq!(
            aliased_frames(
                animation.change_interval,
                animation.texts.len(),
                config.refresh_ticks()
            ),
            None,
            "animation '{name}' skips frames"
        );
    }
}

#[test]
fn frames_advance_on_their_own_clock() {
    let animation = Animation {
        change_interval: 100,
        texts: vec!["a".into(), "b".into(), "c".into()],
    };
    assert_eq!(animation.frame_at(0), Some("a"));
    assert_eq!(animation.frame_at(99), Some("a"));
    assert_eq!(animation.frame_at(100), Some("b"));
    assert_eq!(animation.frame_at(299), Some("c"));
    assert_eq!(animation.frame_at(300), Some("a"));
}

/// An animation faster than the refresh loop skips frames rather than being
/// slowed to match, which is why the clock is wall time and not ticks.
#[test]
fn a_fast_animation_still_advances() {
    let animation = Animation {
        change_interval: 1,
        texts: vec!["a".into(), "b".into()],
    };
    assert_eq!(animation.frame_at(0), Some("a"));
    assert_eq!(animation.frame_at(1), Some("b"));
    assert_eq!(animation.frame_at(20), Some("a"));
}

#[test]
fn an_animation_with_no_frames_has_no_frame() {
    let animation = Animation {
        change_interval: 100,
        texts: Vec::new(),
    };
    assert_eq!(animation.frame_at(0), None);
    assert_eq!(animation.frame_at(12345), None);
}

#[test]
fn world_gates_match_without_regard_to_case() {
    let header = HeaderFooter {
        disabled_worlds: vec!["Lobby".to_string()],
        ..HeaderFooter::default()
    };
    assert!(!header.applies_in("lobby"));
    assert!(!header.applies_in("LOBBY"));
    assert!(header.applies_in("world"));
}

#[test]
fn a_disabled_section_applies_nowhere() {
    let header = HeaderFooter {
        enabled: false,
        ..HeaderFooter::default()
    };
    assert!(!header.applies_in("world"));
}

#[test]
fn a_bossbar_health_outside_the_range_is_reported() {
    let bossbar = BossBar {
        enabled: true,
        health: 1.5,
        ..BossBar::default()
    };
    assert!(bossbar.problem().unwrap().contains("0.0 to 1.0"));
}

#[test]
fn an_unknown_division_is_reported() {
    let bossbar = BossBar {
        enabled: true,
        division: "notches7".to_string(),
        ..BossBar::default()
    };
    assert!(bossbar.problem().unwrap().contains("notches7"));
}

#[test]
fn a_disabled_bossbar_is_never_complained_about() {
    let bossbar = BossBar {
        enabled: false,
        health: 99.0,
        division: "nonsense".to_string(),
        ..BossBar::default()
    };
    assert_eq!(bossbar.problem(), None);
}

#[test]
fn a_colour_template_resolving_to_a_colour_is_accepted() {
    // This is the `%animation:barcolors%` case: the frame is a bare word.
    let bossbar = BossBar {
        enabled: true,
        color: "red".to_string(),
        ..BossBar::default()
    };
    assert_eq!(bossbar.colour_problem(), None);
}

#[test]
fn a_colour_template_resolving_to_display_text_is_reported() {
    let bossbar = BossBar {
        enabled: true,
        color: "&cRed".to_string(),
        ..BossBar::default()
    };
    assert!(bossbar.colour_problem().unwrap().contains("not a bossbar colour"));
}

#[test]
fn op_levels_use_kebab_case_keys() {
    let config = Config::parse("groups: [a]\nop-level-groups:\n  two: a\n").expect("valid");
    let level = config.op_level_groups.keys().next().expect("one mapping");
    assert_eq!(*level, MinOpLevel::Two);
    assert_eq!(level.as_u8(), 2);
    assert!(config.to_yaml().unwrap().contains("two: a"));
}

/// A warning that says `One` when the key is `one` sends the admin looking for
/// a word that is not in the file.
#[test]
fn an_op_level_names_itself_the_way_the_config_writes_it() {
    assert_eq!(MinOpLevel::Zero.as_str(), "zero");
    assert_eq!(MinOpLevel::One.as_str(), "one");
    assert_eq!(MinOpLevel::Two.as_str(), "two");
    assert_eq!(MinOpLevel::Three.as_str(), "three");
    assert_eq!(MinOpLevel::Four.as_str(), "four");
}

#[test]
fn an_unlisted_op_level_group_is_named_the_way_the_config_writes_it() {
    let config = Config::parse("groups: [vip]\nop-level-groups:\n  one: ghost\n").expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("op level one to 'ghost'")),
        "{:?}",
        config.problems()
    );
}

/// A colour spelled slightly wrong renders as literal text, which on a
/// sidebar looks like a styling choice rather than a mistake.
/// Past 99 the two-digit entry name repeats, so two lines would share a key.
#[test]
fn more_lines_than_can_be_given_a_row_is_reported() {
    let lines: Vec<String> = (0..=MAX_LINES).map(|_| "'x'".to_string()).collect();
    let config = Config::parse(&format!("scoreboard:\n  lines:\n{}", indent(&lines))).expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains(&format!("at most {MAX_LINES}"))),
        "{:?}",
        config.problems()
    );
}

/// A client draws 15 and drops the rest without saying so.
#[test]
fn more_lines_than_a_client_shows_is_reported() {
    let lines: Vec<String> = (0..20).map(|_| "'x'".to_string()).collect();
    let config = Config::parse(&format!("scoreboard:\n  lines:\n{}", indent(&lines))).expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("only shows the top")),
        "{:?}",
        config.problems()
    );
}

fn indent(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("    - {line}\n"))
        .collect::<String>()
}

#[test]
fn a_mistyped_tag_is_reported_with_where_it_is() {
    let config = Config::parse("scoreboard:\n  lines:\n    - '<gray8>Europe'\n").expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("scoreboard.lines")
                && problem.contains("<gray8>")),
        "{:?}",
        config.problems()
    );
}

#[test]
fn the_shipped_config_has_no_bad_tag() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    let bad: Vec<String> = config
        .problems()
        .into_iter()
        .filter(|problem| problem.contains("is not a tag"))
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// A template added to the config and left out of `templates()` would have its
/// typos go unreported, which is the whole failure this list prevents.
#[test]
fn every_section_that_holds_a_template_is_covered() {
    let config = Config::parse(
        "header-footer:\n  header: ['<nope1>']\n  footer: ['<nope2>']\n\
             tab-list:\n  format: '<nope3>'\n\
             bossbar:\n  enabled: true\n  title: '<nope4>'\n\
             scoreboard:\n  title: '<nope5>'\n  entry: '<nope6>'\n  lines: ['<nope7>']\n\
             animations:\n  a:\n    change-interval: 100\n    texts: ['<nope8>']\n",
    )
    .expect("valid");
    let reported: Vec<String> = config
        .problems()
        .into_iter()
        .filter(|problem| problem.contains("is not a tag"))
        .collect();
    for index in 1..=8 {
        assert!(
            reported.iter().any(|problem| problem.contains(&format!("<nope{index}>"))),
            "nope{index} was not reported: {reported:#?}"
        );
    }
}

/// The section is shipped, and every condition in it is referenced by the shipped
/// text. An unreferenced one is dead config, and a default that grows a new
/// condition has to grow the `%condition:` call that uses it too, so neither half
/// can drift.
#[test]
fn every_shipped_condition_is_used_and_answers_both_ways() {
    let config = Config::parse(DEFAULT_CONFIG).expect("valid");
    for (name, condition) in &config.conditions {
        let placeholder = format!("%condition:{name}%");
        assert!(
            config.templates().iter().any(|(_, template)| template.contains(&placeholder)),
            "{placeholder} is defined but nothing renders it"
        );
        assert!(
            config.problems().is_empty(),
            "the shipped {placeholder} does not resolve: {:?}",
            config.problems()
        );
        assert!(
            condition.when_true.trim() != condition.when_false.trim(),
            "{placeholder} says the same thing either way, so it is not a condition"
        );
    }
    // And it is documented where an admin will actually be looking. Matched
    // without the surrounding punctuation on purpose: this is about the name being
    // findable, not about how this file happens to typeset it, so rewording a
    // comment cannot fail a build.
    assert!(
        DEFAULT_CONFIG.contains("%condition:<name>%"),
        "the shipped file must still name the placeholder that reaches the section"
    );
}

/// TAB's own example, parsed. The engine is exercised by `render`'s tests
/// through `compile` and `evaluate`; this is the wiring - that a block written
/// the way TAB writes it reaches [`Config::conditions`] and is *not* reported
/// as a problem, which is the only way a dangling reference could hide here.
#[test]
fn tabs_own_condition_example_parses_and_is_not_reported() {
    let config = Config::parse(
        "conditions:\n  low_ping:\n    conditions:\n      - '%ping%<50'\n    true: '&a%ping%ms'\n    \
             false: '&c%ping%ms'\ntab-list:\n  format: '%condition:low_ping%'\n",
    )
    .expect("valid");
    assert!(config.conditions.contains_key("low_ping"));
    let reported: Vec<String> = config
        .problems()
        .into_iter()
        .filter(|problem| problem.contains("is not a condition"))
        .collect();
    assert!(reported.is_empty(), "{reported:#?}");
}

/// The inverse, which is the whole reason the check exists: a condition
/// referenced but never defined is left on screen looking exactly like any
/// other typo, and the section that would explain it may be far away.
#[test]
fn a_condition_that_is_referenced_but_not_defined_is_reported() {
    let config = Config::parse("tab-list:\n  format: '%condition:low_ping%'\n").expect("valid");
    assert!(
        config
            .problems()
            .iter()
            .any(|problem| problem.contains("%condition:low_ping%") && problem.contains("is not a condition")),
        "{:#?}",
        config.problems()
    );
}
}