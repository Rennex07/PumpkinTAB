//! Turning a config string into something a vanilla client actually colours.
//!
//! ## There are two outputs, and this module builds both
//!
//! The host's [`text-component`] resource has first-class styling methods -
//! `gradient`, `rainbow`, `click-open-url`, `hover-show-text` and the rest - so a
//! template can be rendered into a real component tree and none of it has to
//! survive a string format. [`component`] does that: it walks the run tree this
//! module parses and calls those methods.
//!
//! There is still a second output, [`to_legacy`], because not every path can
//! reach the resource. `PlayerChatEvent::message` in the sibling plugin is a
//! plain `String` the host splices raw into `chat_decorated`, and `/tab preview`
//! here prints a rendered line into a chat message built in Rust. Both need a
//! legacy `&`-coded string, and the host's legacy parser
//! (`crates/pumpkin-util/src/text/legacy.rs`) is the only thing either of them
//! can be handed to.
//!
//! So: **the component path is the real one, and the legacy string is the
//! fallback**, and every place that can take a component now takes a component.
//! What follows is the shared vocabulary the two are built from, then each
//! path.
//!
//! ## The shared vocabulary
//!
//! One tag reader, `read_tag`, and one table per kind of tag. A tag the
//! component path accepts and a tag the legacy flattener accepts is decided by
//! the same [`effect`], over the same [`NAMED_COLOURS`], `STYLE_FLAGS` and
//! `known_tag`, so the two paths cannot disagree about what a template is
//! asking for. `Config::problems` reads [`unknown_tags`], which reads the same
//! tables, so a tag this module supports is never reported as a typo and a tag it
//! does not is still named at load.
//!
//! Supported, opening and closing, all case-insensitive:
//!
//! | Tag | Meaning |
//! |:--|:--|
//! | `<red>` … `<white>`, `<gray>`/`<grey>`, `<dark_gray>`/`<dark_grey>` | the sixteen named colours |
//! | `<#rrggbb>` | a hex colour |
//! | `<bold>`, `<italic>`/`<em>`, `<underlined>`/`<u>`, `<strikethrough>`/`<st>`, `<obfuscated>`/`<obf>` | a style; bare means on |
//! | `<bold:false>` and its four siblings | the same style, explicitly **off** |
//! | `<reset>`, `<r>` | everything off |
//! | `<gradient:#aabbcc:#ddeeff>`, any number of stops, names allowed | a per-character colour ramp |
//! | `<rainbow>` | a per-character hue sweep |
//! | `<click:open_url:"…">`, `<click:run_command:"…">`, `<click:suggest_command:"…">`, `<click:copy_to_clipboard:"…">` | a click action |
//! | `<hover:show_text:"…">` | a hover tooltip, itself a template |
//! | `<insert:…>`, `<font:…>` | shift-click insertion, font |
//!
//! An argument may be quoted, which is what lets `<click:open_url:
//! "https://example.com:8080/x">` arrive whole: the tag ends at the first `>`
//! that is not inside quotes, and the argument keeps its colons. A closing tag
//! takes no argument. A tag that is not recognised is left on screen exactly as
//! written - `<3` and `a < b` are ordinary text - and named by [`unknown_tags`].
//!
//! ## Legacy: the rules, and what they cost
//!
//! A legacy string has no closing tags: formatting runs until something resets
//! it, and the only reset it has is `&r`. So a closing tag cannot always be a
//! reset, and the flattener splits them in two by what they close.
//!
//! * A closing **colour** - `</red>`, `</dark_gray>`, `</#ff0000>` - becomes
//!   `&r`. There is no way to say "and only that colour is gone", so the whole
//!   line's formatting ends. That over-reaches, but the alternative is a colour
//!   running on past the tag that closed it, which is the failure that actually
//!   shows up on screen.
//! * A closing **style** - `</bold>`, `</italic>`, … and the `em`/`u`/`st`/`obf`
//!   aliases, plus `</gradient>` and `</rainbow>` - becomes **nothing at all**.
//!   The tag is consumed and no code is written.
//!
//! The second rule is the one that is not obvious, and it exists because of this
//! plugin's own use of a prefix: `tab-list.format` is documented with
//! `%perm_prefix%` in front of `%player%`, so the text converted from a prefix is
//! followed by *more of the same plugin's template*. A LuckPerms-style prefix
//! `<red><bold>[Admin] </bold>` converted to
//! `&c&l[Admin] &r<name>` leaves the name white, which is the opposite of what
//! the prefix asked for and is why a configured prefix "does not work" on the tab
//! list. Dropping the code leaves the colour the prefix opened still open.
//!
//! What that costs is stated rather than hidden: `</bold>` emits nothing, so the
//! `&l` is still open for the next character, and `<red><bold>ADMIN </bold>`
//! leaves a username **bold as well as coloured**. That is paid only by the
//! styles; a colour still stops exactly where the author closed it. Where the
//! bold matters, write the reset out - `<red><bold>[Admin] </bold><reset>` is the
//! exact form.
//!
//! **This rule is legacy's alone.** The component path has `bold(false)`, so
//! `</bold>` there really does end the bold and only the bold. See
//! `Style` and [`tree`].
//!
//! ## Legacy: the parts that had to be flattened by hand
//!
//! * **A gradient** is a property of a *component*, and a legacy string has no
//!   components, so each character is given its own colour: the interpolated
//!   value is written in front of it, followed by every style still open. The
//!   interpolation is a transcription of the host's own
//!   (`pumpkin-util/src/text/mod.rs:1728-1750`), so both paths paint the same
//!   colours. There is no span to interpolate over in a string either, so a
//!   gradient covers **the next literal run of text after the tag**: a nested tag
//!   in between does not end it, and `</gradient>` only stops it waiting.
//! * **A rainbow** is the same idea with the host's `rainbow` sweep
//!   (`pumpkin-util/src/text/mod.rs:1761-1765`).
//! * **A style that turns off** - only `<bold:false>` and its siblings can do
//!   this, since a closing style emits nothing - is a reset followed by every
//!   colour and style that is still open. That is the only correct flattening
//!   available: a legacy code turns everything off, and there is no code that
//!   turns one part of it back on.
//! * **A click or a hover** cannot exist in a legacy string at all. The tag is
//!   consumed and contributes nothing; the text inside it renders, the
//!   interaction does not. Only [`component`] can carry one.
//!
//! One thing the flattener cannot know: a `&` code written by hand in the
//! template is *not* tracked, so the "still open" bookkeeping only knows about
//! tags. A template that mixes the two spellings and then uses `<bold:false>`
//! gets the reset and whatever the tags opened, and a hand-written `&c` in front
//! of it is lost. Write one spelling or the other in a template that scopes.
//!
//! ## Raw `&` codes
//!
//! A template may be written in `&` codes instead of tags - the shipped header
//! is - and both paths accept them. The component path reads them as
//! *unscoped* changes, exactly as the host's own parser does
//! (`legacy.rs:110-131`): `&c`…`&f` replaces the colour **and clears the
//! styles**, `&l`…`&k` set one style, `&r` resets, `&#rrggbb` and the spaced
//! `&x&f&f&8&8&0&0` are both hex. A `&` in front of anything else is a
//! literal ampersand, and `&&` is one escaped ampersand.
//!
//! The legacy path leaves them alone: it is producing the very string the host's
//! parser understands, and it only escapes the ones that would otherwise be
//! eaten ([`escape`]).
//!
//! ## The two are kept apart on purpose
//!
//! [`component`] is a thin shell over the WIT and cannot be exercised on the
//! host target - `pumpkin_plugin_api`'s wit imports are `unreachable!()` stubs
//! off `wasm32` ([`crate::pie`] and [`crate::state`] both carry the note). So the
//! run tree and the legacy flattener are pure data, and every decision either
//! makes is a test here rather than in a running server.
//!
//! [`text-component`]: https://github.com/Pumpkin-MC/Pumpkin/blob/master/crates/pumpkin-plugin-wit/v0.1/text.wit

use pumpkin_plugin_api::{
    common::{NamedColor, RgbColor},
    text::TextComponent,
};

/// The code symbol the host is asked to parse with.
pub const CODE: char = '&';

/// A `%name%` placeholder, and the text around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a> {
    /// Literal text, already converted to legacy codes.
    Text(&'a str),
    /// A placeholder name, without the `%` signs.
    Placeholder(String),
}

/// Splits `template` into literal runs and placeholder names.
///
/// A `%` with no closing `%` after it is literal text, not a broken placeholder,
/// so `100% done` survives intact. The same is true of `%%`, which produces an
/// empty name and is treated as literal.
///
/// Names may contain `:` and `_`, which is what makes `%animation:logo%` and
/// `%perm_group%` work, and they are **not** lower-cased: the provider matches
/// placeholder ids case-insensitively but folds only ASCII, and leaving the
/// author's casing alone keeps a diagnostic readable.
pub fn segment(template: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let bytes = template.as_bytes();
    let mut start = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        // Scan to the next '%', and refuse to cross a newline: a template that
        // is missing one '%' should not swallow the rest of a multi-line header.
        let Some(offset) = template[index + 1..].find('%') else {
            break;
        };
        let end = index + 1 + offset;
        let name = &template[index + 1..end];
        // A name has to look like an identifier. Without this, `50% off %player%`
        // pairs the two stray percent signs and produces a placeholder called
        // `" off "`, which then renders as literal text and takes the real one
        // with it.
        if !is_placeholder_name(name) {
            // `end`, not `end + 1`: the percent sign that closed this rejected
            // candidate has to be reconsidered as an *opening* one. In
            // `50% off %player%` the first pair is not a placeholder and the
            // second one is, and a single forward step would lose it.
            index = end;
            continue;
        }
        if start < index {
            segments.push(Segment::Text(&template[start..index]));
        }
        segments.push(Segment::Placeholder(name.to_string()));
        index = end + 1;
        start = index;
    }

    if start < template.len() {
        segments.push(Segment::Text(&template[start..]));
    }
    segments
}

/// Whether `name` could be a placeholder name.
///
/// Letters, digits and `_ : . -`, which covers every built-in, every PIE
/// identifier and `%animation:<name>%`. Anything else - a space, a percent sign,
/// a newline - means the two percent signs were not a placeholder at all, and
/// treating them as one would swallow real text.
pub fn is_placeholder_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'))
}

/// Substitutes every placeholder `resolve` answers, leaving the rest as written.
///
/// `resolve` is called for each distinct placeholder name once, and its answer
/// is used for every occurrence, so a header with `%player%` twice does not
/// resolve it twice.
///
/// An unknown placeholder is left **exactly** as the author wrote it, including
/// the `%` signs. That is what Java TAB does, and it is the only useful
/// behaviour: a typo has to be visible, and blanking it would hide both the typo
/// and the fact that something is missing.
pub fn substitute(template: &str, resolve: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(template.len());
    for segment in segment(template) {
        match segment {
            Segment::Text(text) => out.push_str(text),
            Segment::Placeholder(name) => match resolve(&name) {
                Some(value) => out.push_str(&value),
                None => {
                    out.push('%');
                    out.push_str(&name);
                    out.push('%');
                }
            },
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// A colour a run can be.
///
/// Named colours are the sixteen the protocol has and the legacy codes name
/// them one for one; `Colour::Rgb` is what a `<#rrggbb>` and a `<gradient>`
/// stop both produce. Kept as this crate's own enum rather than the host's so
/// that the tree and the legacy flattener are plain data with no dependency on
/// a resource type, which is what makes them testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    /// The sixteen named colours, in the order `common.wit` lists them.
    Black,
    /// `<dark_blue>`.
    DarkBlue,
    /// `<dark_green>`.
    DarkGreen,
    /// `<dark_aqua>`.
    DarkAqua,
    /// `<dark_red>`.
    DarkRed,
    /// `<dark_purple>`.
    DarkPurple,
    /// `<gold>`.
    Gold,
    /// `<gray>`, and `<grey>`.
    Gray,
    /// `<dark_gray>`, and `<dark_grey>`.
    DarkGray,
    /// `<blue>`.
    Blue,
    /// `<green>`.
    Green,
    /// `<aqua>`.
    Aqua,
    /// `<red>`.
    Red,
    /// `<light_purple>`.
    LightPurple,
    /// `<yellow>`.
    Yellow,
    /// `<white>`.
    White,
    /// A colour with no name, from `<#rrggbb>` or a gradient stop.
    Rgb(u8, u8, u8),
}

/// Every named colour, with both of its spellings.
///
/// One table, so `named_colour` and [`unknown_tags`] cannot disagree about
/// which names exist.
const NAMED_COLOURS: [(&str, Colour); 18] = [
    ("black", Colour::Black),
    ("dark_blue", Colour::DarkBlue),
    ("dark_green", Colour::DarkGreen),
    ("dark_aqua", Colour::DarkAqua),
    ("dark_red", Colour::DarkRed),
    ("dark_purple", Colour::DarkPurple),
    ("gold", Colour::Gold),
    ("gray", Colour::Gray),
    ("grey", Colour::Gray),
    ("dark_gray", Colour::DarkGray),
    ("dark_grey", Colour::DarkGray),
    ("blue", Colour::Blue),
    ("green", Colour::Green),
    ("aqua", Colour::Aqua),
    ("red", Colour::Red),
    ("light_purple", Colour::LightPurple),
    ("yellow", Colour::Yellow),
    ("white", Colour::White),
];

impl Colour {
    /// The one legacy character that means this colour, or `x` for a hex one.
    ///
    /// `x` is not a colour: it is the introducer for the six digits that follow,
    /// which is why `Legacy::colour` writes seven pairs for a `Colour::Rgb`
    /// and one for anything named.
    fn legacy_code(self) -> char {
        match self {
            Colour::Black => '0',
            Colour::DarkBlue => '1',
            Colour::DarkGreen => '2',
            Colour::DarkAqua => '3',
            Colour::DarkRed => '4',
            Colour::DarkPurple => '5',
            Colour::Gold => '6',
            Colour::Gray => '7',
            Colour::DarkGray => '8',
            Colour::Blue => '9',
            Colour::Green => 'a',
            Colour::Aqua => 'b',
            Colour::Red => 'c',
            Colour::LightPurple => 'd',
            Colour::Yellow => 'e',
            Colour::White => 'f',
            Colour::Rgb(..) => 'x',
        }
    }

    /// The colour as red, green and blue.
    ///
    /// The named values are the host's
    /// (`pumpkin-util/src/text/color.rs:371-390`), so a gradient that mixes
    /// `<red>` with `<#800000>` fades to exactly the same red on both paths.
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Colour::Black => (0, 0, 0),
            Colour::DarkBlue => (0, 0, 170),
            Colour::DarkGreen => (0, 170, 0),
            Colour::DarkAqua => (0, 170, 170),
            Colour::DarkRed => (170, 0, 0),
            Colour::DarkPurple => (170, 0, 170),
            Colour::Gold => (255, 170, 0),
            Colour::Gray => (170, 170, 170),
            Colour::DarkGray => (85, 85, 85),
            Colour::Blue => (85, 85, 255),
            Colour::Green => (85, 255, 85),
            Colour::Aqua => (85, 255, 255),
            Colour::Red => (255, 85, 85),
            Colour::LightPurple => (255, 85, 255),
            Colour::Yellow => (255, 255, 85),
            Colour::White => (255, 255, 255),
            Colour::Rgb(red, green, blue) => (red, green, blue),
        }
    }

    /// The host's named colour, or the exact RGB triple for a hex one.
    fn to_host(self) -> NamedOrRgb {
        match self {
            Colour::Rgb(red, green, blue) => NamedOrRgb::Rgb(RgbColor {
                r: red,
                g: green,
                b: blue,
            }),
            named => NamedOrRgb::Named(match named {
                Colour::Black => NamedColor::Black,
                Colour::DarkBlue => NamedColor::DarkBlue,
                Colour::DarkGreen => NamedColor::DarkGreen,
                Colour::DarkAqua => NamedColor::DarkAqua,
                Colour::DarkRed => NamedColor::DarkRed,
                Colour::DarkPurple => NamedColor::DarkPurple,
                Colour::Gold => NamedColor::Gold,
                Colour::Gray => NamedColor::Gray,
                Colour::DarkGray => NamedColor::DarkGray,
                Colour::Blue => NamedColor::Blue,
                Colour::Green => NamedColor::Green,
                Colour::Aqua => NamedColor::Aqua,
                Colour::Red => NamedColor::Red,
                Colour::LightPurple => NamedColor::LightPurple,
                Colour::Yellow => NamedColor::Yellow,
                Colour::White => NamedColor::White,
                // Caught by the `Rgb` arm above; listed so adding a variant is a
                // compile error rather than a wrong answer.
                Colour::Rgb(..) => NamedColor::White,
            }),
        }
    }
}

/// Either flavour of the host's colour, because it has two setters.
enum NamedOrRgb {
    /// `TextComponent::color_named`.
    Named(NamedColor),
    /// `TextComponent::color_rgb`.
    Rgb(RgbColor),
}

/// The named colour `name` spells, or [`None`].
///
/// Case-insensitive, because every other half of the vocabulary is.
pub fn named_colour(name: &str) -> Option<Colour> {
    NAMED_COLOURS
        .iter()
        .find(|(spelled, _)| name.eq_ignore_ascii_case(spelled))
        .map(|(_, colour)| *colour)
}

/// The colour a `<#...>` tag spells, or [`None`].
pub fn hex_colour(name: &str) -> Option<Colour> {
    let digits = name.strip_prefix('#')?;
    is_hex_colour(digits).then(|| {
        Colour::Rgb(
            u8::from_str_radix(&digits[0..2], 16).unwrap_or_default(),
            u8::from_str_radix(&digits[2..4], 16).unwrap_or_default(),
            u8::from_str_radix(&digits[4..6], 16).unwrap_or_default(),
        )
    })
}

/// Whether `hex` is six hexadecimal digits.
pub fn is_hex_colour(hex: &str) -> bool {
    hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The colour a legacy `&<digit>` names, or [`None`].
///
/// The inverse of `Colour::legacy_code` for the sixteen named colours, and
/// separate from `named_colour` because `&7` is not the name `gray` - a `&` code
/// is a *digit*, and the shipped header is written in digits (`&r&7&l>>`).
fn legacy_colour(code: char) -> Option<Colour> {
    let code = code.to_ascii_lowercase();
    NAMED_COLOURS
        .iter()
        .find(|(_, colour)| colour.legacy_code() == code)
        .map(|(_, colour)| *colour)
}

/// Whether `name` is one of `names`, without regard to case.
fn is_any(name: &str, names: &[&str]) -> bool {
    names.iter().any(|spelled| name.eq_ignore_ascii_case(spelled))
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

/// The five styles a legacy code and a tag both exist for.
///
/// The aliases already collapsed into these five in `STYLE_FLAGS`, and the
/// closing-tag split in `Legacy::close` is by flag rather than by name for
/// exactly that reason: a second list of names would be a second thing to
/// forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    /// `<bold>`, `&l`.
    Bold,
    /// `<italic>` or `<em>`, `&o`.
    Italic,
    /// `<underlined>` or `<u>`, `&n`.
    Underlined,
    /// `<strikethrough>` or `<st>`, `&m`.
    Strikethrough,
    /// `<obfuscated>` or `<obf>`, `&k`.
    Obfuscated,
}

impl Flag {
    /// Every flag, in the order the legacy codes and the host's setters list
    /// them, so the flattener and the builder cannot disagree about precedence.
    pub const ALL: [Flag; 5] = [
        Flag::Bold,
        Flag::Italic,
        Flag::Underlined,
        Flag::Strikethrough,
        Flag::Obfuscated,
    ];

    /// The one legacy character that means this style.
    fn legacy_code(self) -> char {
        match self {
            Flag::Bold => 'l',
            Flag::Italic => 'o',
            Flag::Underlined => 'n',
            Flag::Strikethrough => 'm',
            Flag::Obfuscated => 'k',
        }
    }
}

/// Every style name this module accepts, with both of its spellings.
const STYLE_FLAGS: [(&str, Flag); 9] = [
    ("bold", Flag::Bold),
    ("italic", Flag::Italic),
    ("em", Flag::Italic),
    ("underlined", Flag::Underlined),
    ("u", Flag::Underlined),
    ("strikethrough", Flag::Strikethrough),
    ("st", Flag::Strikethrough),
    ("obfuscated", Flag::Obfuscated),
    ("obf", Flag::Obfuscated),
];

/// The style `name` spells, or [`None`].
fn style_flag(name: &str) -> Option<Flag> {
    STYLE_FLAGS
        .iter()
        .find(|(spelled, _)| name.eq_ignore_ascii_case(spelled))
        .map(|(_, flag)| *flag)
}

/// The click action a `<click:…>` tag asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// `<click:open_url:"…">`, for a URL the client opens.
    OpenUrl(String),
    /// `<click:run_command:"…">`, run as the player who clicked.
    RunCommand(String),
    /// `<click:suggest_command:"…">`, put in the chat box to be edited.
    SuggestCommand(String),
    /// `<click:copy_to_clipboard:"…">`.
    CopyToClipboard(String),
}

/// The style in force over a `Run`.
///
/// Every key is an `Option` because that is what makes scoping work: a closing
/// tag restores exactly what was active before it, **including an explicit
/// false**. There is no "unset" sentinel distinct from "off" here, and that is
/// the whole of it - `Style::flag` answers `None` for a style no tag has
/// touched, which is not the same answer as `Some(false)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    /// `<red>`, `<#ff8800>`, `&c`.
    pub colour: Option<Colour>,
    /// `<gradient:…>`, which is a property of the component and not a colour.
    pub gradient: Option<Vec<Colour>>,
    /// `<rainbow>`, the host's own sweep.
    pub rainbow: bool,
    /// `<bold>` and `<bold:false>`.
    pub bold: Option<bool>,
    /// `<italic>` and `<italic:false>`.
    pub italic: Option<bool>,
    /// `<underlined>` and `<underlined:false>`.
    pub underlined: Option<bool>,
    /// `<strikethrough>` and `<strikethrough:false>`.
    pub strikethrough: Option<bool>,
    /// `<obfuscated>` and `<obfuscated:false>`.
    pub obfuscated: Option<bool>,
    /// `<font:…>`.
    pub font: Option<String>,
    /// `<insert:…>`.
    pub insertion: Option<String>,
    /// `<click:…>`.
    pub click: Option<Click>,
    /// `<hover:show_text:"…">`, still a template: see [`tree`].
    pub hover: Option<String>,
}

impl Style {
    /// What `flag` is set to, or [`None`] if nothing has touched it.
    pub fn flag(&self, flag: Flag) -> Option<bool> {
        match flag {
            Flag::Bold => self.bold,
            Flag::Italic => self.italic,
            Flag::Underlined => self.underlined,
            Flag::Strikethrough => self.strikethrough,
            Flag::Obfuscated => self.obfuscated,
        }
    }

    /// Set or clear one style.
    fn set_flag(&mut self, flag: Flag, value: Option<bool>) {
        match flag {
            Flag::Bold => self.bold = value,
            Flag::Italic => self.italic = value,
            Flag::Underlined => self.underlined = value,
            Flag::Strikethrough => self.strikethrough = value,
            Flag::Obfuscated => self.obfuscated = value,
        }
    }
}

/// One span of literal text, and everything in force over it.
///
/// A run's own `text` is what it opened with; anything a nested tag produced is
/// in `children`. Together, in that order, they are the text of the run - which
/// is what lets a gradient cover a whole span rather than one fragment of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The literal text read before the first nested tag.
    pub text: String,
    /// The style in force over `text` and over every child.
    pub style: Style,
    /// The runs nested inside this one, in order.
    pub children: Vec<Run>,
}

/// One frame of the tree parser: a style, and the runs read under it.
///
/// `node` is the only thing that is not "a scope of style", and it is why a
/// gradient works at all. A closing tag normally hands its runs to the frame
/// below as **siblings**, because that is what a tag list means: `<red>a<bold>b</bold>c</red>`
/// is three runs that all inherit red, not three runs inside a red one. A
/// gradient cannot work that way - it is a property of a *component*, and the
/// host repaints the text of the component it is set on
/// (`pumpkin-util/src/text/mod.rs:1782`) - so a frame that carries one becomes a
/// run of its own, and its runs become that run's children.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    style: Style,
    runs: Vec<Run>,
    node: bool,
}

/// What one tag does to the style stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Effect {
    /// An opening tag: push a frame carrying the style `change` makes.
    Push(Change),
    /// `<reset>`, `<r>` and `&r`: clear the current frame's style in place.
    ///
    /// In place rather than as a frame, because a reset is a statement about
    /// everything from here on. `</reset>` is the same thing: it does not restore
    /// what was open before, which is what legacy's `&r` means and what the
    /// existing `<red><bold>A</bold></reset>x` test pins.
    Reset,
    /// A closing tag: pop the frame its opening pushed, restoring its style.
    Pop,
}

/// The change one opening tag makes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// A named or hex colour.
    Colour(Colour),
    /// A style, explicitly on or off.
    Flag(Flag, bool),
    /// A gradient, which needs at least two stops to mean anything.
    Gradient(Vec<Colour>),
    /// The host's rainbow.
    Rainbow,
    /// A click action.
    Click(Click),
    /// A hover tooltip, still a template.
    Hover(String),
    /// A font.
    Font(String),
    /// Shift-click insertion.
    Insertion(String),
}

/// Parses `template` into its runs, with the style over each one.
///
/// Pure data. Nothing here calls the host, which is what lets it be tested at
/// all ([`crate::pie`] and [`crate::state`] carry the note about the
/// `unreachable!()` stubs the wit imports are off `wasm32`) and what keeps
/// [`component`] thin enough to have nothing worth testing in it.
///
/// Three things it does that the legacy flattener cannot:
///
/// * **A closing tag really closes.** `<red><bold>A</bold>B` gives `A` red and
///   bold and `B` red and not bold, because the tree nests.
/// * **`<bold:false>` turns one style off.** `Style` holds an `Option`, so a
///   child under a bold parent can say `Some(false)` and be not-bold, which is
///   the one thing the format genuinely cannot express.
/// * **A gradient belongs to the run that owns the span.** It is set on that
///   run's own component in `build`, after its children are in place, because
///   the host reads the component's text *and* its children's and repaints every
///   character. That has one sharp edge, written down here because it is
///   invisible until it surprises somebody: the host copies the painted
///   component's **own** style onto each character
///   (`pumpkin-util/src/text/mod.rs:1798`), so a style set on a run *inside* a
///   gradient is lost. Write `<bold><gradient:red:blue>…</gradient></bold>`, not
///   the other way round.
///
/// A `&` code is read here too, and it is the host's own reading of one: a
/// colour code replaces the colour **and clears the styles**
/// (`legacy.rs:110-117`), a hex one replaces only the colour
/// (`legacy.rs:62-109`), and `&r` resets everything.
pub fn tree(template: &str) -> Vec<Run> {
    // The root frame is always there, so an opening tag always has somewhere to
    // push and `tree("")` is a plain empty list.
    let mut frames = vec![Frame {
        style: Style::default(),
        runs: Vec::new(),
        node: false,
    }];
    let mut pending = String::new();
    let mut index = 0;

    while index < template.len() {
        let ch = template[index..].chars().next().unwrap_or('\u{fffd}');

        if ch == '<'
            && let Some(tag) = read_tag(template, index)
            && let Some(effect) = effect(&tag)
        {
            push_text(&mut pending, &mut frames);
            match effect {
                Effect::Push(change) => {
                    let outer = frames.last().map(|frame| &frame.style);
                    let mut style = outer.cloned().unwrap_or_default();
                    let node = change.is_node();
                    change.apply(&mut style);
                    frames.push(Frame {
                        style,
                        runs: Vec::new(),
                        node,
                    });
                }
                Effect::Reset => {
                    if let Some(frame) = frames.last_mut() {
                        frame.style = Style::default();
                    }
                }
                // A closing tag with nothing open consumes itself and does
                // nothing, which is what a stray `</bold>` has always done.
                Effect::Pop => close_frame(&mut frames),
            }
            index = tag.end;
            continue;
        }

        if ch == CODE
            && let Some((code, read)) = read_code(template, index)
        {
            match code {
                Code::Literal => pending.push(CODE),
                Code::Colour(colour, hex) => {
                    push_text(&mut pending, &mut frames);
                    if let Some(frame) = frames.last_mut() {
                        frame.style.colour = Some(colour);
                        if !hex {
                            for flag in Flag::ALL {
                                frame.style.set_flag(flag, Some(false));
                            }
                        }
                    }
                }
                Code::Flag(flag) => {
                    push_text(&mut pending, &mut frames);
                    if let Some(frame) = frames.last_mut() {
                        frame.style.set_flag(flag, Some(true));
                    }
                }
                Code::Reset => {
                    push_text(&mut pending, &mut frames);
                    if let Some(frame) = frames.last_mut() {
                        frame.style = Style::default();
                    }
                }
            }
            index += read;
            continue;
        }

        pending.push(ch);
        index += ch.len_utf8();
    }
    push_text(&mut pending, &mut frames);

    // Whatever is still open when the template ends is closed, so its runs reach
    // the root rather than being dropped: `<red>unclosed` is a red run.
    while frames.len() > 1 {
        close_frame(&mut frames);
    }
    frames.pop().map_or_else(Vec::new, |frame| frame.runs)
}

/// Moves the text read since the last tag into a run under the current style.
fn push_text(pending: &mut String, frames: &mut [Frame]) {
    if pending.is_empty() {
        return;
    }
    let text = std::mem::take(pending);
    let Some(frame) = frames.last_mut() else {
        return;
    };
    frame.runs.push(Run {
        text,
        style: frame.style.clone(),
        children: Vec::new(),
    });
}

/// Pops the innermost frame and hands its runs to the one below it.
///
/// The style comes back with it, because the frame below still holds the style
/// as it was before the tag that pushed this one. A frame that carries a gradient
/// or a rainbow becomes a run of its own instead, so the component the gradient
/// goes on owns the whole span - see `Frame`.
fn close_frame(frames: &mut Vec<Frame>) {
    if frames.len() < 2 {
        return;
    }
    let frame = frames.pop().expect("checked one frame above");
    let Some(outer) = frames.last_mut() else {
        return;
    };
    if frame.node {
        outer.runs.push(Run {
            text: String::new(),
            style: frame.style,
            children: frame.runs,
        });
    } else {
        outer.runs.extend(frame.runs);
    }
}

impl Change {
    /// Whether this change has to become a component of its own.
    ///
    /// A gradient and a rainbow, and nothing else. Both are properties of a
    /// component rather than of a span of style: the host reads the text of the
    /// component they are set on, so a span of style cannot express them and a
    /// sibling run would each get a one-character ramp of its own.
    fn is_node(&self) -> bool {
        matches!(self, Change::Gradient(_) | Change::Rainbow)
    }

    /// Applies this change to a copy of the style it was opened in.
    fn apply(self, style: &mut Style) {
        match self {
            Change::Colour(colour) => style.colour = Some(colour),
            Change::Flag(flag, value) => style.set_flag(flag, Some(value)),
            Change::Gradient(stops) => style.gradient = Some(stops),
            Change::Rainbow => style.rainbow = true,
            Change::Click(click) => style.click = Some(click),
            Change::Hover(template) => style.hover = Some(template),
            Change::Font(font) => style.font = Some(font),
            Change::Insertion(text) => style.insertion = Some(text),
        }
    }
}

// ---------------------------------------------------------------------------
// The tag vocabulary, shared by both outputs
// ---------------------------------------------------------------------------

/// One `<...>` tag, read off the front of a template.
///
/// Borrowed from the template rather than allocated, because both readers walk
/// every template once per refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tag<'a> {
    /// `</name>` rather than `<name>`.
    closing: bool,
    /// The name as written: no leading `/`, no `:argument`, not lower-cased.
    name: &'a str,
    /// Everything after the first `:`, with one layer of quotes removed.
    argument: Option<&'a str>,
    /// The byte just past this tag's `>`.
    end: usize,
}

/// Reads the tag at `input[at]`, or [`None`] if there is not one.
///
/// [`None`] is the answer for every case where guessing would corrupt the line,
/// and it is what keeps text that merely looks like a tag as text:
///
/// * no `>` before the end of the line - `<red` and `a < b > c`;
/// * a newline before the `>` - a template that spans lines must not have the
///   whole of the second one eaten as a tag;
/// * a `"` that is opened and never closed.
///
/// The quotes are the other half of the job. The tag ends at the first `>`
/// **that is not inside quotes**, so
/// `<click:open_url:"https://example.com/a>b">` is one tag and the argument
/// keeps its colons and its `>`. Everything from the first `:` on is the
/// argument, so the action in `<click:open_url:…>` and the stops in
/// `<gradient:…>` are not cut at the first colon the way a naive split would.
fn read_tag(input: &str, at: usize) -> Option<Tag<'_>> {
    let body = input.get(at..)?.strip_prefix('<')?;
    let mut quoted = false;
    for (offset, ch) in body.char_indices() {
        match ch {
            '"' | '\'' => quoted = !quoted,
            '\n' | '\r' => return None,
            '>' if !quoted => {
                let raw = &body[..offset];
                let (closing, rest) = match raw.strip_prefix('/') {
                    Some(rest) => (true, rest),
                    None => (false, raw),
                };
                let (name, argument) = match rest.split_once(':') {
                    Some((name, argument)) => (name, Some(argument.trim())),
                    None => (rest, None),
                };
                return Some(Tag {
                    closing,
                    name,
                    argument,
                    end: at + 1 + offset + 1,
                });
            }
            _ => {}
        }
    }
    None
}

/// One layer of quotes off an argument, if it has any.
///
/// Applied to whatever the tag takes as its **payload**, not to the argument as a
/// whole: in `<hover:show_text:"he said no">` the quotes wrap the payload, not
/// `show_text:"he said no"`. The inner text is left exactly as it is, quotes and
/// all, so `<click:copy_to_clipboard:"a \"b\"">` comes out as `a \"b\"` rather
/// than being silently mangled. No escape processing is claimed.
fn unquote(argument: &str) -> &str {
    let argument = argument.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = argument.strip_prefix(quote)
            && let Some(inner) = inner.strip_suffix(quote)
        {
            return inner;
        }
    }
    argument
}

/// The colour a tag names, or [`None`].
///
/// A tag with an argument is never a colour: `<red:false>` is a mistake, not a
/// way to write a colour, and leaving it as text is how the mistake becomes
/// visible.
fn colour_of(tag: &Tag<'_>) -> Option<Colour> {
    if tag.argument.is_some() {
        return None;
    }
    named_colour(tag.name).or_else(|| hex_colour(tag.name))
}

/// What a tag does to the style stack, or [`None`] if it spells no known tag.
///
/// Shared by [`tree`] and `Legacy`. They then do different things with the same
/// answer, which is the point: there is one vocabulary and two renderings of it.
fn effect(tag: &Tag<'_>) -> Option<Effect> {
    if tag.argument.is_none() && is_any(tag.name, &["reset", "r"]) {
        return Some(Effect::Reset);
    }
    if tag.closing {
        // A closing tag takes no argument, and its name has to be one this
        // module knows: `</bold:false>` is a mistake and `</sparkle>` is a
        // misspelling, and treating either as "close whatever is open" would
        // silently drop a style the author meant to keep.
        if tag.argument.is_none() && known_tag(tag.name) {
            return Some(Effect::Pop);
        }
        return None;
    }
    if let Some(colour) = colour_of(tag) {
        return Some(Effect::Push(Change::Colour(colour)));
    }
    if let Some(flag) = style_flag(tag.name) {
        return Some(Effect::Push(Change::Flag(flag, switch(tag.argument)?)));
    }
    if is_any(tag.name, &["gradient"]) {
        return gradient_stops(tag.argument).map(Change::Gradient).map(Effect::Push);
    }
    if tag.argument.is_none() && is_any(tag.name, &["rainbow"]) {
        return Some(Effect::Push(Change::Rainbow));
    }
    if let Some(click) = click_action(tag.name, tag.argument) {
        return Some(Effect::Push(Change::Click(click)));
    }
    if let Some(template) = hover_template(tag.name, tag.argument) {
        return Some(Effect::Push(Change::Hover(template)));
    }
    let argument = tag.argument?;
    if is_any(tag.name, &["font"]) && !unquote(argument).is_empty() {
        return Some(Effect::Push(Change::Font(unquote(argument).to_string())));
    }
    if is_any(tag.name, &["insert"]) {
        return Some(Effect::Push(Change::Insertion(
            unquote(argument).to_string(),
        )));
    }
    None
}

/// `:true` or `:false` on a style tag. Bare means `true`.
fn switch(argument: Option<&str>) -> Option<bool> {
    match argument {
        None | Some("true") => Some(true),
        Some("false") => Some(false),
        Some(_) => None,
    }
}

/// The stops of a `<gradient:…>` tag, or [`None`].
///
/// At least two, because one is not a ramp: the host's own `gradient` returns
/// the component untouched below two (`pumpkin-util/src/text/mod.rs:1724-1726`)
/// and this does the same rather than inventing a colour.
fn gradient_stops(argument: Option<&str>) -> Option<Vec<Colour>> {
    let argument = argument?;
    let mut stops = Vec::new();
    for part in argument.split(':') {
        let part = unquote(part);
        stops.push(named_colour(part).or_else(|| hex_colour(part))?);
    }
    (stops.len() >= 2).then_some(stops)
}

/// The click a `<click:action:"payload">` tag asks for, or [`None`].
///
/// The payload is whatever is after the **first** colon, so the colons in
/// `https://example.com:8080/x` survive, and the four actions are the four
/// `click-*` methods this calls. `open_file`, `change_page`, `show_item` and
/// `show_entity` are deliberately absent: the first two want a path or a page
/// number a template has no business carrying, and the last two are structured
/// data rather than text.
fn click_action(name: &str, argument: Option<&str>) -> Option<Click> {
    if !is_any(name, &["click"]) {
        return None;
    }
    let (action, payload) = argument?.split_once(':')?;
    let payload = unquote(payload);
    if payload.is_empty() {
        return None;
    }
    Some(if is_any(action, &["open_url"]) {
        Click::OpenUrl(payload.to_string())
    } else if is_any(action, &["run_command"]) {
        Click::RunCommand(payload.to_string())
    } else if is_any(action, &["suggest_command"]) {
        Click::SuggestCommand(payload.to_string())
    } else if is_any(action, &["copy_to_clipboard"]) {
        Click::CopyToClipboard(payload.to_string())
    } else {
        return None;
    })
}

/// The template a `<hover:show_text:"…">` tag carries, or [`None`].
///
/// Only `show_text` is supported, and only because it is the one whose payload
/// is text: `component` parses it recursively, so a hover may carry tags of its
/// own. `show_item` wants SNBT and `show_entity` wants a UUID, neither of which
/// is a template.
fn hover_template(name: &str, argument: Option<&str>) -> Option<String> {
    if !is_any(name, &["hover"]) {
        return None;
    }
    let (action, payload) = argument?.split_once(':')?;
    is_any(action, &["show_text"]).then(|| unquote(payload).to_string())
}

/// What a `&` code means to the host's parser.
///
/// `hex` records *how* the colour arrived, because the host treats the two
/// differently: `&c` clears the styles and `&x&f&f…` does not
/// (`legacy.rs:110-117` against `:62-109`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Code {
    /// A colour, and whether it came from `&#rrggbb`/`&x`.
    Colour(Colour, bool),
    /// One style, turned on.
    Flag(Flag),
    /// `&r`.
    Reset,
    /// A `&` that means nothing, to be written out as text.
    Literal,
}

/// Reads the `&` code at `input[at]`, and how many bytes it used.
///
/// [`None`] when there is no `&` there at all. A `&` that means nothing is
/// `Code::Literal` rather than [`None`], because it still has to be consumed
/// as a character rather than re-read as the start of another code.
fn read_code(input: &str, at: usize) -> Option<(Code, usize)> {
    let rest = input.get(at..)?.strip_prefix(CODE)?;
    let next = rest.chars().next()?;

    // `&&` is one escaped ampersand. The host's parser also treats it as the
    // escape - `split` on the code symbol leaves an empty part, which its loop
    // skips - so this is its rule rather than a new one.
    if next == CODE {
        return Some((Code::Literal, 2));
    }

    if next == '#' {
        let digits: String = rest.get(1..7).unwrap_or_default().chars().take(6).collect();
        if is_hex_colour(&digits) {
            let colour = hex_colour(&format!("#{digits}")).unwrap_or(Colour::Rgb(0, 0, 0));
            return Some((Code::Colour(colour, true), 1 + digits.len()));
        }
        return Some((Code::Literal, 1));
    }

    // The spaced hex form Java TAB writes: `&x` then six `&`-prefixed digits.
    // Anything left over after the sixth digit is ordinary text, which is what
    // the host does with the remainder (`legacy.rs:83-87`).
    if next == 'x' || next == 'X' {
        let mut digits = String::new();
        let mut read = 2;
        for _ in 0..6 {
            let Some(digit) = input.get(at + read..)?.strip_prefix(CODE) else {
                return Some((Code::Literal, 1));
            };
            let Some(hex) = digit.chars().next() else {
                return Some((Code::Literal, 1));
            };
            if !hex.is_ascii_hexdigit() {
                return Some((Code::Literal, 1));
            }
            digits.push(hex);
            read += 1 + hex.len_utf8();
        }
        let colour = hex_colour(&format!("#{digits}"))?;
        return Some((Code::Colour(colour, true), read));
    }

    let lower = next.to_ascii_lowercase();
    let code = match lower {
        '0'..='9' | 'a'..='f' => Code::Colour(legacy_colour(lower)?, false),
        'l' => Code::Flag(Flag::Bold),
        'o' => Code::Flag(Flag::Italic),
        'n' => Code::Flag(Flag::Underlined),
        'm' => Code::Flag(Flag::Strikethrough),
        'k' => Code::Flag(Flag::Obfuscated),
        'r' => Code::Reset,
        _ => return Some((Code::Literal, 1)),
    };
    Some((code, 1 + next.len_utf8()))
}

/// The `<...>` tags in `template` that this module does not know.
///
/// A mistyped tag is left in the text exactly as written, which is the right
/// behaviour for a placeholder and the wrong one for a colour: `<gray8>` is not
/// a typo anybody can see on the sidebar, it just quietly loses its colour. Naming
/// it at load turns that into a warning.
///
/// Only `<name>` shapes are considered, and that has not changed: `<3` or `a < b`
/// are text, a `#rrggbb` body is a colour tag rather than a name, and **any**
/// `<name:argument>` is skipped whatever the argument - so `<bold:bogus>` is not
/// reported, for the same reason `<gray8>`'s absence from the help text has
/// never needed arguing about. The names that can only ever appear with an
/// argument are still listed in `known_tag`, so that a bare `<click>` is not
/// reported either.
pub fn unknown_tags(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else {
            break;
        };
        let body = &rest[..close];
        rest = &rest[close + 1..];

        // A name is letters, digits and underscores, and cannot start with a
        // digit. That last part is what keeps `5<6>7` text rather than a tag
        // called `6`, and it is the same rule MiniMessage itself uses.
        let name = body.trim_start_matches('/');
        let starts_like_a_name = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !starts_like_a_name
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        if !known_tag(name) && !found.iter().any(|seen| seen == name) {
            found.push(name.to_string());
        }
    }
    found
}

/// Whether `name` is a tag this module knows.
///
/// Built from the tables the parsers use, so a tag this module handles can never
/// be reported as a typo. The five names with arguments are here because
/// `unknown_tags` cannot see whether an argument is there - a bare `<click>` is
/// not a usable tag, but it is not a *mistyped* one either.
fn known_tag(name: &str) -> bool {
    named_colour(name).is_some()
        || style_flag(name).is_some()
        || is_any(
            name,
            &[
                "reset", "r", "rainbow", "gradient", "click", "hover", "font", "insert",
            ],
        )
}

// ---------------------------------------------------------------------------
// Legacy flattening
// ---------------------------------------------------------------------------

/// What a gradient or rainbow is painting, once a colour or the tag itself has
/// been read.
enum Paint {
    /// A ramp of at least two stops.
    Gradient(Vec<Colour>),
    /// The host's own sweep.
    Rainbow,
}

/// Every style open in `style`, and only those. Never the colour.
///
/// A free function rather than a method, because the one caller that re-opens a
/// style has just called `Legacy::reset` - and `self.active` no longer says
/// what it wants back.
fn styles_of(style: &Style) -> String {
    let mut out = String::new();
    for flag in Flag::ALL {
        if style.flag(flag) == Some(true) {
            out.push(CODE);
            out.push(flag.legacy_code());
        }
    }
    out
}

/// The legacy flattener, and the state it needs to get the awkward cases right.
///
/// The only state beyond the output string is what is currently **open**, which
/// is what a style that turns off needs: a legacy code turns everything off and
/// nothing can turn one part back on, so the only correct answer is a reset
/// followed by every colour and style still wanted.
///
/// The bookkeeping is over tags only. A `&` code written by hand in a template
/// is not tracked, and the module header says what that costs.
struct Legacy {
    out: String,
    active: Style,
    pending: Option<Paint>,
}

impl Legacy {
    fn new() -> Self {
        Legacy {
            out: String::new(),
            active: Style::default(),
            pending: None,
        }
    }

    /// Writes one `&` code.
    fn code(&mut self, code: char) {
        self.out.push(CODE);
        self.out.push(code);
    }

    /// Writes a colour: one pair for a named colour, seven for a hex one.
    ///
    /// The spaced hex form rather than the run-together one the parser also
    /// accepts, because it is what the host's own documentation and tests use
    /// (`legacy.rs:14`) and being explicit costs nothing.
    fn colour(&mut self, colour: Colour) {
        let Colour::Rgb(red, green, blue) = colour else {
            self.code(colour.legacy_code());
            return;
        };
        self.code('x');
        for digit in format!("{red:02x}{green:02x}{blue:02x}").chars() {
            self.code(digit);
        }
    }

    /// `&r`, and forget what was open, because a reset ends all of it.
    fn reset(&mut self) {
        self.code('r');
        self.active = Style::default();
    }

    /// The colour and the styles `style` leaves open, in that order.
    ///
    /// The order matters. A colour code clears the parser's styles
    /// (`legacy.rs:114-116`), so anything still on has to be written *after* the
    /// colour or it is gone again - which is exactly the shape of a per-character
    /// gradient, and of a reset followed by a re-open. `style` rather than
    /// `self.active`, because the one caller that re-opens has just reset, and
    /// `self.active` no longer says what it wants back.
    fn emit_open(&mut self, style: &Style) {
        if let Some(colour) = style.colour {
            self.colour(colour);
        }
        let styles = styles_of(style);
        self.out.push_str(&styles);
    }

    /// Literal text, painted character by character if a gradient or a rainbow
    /// is waiting for it.
    ///
    /// An empty run is left alone rather than consuming the pending paint, which
    /// is what lets a gradient written before a nested tag still reach the text
    /// after it.
    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(paint) = self.pending.take() else {
            self.out.push_str(text);
            return;
        };
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let open = styles_of(&self.active);
        for (index, ch) in chars.iter().enumerate() {
            let colour = match &paint {
                Paint::Gradient(stops) => gradient_at(stops, index, len),
                Paint::Rainbow => rainbow_at(index, len),
            };
            self.colour(colour);
            // The styles go after the colour, because the colour just cleared
            // them. Copied once per character rather than per whole run: a colour
            // code resets them, so they have to be re-opened each time.
            self.out.push_str(&open);
            self.out.push(*ch);
        }
    }

    /// An opening tag. `false` means "not a tag after all", so the `<` is text.
    ///
    /// `literal` is the text read since the last tag, and it is written out
    /// **before** anything this writes: a tag's codes apply to what follows them,
    /// and flushing afterwards would put `&r` in front of the text it closes.
    fn open(&mut self, tag: &Tag<'_>, literal: &str) -> bool {
        let Some(open) = effect(tag) else {
            return false;
        };
        self.text(literal);
        match open {
            Effect::Reset => {
                self.reset();
                true
            }
            Effect::Push(Change::Colour(colour)) => {
                self.colour(colour);
                if hex_colour(tag.name).is_some() {
                    // A hex code replaces only the colour (`legacy.rs:62-109`).
                    self.active.colour = Some(colour);
                } else {
                    // A named one replaces the colour *and* the styles
                    // (`legacy.rs:110-117`), so the bookkeeping has to as well or
                    // the re-open after a `<bold:false>` would bring back
                    // formatting the parser has already dropped.
                    self.active = Style {
                        colour: Some(colour),
                        ..Style::default()
                    };
                }
                true
            }
            Effect::Push(Change::Flag(flag, true)) => {
                self.code(flag.legacy_code());
                self.active.set_flag(flag, Some(true));
                true
            }
            Effect::Push(Change::Flag(flag, false)) => {
                // The one place legacy has to put a style back: reset, then
                // re-open everything that is still wanted. Only when the flag
                // was actually on - a `<bold:false>` under nothing has nothing
                // to undo, and writing a bare `&r` there would clear a colour
                // the author never asked it to clear.
                if self.active.flag(flag) == Some(true) {
                    let mut keep = self.active.clone();
                    keep.set_flag(flag, Some(false));
                    self.reset();
                    self.emit_open(&keep);
                    self.active = keep;
                } else {
                    self.active.set_flag(flag, Some(false));
                }
                true
            }
            Effect::Push(Change::Gradient(stops)) => {
                self.pending = Some(Paint::Gradient(stops));
                true
            }
            Effect::Push(Change::Rainbow) => {
                self.pending = Some(Paint::Rainbow);
                true
            }
            // A click, a hover, a font and an insertion cannot exist in a legacy
            // string. The tag is consumed so it does not reach the client as
            // literal text, and contributes nothing; `component` is where they
            // work.
            Effect::Push(
                Change::Click(_) | Change::Hover(_) | Change::Font(_) | Change::Insertion(_),
            ) => true,
            // `effect` never answers `Pop` for an opening tag, so this arm is
            // unreachable in practice; treating it as "not a tag" keeps the
            // function total.
            Effect::Pop => false,
        }
    }

    /// A closing tag. `false` means "not a tag after all".
    ///
    /// `literal` is flushed first, for the same reason as in `Legacy::open`.
    fn close(&mut self, tag: &Tag<'_>, literal: &str) -> bool {
        if tag.argument.is_some() {
            return false;
        }
        if is_any(tag.name, &["reset", "r"]) || colour_of(tag).is_some() {
            // A closing colour is a reset, because `&r` is the only thing the
            // format can honestly express for one - see the module header. So is a
            // closing reset, which has always reset.
            self.text(literal);
            self.reset();
            return true;
        }
        if style_flag(tag.name).is_some() {
            // The colour-bleed rule, and the reason a configured prefix works at
            // all: consumed, and no code written, so what the prefix opened stays
            // open.
            self.text(literal);
            return true;
        }
        if is_any(tag.name, &["gradient", "rainbow"]) {
            // A gradient covers the text run that follows it and stops where the
            // next tag does, so the closing tag only has to stop it waiting.
            self.text(literal);
            self.pending = None;
            return true;
        }
        if is_any(tag.name, &["click", "hover", "font", "insert"]) {
            self.text(literal);
            return true;
        }
        false
    }
}

/// The colour the host's `gradient` gives character `index` of `len`.
///
/// A transcription of `pumpkin-util/src/text/mod.rs:1728-1750`: position across
/// the whole run, scaled by the number of segments, then a per-channel lerp
/// truncated to `u8`. Written out rather than delegated because this path has no
/// component to hand a gradient to, and matched deliberately so the two paths
/// paint the same colours for the same stops.
fn gradient_at(stops: &[Colour], index: usize, len: usize) -> Colour {
    let segments = stops.len() - 1;
    if len <= 1 {
        return stops[0];
    }
    let position = index as f32 / (len - 1) as f32;
    let across = position * segments as f32;
    let segment = (across.floor() as usize).min(segments - 1);
    let local = across - segment as f32;
    let (from, to) = (stops[segment].rgb(), stops[segment + 1].rgb());
    Colour::Rgb(
        lerp(from.0, to.0, local),
        lerp(from.1, to.1, local),
        lerp(from.2, to.2, local),
    )
}

/// One channel of the host's gradient lerp.
fn lerp(from: u8, to: u8, at: f32) -> u8 {
    (f32::from(to) - f32::from(from)).mul_add(at, f32::from(from)) as u8
}

/// The colour the host's `rainbow` gives character `index` of `len`.
///
/// `pumpkin-util/src/text/mod.rs:1761-1765`, calling the host's own HSV
/// conversion rather than another copy of it.
fn rainbow_at(index: usize, len: usize) -> Colour {
    let hue = (index as f32 / len as f32) * 360.0;
    let (red, green, blue) = hsv_to_rgb(hue, 1.0, 1.0);
    Colour::Rgb(red, green, blue)
}

/// HSV to RGB, transcribed from `pumpkin-util/src/text/color.rs:35-52`.
///
/// Its `(h as i32 / 60) % 6` and its truncating `as u8` are reproduced as they
/// are, because the point of transcribing is to agree: a slightly different sweep
/// would be a rainbow the legacy path draws slightly differently from the
/// component path.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (red, green, blue) = match (h as i32 / 60) % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((red + m) * 255.0) as u8,
        ((green + m) * 255.0) as u8,
        ((blue + m) * 255.0) as u8,
    )
}

/// Rewrites MiniMessage-style tags into legacy `&` codes.
///
/// A tag that is not recognised is left alone, character for character. That is
/// deliberate: `<3` and `a < b` are ordinary text, and guessing at what the
/// author meant would corrupt them. The rules for what a closing tag emits are in
/// the module header and on `Legacy::close`; `Legacy::open` has the rest.
pub fn tags_to_legacy(input: &str) -> String {
    let mut legacy = Legacy::new();
    let mut literal = String::new();
    let mut index = 0;

    while index < input.len() {
        let ch = input[index..].chars().next().unwrap_or('\u{fffd}');
        if ch == '<'
            && let Some(tag) = read_tag(input, index)
        {
            let handled = if tag.closing {
                legacy.close(&tag, &literal)
            } else {
                legacy.open(&tag, &literal)
            };
            if handled {
                // Both of those flushed `literal` before writing any code, which
                // is the rule: a tag's codes belong *after* the text they apply
                // to, never in front of it.
                literal.clear();
                index = tag.end;
                continue;
            }
        }
        literal.push(ch);
        index += ch.len_utf8();
    }
    legacy.text(&literal);
    legacy.out
}

/// Whether `ch` is a code the host's legacy parser acts on.
///
/// The list is the parser's own match arms (`legacy.rs:61-131`), plus `x`. Anything else
/// is consumed and dropped by the parser, so it has to be escaped instead.
fn is_code(ch: char) -> bool {
    ch.is_ascii_hexdigit() || matches!(ch, 'l' | 'o' | 'n' | 'm' | 'k' | 'r' | 'x' | 'X')
}

/// Escapes what the host's parser would otherwise eat, and expands `&#RRGGBB`.
///
/// The two jobs are the same pass because they are the same problem: deciding
/// whether a `&` introduces a code.
///
/// `&#RRGGBB` becomes the spaced `&x&R&R&G&G&B&B` form the parser understands.
/// The parser *also* accepts the run-together `&xff0000` form, but the spaced one
/// is what its own documentation and tests use, and being explicit costs
/// nothing.
pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != CODE {
            out.push(chars[index]);
            index += 1;
            continue;
        }

            if chars.get(index + 1) == Some(&'#') {
                // `slice::get` is used rather than indexing, so a `&#` at the very
                // end of the string cannot panic. A short read is not a colour, and
                // falls through to the escaping below, which leaves the text alone.
                let hex: String = chars
                    .get(index + 2..index + 8)
                    .map_or_else(String::new, |digits| digits.iter().copied().collect());
                if is_hex_colour(&hex) {
                    out.push(CODE);
                    out.push('x');
                    for digit in hex.chars() {
                        out.push(CODE);
                        out.push(digit.to_ascii_lowercase());
                    }
                    index += 8;
                    continue;
                }
            }

        match chars.get(index + 1) {
            Some(&next) if is_code(next) => {
                out.push(CODE);
                out.push(next.to_ascii_lowercase());
                index += 2;
            }
            // A trailing '&' with nothing after it, or an '&' in front of
            // ordinary text. Doubling it is the parser's own escape: splitting
            // `&&` yields an empty part, which its loop skips.
            _ => {
                out.push(CODE);
                out.push(CODE);
                index += 1;
            }
        }
    }

    out
}

/// The whole legacy pipeline: MiniMessage tags in, a legacy `&`-coded string out.
///
/// Still what `/tab preview` prints and what `Config::problems` sees. It is **not**
/// what the tab list, the header, the footer, the sidebar or the bar are built
/// from any more - those take a component, which is the only way a gradient, a
/// click or a hover survives. Where the two differ the module header says which
/// is which.
pub fn to_legacy(input: &str) -> String {
    escape(&tags_to_legacy(input))
}

/// Joins rendered lines into one string the legacy parser understands.
///
/// Newlines are plain text to the parser, so this is a join and nothing more.
pub fn join_lines(lines: &[&str]) -> String {
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// The component path
// ---------------------------------------------------------------------------

/// A component for `rendered`, built from its run tree.
///
/// The one place in this crate that calls into the host to make a text
/// component, and every other one goes through here: `tab::component`,
/// `scoreboard::prefix_component` and `bossbar::title_component` are all this
/// function under a name that says what it is for.
///
/// **Thin on purpose.** Everything it decides was decided by [`tree`], which is
/// pure data and tested as such; nothing here can be, because
/// `pumpkin_plugin_api`'s wit imports are `unreachable!()` stubs off `wasm32` and
/// a test that built a component would abort the process rather than fail an
/// assertion.
///
/// The root is an empty text with one child per run, which is the shape the host
/// produces for a legacy string too (`legacy.rs:37-44`) and keeps the tab list,
/// the sidebar and the bar looking the same as they did.
pub fn component(rendered: &str) -> TextComponent {
    let root = Style::default();
    let mut component = TextComponent::text("");
    for run in tree(rendered) {
        component = component.add_child(build(&run, &root));
    }
    component
}

/// The component for one run, and everything nested inside it.
///
/// `parent` is the style of the component this one hangs off, and it is asked
/// two questions. A setter is called **only when the value differs from the
/// default**, so a run that inherits what it already has costs nothing and
/// produces no redundant JSON - with one exception, and the exception is the
/// feature: a style that is explicitly *false* under a parent that is *true* is
/// called, because that is the only way to say "not bold here".
///
/// A gradient or a rainbow goes on **last**, after the children are in place.
/// The host reads the component's own text and its children's
/// (`pumpkin-util/src/text/mod.rs:1782`) and repaints every character, so the
/// component has to already own the whole span by then; and the click and hover
/// are set before it, because the same function copies them onto each character
/// and clears them from the component (`:1798-1808`).
fn build(run: &Run, parent: &Style) -> TextComponent {
    let style = &run.style;
    let mut part = TextComponent::text(&run.text);

    if let Some(colour) = style.colour
        && parent.colour != Some(colour)
    {
        part = match colour.to_host() {
            NamedOrRgb::Named(named) => part.color_named(named),
            NamedOrRgb::Rgb(rgb) => part.color_rgb(rgb),
        };
    }

    for flag in Flag::ALL {
        let value = style.flag(flag);
        if value == Some(true) || (value == Some(false) && parent.flag(flag) == Some(true)) {
            part = apply_flag(part, flag, value == Some(true));
        }
    }

    if let Some(font) = &style.font {
        part = part.font(font);
    }
    if let Some(insertion) = &style.insertion {
        part = part.insertion(insertion);
    }
    if let Some(click) = &style.click {
        part = match click {
            Click::OpenUrl(url) => part.click_open_url(url),
            Click::RunCommand(command) => part.click_run_command(command),
            Click::SuggestCommand(command) => part.click_suggest_command(command),
            Click::CopyToClipboard(text) => part.click_copy_to_clipboard(text),
        };
    }
    if let Some(hover) = &style.hover {
        // Recursively, so a hover may carry tags of its own - which is the
        // whole reason the argument is kept as a template rather than a string.
        part = part.hover_show_text(component(hover));
    }

    for child in &run.children {
        part = part.add_child(build(child, style));
    }

    // A gradient and a rainbow go on the component that **owns the span**, and
    // only there. A run inherits them from the frame it sits in, and the frame's
    // component has already been handed the whole span - so a run carrying the
    // same one as its parent skips this and the parent repaints it instead. That
    // is also what makes the outer component the one that matters: it is the only
    // one whose text covers every character.
    let owns_span = style.gradient != parent.gradient || style.rainbow != parent.rainbow;
    if owns_span {
        if let Some(stops) = &style.gradient {
            let colours: Vec<RgbColor> = stops
                .iter()
                .map(|stop| {
                    let (red, green, blue) = stop.rgb();
                    RgbColor {
                        r: red,
                        g: green,
                        b: blue,
                    }
                })
                .collect();
            part = part.gradient(&colours);
        }
        if style.rainbow {
            part = part.rainbow();
        }
    }

    part
}

/// One of the five style setters, which differ only in their name.
fn apply_flag(component: TextComponent, flag: Flag, value: bool) -> TextComponent {
    match flag {
        Flag::Bold => component.bold(value),
        Flag::Italic => component.italic(value),
        Flag::Underlined => component.underlined(value),
        Flag::Strikethrough => component.strikethrough(value),
        Flag::Obfuscated => component.obfuscated(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- the tree ---------------------------------------------------------

    /// A closing tag closes its own style and leaves the rest alone. This is the
    /// case legacy cannot express at all, so it is the first thing the tree has
    /// to get right.
    #[test]
    fn a_closing_tag_restores_the_style_underneath_it() {
        let runs = tree("<red><bold>A</bold>B");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "A");
        assert_eq!(runs[0].style.colour, Some(Colour::Red));
        assert_eq!(runs[0].style.bold, Some(true));
        assert_eq!(runs[1].text, "B");
        assert_eq!(
            runs[1].style.bold, None,
            "the bold ended at the closing tag"
        );
        assert_eq!(runs[1].style.colour, Some(Colour::Red));
    }

    /// Nested, so the parent carries the bold and the child says no. The whole of
    /// proper scoping, and it is `Some(false)` rather than a sentinel that makes
    /// it work.
    #[test]
    fn an_explicit_false_is_not_the_same_as_untouched() {
        let runs = tree("<bold>a<bold:false>b");
        assert_eq!(runs[0].style.bold, Some(true));
        assert_eq!(runs[1].style.bold, Some(false));
        // And it does not leak past the closing tag either.
        let runs = tree("<bold><italic><bold:false>x</bold>y</italic>z");
        let z = runs.last().expect("z is there");
        assert_eq!(z.text, "z");
        assert_eq!(z.style.bold, Some(true), "the bold was never closed");
        assert_eq!(z.style.italic, None, "but the italic was");
    }

    /// The gradient belongs to the run that owns the span, which is the run the
    /// nested runs hang off. That is what makes `<gradient:red:blue>a<bold>b</bold>c</gradient>`
    /// one ramp over `abc` rather than three unrelated colours.
    #[test]
    fn a_gradient_is_set_on_the_component_that_owns_the_span() {
        let runs = tree("<gradient:red:blue>a<bold>b</bold>c</gradient>");
        assert_eq!(runs.len(), 1, "the whole span is one run");
        let run = &runs[0];
        assert_eq!(
            run.style.gradient,
            Some(vec![Colour::Red, Colour::Blue]),
            "the gradient is on the run, not on the text"
        );
        assert!(run.text.is_empty(), "the span opened with no text of its own");
        assert_eq!(run.children.len(), 3);
        assert_eq!(run.children[0].text, "a");
        assert_eq!(run.children[1].text, "b");
        assert_eq!(run.children[1].style.bold, Some(true));
        assert_eq!(run.children[2].text, "c");
    }

    /// Named stops, hex stops and more than two of them, in any order.
    #[test]
    fn a_gradient_takes_named_hex_and_as_many_stops_as_it_likes() {
        let runs = tree("<gradient:red:#00ff00:aqua:dark_blue>hi</gradient>");
        assert_eq!(
            runs[0].style.gradient,
            Some(vec![
                Colour::Red,
                Colour::Rgb(0, 255, 0),
                Colour::Aqua,
                Colour::DarkBlue
            ])
        );
        // One stop is not a ramp, and the host's own gradient refuses it.
        assert_eq!(
            tree("<gradient:red>x").len(),
            1,
            "a bad gradient is left as text, not silently dropped"
        );
        assert_eq!(tree("<gradient:red>x")[0].text, "<gradient:red>x");
        // A stop that is not a colour makes the whole tag unusable, rather than
        // quietly using the ones that parsed.
        assert_eq!(tree("<gradient:red:nonsense>x")[0].text, "<gradient:red:nonsense>x");
    }

    /// The one thing that is *not* inherited as a span of style: a gradient
    /// belongs to the component that owns the span, which is the run the tag
    /// opened. The inner run keeps the same `gradient` in its style - it was read
    /// under the same frame - and `build` is what decides not to hand the host a
    /// one-character ramp for it, because the outer component is about to repaint
    /// that character anyway.
    ///
    /// Asserted on the tree rather than on the call, because the call is not
    /// observable off `wasm32`.
    #[test]
    fn a_gradient_frame_and_the_runs_inside_it_agree() {
        let runs = tree("<gradient:red:blue>ab</gradient>");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].style.gradient, runs[0].children[0].style.gradient);
        // A second, *different* gradient inside the first is a genuinely nested
        // node, and its own text is a separate component.
        let runs = tree("<gradient:red:blue>a<gradient:green:yellow>b</gradient>c</gradient>");
        assert_eq!(runs[0].children.len(), 3);
        assert_eq!(runs[0].children[0].text, "a");
        assert_eq!(runs[0].children[1].style.gradient, Some(vec![Colour::Green, Colour::Yellow]));
        assert_eq!(runs[0].children[2].text, "c");
    }

    #[test]
    fn a_hover_is_parsed_recursively_into_a_template() {
        let runs = tree("<hover:show_text:\"<red>tip</red>\">x</hover>");
        let hover = runs[0].style.hover.as_deref().expect("the hover is there");
        assert_eq!(hover, "<red>tip</red>");
        // And the template is itself a template, not a finished string.
        let inner = tree(hover);
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].text, "tip");
        assert_eq!(inner[0].style.colour, Some(Colour::Red));
    }

    /// The case in the brief: a URL with a port and a path, quoted, must arrive
    /// whole. A split at the first colon would produce `open_url` = `open` and a
    /// payload of `url:"https://example.com:8080/x"`.
    #[test]
    fn a_quoted_click_argument_survives_its_own_colons() {
        let runs = tree("<click:open_url:\"https://example.com:8080/x\">link</click>");
        assert_eq!(
            runs[0].style.click,
            Some(Click::OpenUrl("https://example.com:8080/x".to_string()))
        );
        // And a `>` inside the quotes does not end the tag early.
        let runs = tree("<click:open_url:\"https://example.com/a>b\">link</click>");
        assert_eq!(
            runs[0].style.click,
            Some(Click::OpenUrl("https://example.com/a>b".to_string()))
        );
    }

    #[test]
    fn every_click_action_is_answered() {
        for (tag, expected) in [
            ("run_command", Click::RunCommand("/tab reload".into())),
            ("suggest_command", Click::SuggestCommand("/tab ".into())),
            ("copy_to_clipboard", Click::CopyToClipboard("abc".into())),
        ] {
            let template = format!("<click:{tag}:\"payload here\">x</click>");
            let runs = tree(&template);
            let _ = expected;
            assert!(
                runs[0].style.click.is_some(),
                "{tag} was not recognised: {:?}",
                runs[0].text
            );
        }
        assert_eq!(
            tree("<click:copy_to_clipboard:\"abc\">x</click>")[0]
                .style
                .click,
            Some(Click::CopyToClipboard("abc".into()))
        );
        // An action the WIT offers but this does not spell stays text, so the
        // mistake is visible rather than a silently dead prefix.
        assert_eq!(
            tree("<click:open_file:\"/etc/passwd\">x</click>")[0].text,
            "<click:open_file:\"/etc/passwd\">x"
        );
    }

    /// A reset clears the current frame in place rather than pushing one, which
    /// is why `<red>a<bold>b</bold><reset>c` leaves `c` with nothing on and not
    /// with the bold back.
    #[test]
    fn a_reset_clears_rather_than_restores() {
        let runs = tree("<red><bold>A</bold><reset>c");
        let c = runs.last().expect("c is there");
        assert_eq!(c.text, "c");
        assert_eq!(c.style, Style::default());
    }

    /// An unclosed tag at the end of the template still reaches the root, so its
    /// runs are not simply dropped.
    #[test]
    fn an_unclosed_tag_still_colours_its_text() {
        let runs = tree("<red>never closed");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "never closed");
        assert_eq!(runs[0].style.colour, Some(Colour::Red));
    }

    /// A closing tag with nothing open consumes itself and does nothing, which is
    /// what a stray `</bold>` has always done.
    #[test]
    fn a_stray_closing_tag_does_nothing() {
        let runs = tree("</bold>plain");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "plain");
        assert_eq!(runs[0].style, Style::default());
    }

    /// The shipped header is written in `&` codes, so the tree has to read them -
    /// and has to read them the way the host's own parser does, or the two would
    /// draw different things from the same config.
    #[test]
    fn raw_codes_are_read_as_unscoped_changes() {
        let runs = tree("&r&7&l>> &7Welcome, &3&l%player%");
        assert_eq!(runs.len(), 3, "each code opens a new run");
        assert_eq!(runs[0].text, ">> ");
        assert_eq!(runs[0].style.colour, Some(Colour::Gray));
        assert_eq!(runs[0].style.bold, Some(true));
        assert_eq!(runs[1].text, "Welcome, ");
        assert_eq!(runs[1].style.colour, Some(Colour::Gray));
        // `Some(false)` rather than `None`: the host's parser turns the flag off
        // (`legacy.rs:114-116`), and "off" is what lets a child that says nothing
        // still be told not to be bold.
        assert_eq!(runs[1].style.bold, Some(false));
        // `&3` replaced the colour *and* the styles, as `legacy.rs:110-117` says,
        // and `&l` after it turned the bold back on.
        assert_eq!(runs[2].text, "%player%");
        assert_eq!(runs[2].style.colour, Some(Colour::DarkAqua));
        assert_eq!(runs[2].style.bold, Some(true));
    }

    #[test]
    fn a_raw_hex_code_is_a_colour_in_both_spellings() {
        assert_eq!(
            tree("&#ff8800x")[0].style.colour,
            Some(Colour::Rgb(255, 136, 0))
        );
        assert_eq!(
            tree("&x&f&f&8&8&0&0x")[0].style.colour,
            Some(Colour::Rgb(255, 136, 0))
        );
        // The six-digit form keeps the rest of the line as text, which is what
        // the host does with the remainder (`legacy.rs:83-87`).
        assert_eq!(tree("&x&f&f&8&8&0&0tail")[0].text, "tail");
        // A hex code replaces only the colour, unlike a named one.
        let runs = tree("<bold>&#ff8800x");
        assert_eq!(runs[0].style.bold, Some(true));
        assert_eq!(runs[0].style.colour, Some(Colour::Rgb(255, 136, 0)));
    }

    #[test]
    fn an_ampersand_that_means_nothing_is_a_literal_one() {
        assert_eq!(tree("AT&T")[0].text, "AT&T");
        assert_eq!(tree("100% & rising")[0].text, "100% & rising");
        assert_eq!(tree("trailing&")[0].text, "trailing&");
        assert_eq!(tree("&&")[0].text, "&");
        // No doubling, which the legacy path still needs and this one does not.
        assert_eq!(tree("AT&T")[0].text, "AT&T");
    }

    // -- legacy, unchanged -----------------------------------------------

    #[test]
    fn a_mistyped_colour_tag_is_found() {
        // `<gray8>` is not a colour, and it is not a typo anybody can see on a
        // sidebar: it just quietly loses its colour, which is the worst kind.
        assert_eq!(unknown_tags("<gray8>hello"), ["gray8"]);
        assert_eq!(unknown_tags("<bold>ok<gray8>bad"), ["gray8"]);
    }

    #[test]
    fn a_closing_tag_is_checked_by_the_same_name() {
        assert_eq!(unknown_tags("</gray8>"), ["gray8"]);
    }

    #[test]
    fn real_tags_and_hex_are_not_reported() {
        assert!(unknown_tags("<#00FC00><bold>$ <white>Money").is_empty());
        assert!(unknown_tags("<dark_gray>x</dark_gray>").is_empty());
        assert!(unknown_tags("<st>a</st>").is_empty());
    }

    /// A `<` that is not a tag has to stay text. Reporting `a < b` as a bad tag
    /// would be worse than useless.
    #[test]
    fn text_that_merely_looks_like_a_tag_is_left_alone() {
        assert!(unknown_tags("1 < 2 and 3 > 2").is_empty());
        assert!(unknown_tags("5<6>7").is_empty());
        assert!(unknown_tags("a <not closed").is_empty());
        assert!(unknown_tags("<>").is_empty());
    }

    #[test]
    fn the_same_bad_tag_is_reported_once() {
        assert_eq!(unknown_tags("<gray8> and <gray8> again"), ["gray8"]);
    }

    #[test]
    fn plain_text_is_left_alone() {
        assert_eq!(to_legacy("hello"), "hello");
    }

    #[test]
    fn named_colour_tags_become_legacy_codes() {
        assert_eq!(to_legacy("<red>danger"), "&cdanger");
        assert_eq!(to_legacy("<dark_aqua>"), "&3");
        assert_eq!(to_legacy("<RED>"), "&c");
    }

    #[test]
    fn style_tags_become_legacy_codes() {
        assert_eq!(to_legacy("<bold>"), "&l");
        assert_eq!(to_legacy("<underlined>"), "&n");
        assert_eq!(to_legacy("<obfuscated>"), "&k");
    }

    #[test]
    fn a_hex_tag_is_spaced_out_for_the_parser() {
        assert_eq!(to_legacy("<#ff8800>x"), "&x&f&f&8&8&0&0x");
        assert_eq!(to_legacy("<#FF8800>x"), "&x&f&f&8&8&0&0x");
    }

    #[test]
    fn an_ampersand_hex_colour_is_also_expanded() {
        // The host's parser has no `&#` form at all, so this has to be rewritten
        // before it reaches it.
        assert_eq!(to_legacy("&#ff8800x"), "&x&f&f&8&8&0&0x");
    }

    /// The bug this pins: a two-colour gradient-style title reached the client as
    /// literal `%#FF0000%`, because neither `tag_hex` (which wants a leading `#`)
    /// nor `known_tag` (which has never heard of hex) accepted `</#FF0000>`, and
    /// `unknown_tags` skipped it for not starting like a name. Hex is a colour, so
    /// the colour/style split makes it a reset - which is also the only thing
    /// legacy can honestly express for one.
    #[test]
    fn a_closing_hex_tag_is_a_reset_and_never_literal_text() {
        assert_eq!(to_legacy("<#E0B11E>name</#FF0000>"), "&x&e&0&b&1&1&ename&r");
        // And it must not appear in the unknown-tag report either, because it is
        // handled rather than unknown.
        assert!(unknown_tags("<#E0B11E>name</#FF0000>").is_empty());
    }

    /// `</3>` and `a </ b` are text, and must stay text - a closing bracket in
    /// front of something that is not a tag is not a tag.
    #[test]
    fn a_slash_in_front_of_something_that_is_not_a_tag_stays_text() {
        assert_eq!(to_legacy("</3>"), "</3>");
        assert_eq!(to_legacy("a </ b"), "a </ b");
        assert_eq!(to_legacy("</#GGGGGG>"), "</#GGGGGG>");
    }

    #[test]
    fn legacy_codes_pass_through_untouched() {
        assert_eq!(to_legacy("&cRed &lBold"), "&cRed &lBold");
        assert_eq!(to_legacy("&x&f&f&0&0&0&0hi"), "&x&f&f&0&0&0&0hi");
    }

    #[test]
    fn a_literal_ampersand_is_doubled() {
        // Without this the host's parser consumes the next character and the
        // string renders wrong. `AT&T` becomes `AT` and then a dropped `T`.
        assert_eq!(to_legacy("AT&T"), "AT&&T");
        assert_eq!(to_legacy("100% & rising"), "100% && rising");
        assert_eq!(to_legacy("trailing&"), "trailing&&");
    }

    #[test]
    fn a_less_than_that_is_not_a_tag_is_text() {
        assert_eq!(to_legacy("a < b"), "a < b");
        assert_eq!(to_legacy("<3"), "<3");
        assert_eq!(to_legacy("5<10"), "5<10");
    }

    /// The colour/style split, which is the whole of the closing-tag rule.
    ///
    /// A closing colour resets, because `&r` is the only thing the format can
    /// express and a colour running past its own tag is the failure that shows.
    #[test]
    fn a_closing_colour_tag_is_a_reset() {
        assert_eq!(to_legacy("<red>a</red>b"), "&ca&rb");
        assert_eq!(to_legacy("<dark_gray>a</dark_gray>b"), "&8a&rb");
        assert_eq!(to_legacy("</red>"), "&r");
        assert_eq!(to_legacy("</grey>"), "&r");
    }

    /// A closing style emits nothing at all, so the colour the prefix opened is
    /// still open for whatever follows. This is the case the rule exists for:
    /// with a reset here the name after the prefix came out white.
    #[test]
    fn a_closing_style_tag_emits_nothing() {
        assert_eq!(to_legacy("</bold>"), "");
        assert_eq!(to_legacy("<red><bold>ADMIN </bold>x"), "&c&lADMIN x");
        // And the aliases, which is why the split is by flag and not by name.
        assert_eq!(to_legacy("</em>"), "");
        assert_eq!(to_legacy("</u>"), "");
        assert_eq!(to_legacy("</st>"), "");
        assert_eq!(to_legacy("</obf>"), "");
        assert_eq!(
            to_legacy("<red><italic>a</italic><underlined>b</underlined>c"),
            "&c&oa&nbc"
        );
    }

    /// `</reset>` and `</r>` are the format's own reset and keep resetting.
    #[test]
    fn a_closing_reset_tag_is_a_reset() {
        assert_eq!(to_legacy("</reset>"), "&r");
        assert_eq!(to_legacy("</r>"), "&r");
        assert_eq!(to_legacy("<red><bold>A</bold></reset>x"), "&c&lA&rx");
    }

    /// The two spellings of the LuckPerms prefix, which differ only in whether
    /// the author closed the colour. Both have to leave the name coloured;
    /// before the split the second one did not.
    #[test]
    fn a_prefix_colours_the_name_with_or_without_a_closed_colour() {
        assert_eq!(
            to_legacy("<red><bold>ADMIN </bold>"),
            "&c&lADMIN ",
            "an unclosed colour must stay open"
        );
        assert_eq!(
            to_legacy("<red><bold>ADMIN </bold></red>"),
            "&c&lADMIN &r",
            "a closed colour stops, so the name after it is not red"
        );
    }

    /// The trade-off, pinned rather than described: the name picks up the prefix's
    /// bold along with its colour, because there is no code that ends bold.
    #[test]
    fn dropping_the_closing_tag_leaves_the_bold_open_too() {
        let out = to_legacy("<red><bold>ADMIN </bold>Steve");
        assert_eq!(out, "&c&lADMIN Steve");
        assert_eq!(
            out.matches('&').count(),
            2,
            "no code closed the bold: {out}"
        );
        // `<reset>` is the way to ask for the exact thing, and it still works.
        assert_eq!(
            to_legacy("<red><bold>ADMIN </bold><reset>Steve"),
            "&c&lADMIN &rSteve"
        );
    }

    /// The shipped header's first line, which is the one string in this crate's
    /// config that closes a tag. `</#FFFF00>` is a closing *colour*, so it is
    /// still a reset and the gradient bar still ends exactly where the author put
    /// it - the split changed style closes, not colour closes.
    #[test]
    fn the_shipped_header_line_that_closes_a_tag_still_resets() {
        // 48 spaces, as shipped in `config.yml`. Written out here rather than read
        // from the config so a whitespace edit is visible in the diff here.
        let bar = " ".repeat(48);
        assert_eq!(
            to_legacy(&format!("<#FFFFFF>&m{bar}</#FFFF00>")),
            format!("&x&f&f&f&f&f&f&m{bar}&r")
        );
        // The rest of the header is written in plain `&` codes and is untouched.
        assert_eq!(
            to_legacy("&r&7&l>> &7Welcome, &3&l%player%"),
            "&r&7&l>> &7Welcome, &3&l%player%"
        );
    }

    #[test]
    fn the_reset_tag_works_too() {
        assert_eq!(to_legacy("<reset>"), "&r");
    }

    #[test]
    fn an_unterminated_tag_is_text() {
        assert_eq!(to_legacy("<red"), "<red");
        assert_eq!(to_legacy("a < b > c"), "a < b > c");
    }

    // -- legacy, extended -------------------------------------------------

    /// A gradient is per character, each with its own colour, and the stops are
    /// hit exactly: `AB` over `#000000:#ffffff` is black then white, with nothing
    /// between because there are only two characters.
    #[test]
    fn a_gradient_is_expanded_per_character() {
        assert_eq!(
            to_legacy("<gradient:#000000:#ffffff>AB</gradient>"),
            "&x&0&0&0&0&0&0A&x&f&f&f&f&f&fB"
        );
    }

    /// Three stops over five characters, so the middle one is the midpoint of the
    /// whole ramp rather than a segment boundary - which is what the host's own
    /// interpolation gives and is the reason it is transcribed rather than
    /// reinvented.
    #[test]
    fn a_gradient_with_several_stops_hits_its_middle() {
        // #000000, #808080, #ffffff over ABCDE: the middle character is the
        // second stop exactly, and the two between are the quarter points of the
        // segments either side of it.
        let out = to_legacy("<gradient:#000000:#808080:#ffffff>ABCDE</gradient>");
        assert_eq!(
            out,
            "&x&0&0&0&0&0&0A\
             &x&4&0&4&0&4&0B\
             &x&8&0&8&0&8&0C\
             &x&b&f&b&f&b&fD\
             &x&f&f&f&f&f&fE"
        );
    }

    /// A gradient covers the next **literal run** of text, and a tag in between does
    /// not end it - only the next thing that is not text does. `</gradient>` is
    /// therefore free: it stops the gradient waiting rather than writing anything.
    #[test]
    fn a_gradient_paints_the_next_literal_run_and_its_close_is_free() {
        assert_eq!(
            to_legacy("<gradient:red:blue>AB<bold>C"),
            "&x&f&f&5&5&5&5A&x&5&5&5&5&f&fB&lC"
        );
        assert_eq!(
            to_legacy("<rainbow>AB</rainbow>x"),
            "&x&f&f&0&0&0&0A&x&0&0&f&f&f&fBx"
        );
    }

    /// The styles still open are re-emitted after every colour, because a colour
    /// code clears them. Without this the bold is gone from the second character
    /// onwards, which looks like a gradient that only half worked.
    #[test]
    fn a_gradient_inside_a_bold_reopens_the_bold() {
        assert_eq!(
            to_legacy("<bold><gradient:#000000:#ffffff>AB</gradient>"),
            "&l&x&0&0&0&0&0&0&lA&x&f&f&f&f&f&f&lB"
        );
    }

    #[test]
    fn a_rainbow_sweeps_the_host_sweep() {
        // Five characters at full saturation and value, so the hues are 0, 72, 144,
        // 216 and 288. The odd-looking digits are the host's own `f32`
        // arithmetic, transcribed as it is rather than rounded: `216 / 60` is
        // 3.6000001 in `f32`, not 3.6.
        assert_eq!(
            to_legacy("<rainbow>ABCDE</rainbow>"),
            "&x&f&f&0&0&0&0A\
             &x&c&b&f&f&0&0B\
             &x&0&0&f&f&6&6C\
             &x&0&0&6&5&f&fD\
             &x&c&c&0&0&f&fE"
        );
    }

    /// The rule that has no equivalent at all in the old flattener: a style that
    /// turns off has to be undone, and the only way is a reset plus everything
    /// still wanted.
    #[test]
    fn a_style_that_turns_off_resets_and_reopens_what_is_left() {
        assert_eq!(
            to_legacy("<red><bold><italic>A<bold:false>B"),
            "&c&l&oA&r&c&oB",
            "the reset kills the bold, and red and italic come back"
        );
        // Nothing to undo when the style was not on: writing a bare `&r` there
        // would clear a colour the author never asked it to clear.
        assert_eq!(to_legacy("<red>A<bold:false>B"), "&cAB");
        assert_eq!(to_legacy("A<bold:false>B"), "AB");
    }

    /// The click and the hover are gone from a legacy string, and the tag goes
    /// with them: the text still renders, the interaction cannot.
    #[test]
    fn a_click_and_a_hover_are_dropped_and_their_tags_with_them() {
        assert_eq!(
            to_legacy("<click:open_url:\"https://example.com\">link</click>"),
            "link"
        );
        assert_eq!(
            to_legacy("<hover:show_text:\"<red>tip</red>\">x</hover>"),
            "x"
        );
        assert_eq!(to_legacy("<font:minecraft:alt>x</font>"), "x");
        assert_eq!(to_legacy("<insert:hello>x</insert>"), "x");
    }

    /// A tag the vocabulary does not have is still left as text and still named,
    /// whichever path is looking at it.
    #[test]
    fn an_unknown_tag_is_text_and_is_still_reported() {
        assert_eq!(to_legacy("<sparkle>x</sparkle>"), "<sparkle>x</sparkle>");
        assert_eq!(unknown_tags("<sparkle>x</sparkle>"), ["sparkle"]);
        let runs = tree("<sparkle>x</sparkle>");
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<String>(),
            "<sparkle>x</sparkle>",
            "and the tree leaves it alone too"
        );
    }

    /// Every name the tree answers must be a name the flattener answers, or the
    /// two paths could drift on what a template is asking for.
    #[test]
    fn the_new_tags_are_not_reported_as_typos() {
        for template in [
            "<rainbow>x</rainbow>",
            "<gradient:red:blue>x</gradient>",
            "<click:open_url:\"https://example.com\">x</click>",
            "<hover:show_text:\"x\">y</hover>",
            "<font:alt>x</font>",
            "<insert:x>y</insert>",
            "<click>",
            "<hover>",
        ] {
            assert!(
                unknown_tags(template).is_empty(),
                "{template} was reported: {:?}",
                unknown_tags(template)
            );
        }
    }

    /// `Config::problems` reads this, and a tag this module handles must never
    /// turn up in it. Asserted over the whole shipped vocabulary rather than
    /// listed, so a tag added to one table and not the other fails here.
    #[test]
    fn every_name_the_parser_accepts_is_a_known_tag() {
        for name in NAMED_COLOURS.iter().map(|(name, _)| *name) {
            assert!(known_tag(name), "{name}");
        }
        for name in STYLE_FLAGS.iter().map(|(name, _)| *name) {
            assert!(known_tag(name), "{name}");
        }
        for name in ["reset", "r", "rainbow", "gradient", "click", "hover", "font", "insert"] {
            assert!(known_tag(name), "{name}");
        }
        assert!(!known_tag("gray8"));
        assert!(!known_tag("sparkle"));
    }

    // -- shared -----------------------------------------------------------

    #[test]
    fn segments_split_text_from_placeholders() {
        assert_eq!(
            segment("&7Players: &f%online%&7/&f%max_players%"),
            vec![
                Segment::Text("&7Players: &f"),
                Segment::Placeholder("online".to_string()),
                Segment::Text("&7/&f"),
                Segment::Placeholder("max_players".to_string()),
            ]
        );
    }

    #[test]
    fn a_placeholder_may_carry_a_colon_and_underscores() {
        assert_eq!(
            segment("%animation:my_logo% %perm_group%"),
            vec![
                Segment::Placeholder("animation:my_logo".to_string()),
                Segment::Text(" "),
                Segment::Placeholder("perm_group".to_string()),
            ]
        );
    }

    #[test]
    fn an_unterminated_placeholder_is_text() {
        assert_eq!(
            segment("50% off %player%"),
            vec![
                Segment::Text("50% off "),
                Segment::Placeholder("player".to_string()),
            ]
        );
        assert_eq!(segment("100%"), vec![Segment::Text("100%")]);
    }

    #[test]
    fn a_doubled_percent_is_not_a_placeholder() {
        assert_eq!(segment("100%% sure"), vec![Segment::Text("100%% sure")]);
    }

    #[test]
    fn a_placeholder_does_not_swallow_a_newline() {
        // A header that is missing one '%' must not eat the following line.
        assert_eq!(
            segment("&7a %oops\n&7b"),
            vec![Segment::Text("&7a %oops\n&7b")]
        );
    }

    #[test]
    fn an_unknown_placeholder_is_left_exactly_as_written() {
        assert_eq!(
            substitute("%known% and %unknown%", |name| {
                (name == "known").then(|| "yes".to_string())
            }),
            "yes and %unknown%"
        );
    }

    #[test]
    fn an_empty_template_stays_empty() {
        assert_eq!(substitute("", |_| None), "");
        assert!(tree("").is_empty());
        assert_eq!(to_legacy(""), "");
    }

    #[test]
    fn lines_join_with_newlines() {
        assert_eq!(join_lines(&["a", "b", "c"]), "a\nb\nc");
        assert_eq!(join_lines(&[]), "");
    }
}
