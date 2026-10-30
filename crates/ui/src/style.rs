//! The visual system: palettes, surface tokens, typography, icons and the
//! styles of the standard iced widgets. Views never pick raw colors.

use std::sync::LazyLock;

use iced::{
    Background, Border, Color, Font, Shadow, Theme, Vector, border, font,
    theme::Palette,
    widget::{button, container, overlay::menu, pick_list, text, text_input, toggler},
};

pub const REGULAR: Font = Font::with_name("Inter");
pub const SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..Font::with_name("Inter")
};
pub const ICONS: Font = Font::with_name("lucide");

/// Corner radii shared by every surface so nested shapes stay concentric.
pub const RADIUS_CARD: f32 = 14.0;
pub const RADIUS_CONTROL: f32 = 8.0;

/// Lucide glyphs bundled in `assets/fonts/lucide/Lucide.ttf`.
#[derive(Clone, Copy, Debug)]
pub enum Icon {
    Refresh,
    Settings,
    ChevronDown,
    ChevronRight,
    Trash,
    Plus,
    Battery,
    BatteryCharging,
    BatteryWarning,
    Mouse,
    Keyboard,
    Headphones,
    Check,
    Close,
    Back,
    Lightbulb,
    Power,
    History,
    Alert,
    Revert,
    Plug,
    Info,
    Crosshair,
    Lock,
    LockOpen,
}

impl Icon {
    pub fn glyph(self) -> char {
        match self {
            Self::Refresh => '\u{e145}',
            Self::Settings => '\u{e154}',
            Self::ChevronDown => '\u{e06d}',
            Self::ChevronRight => '\u{e06f}',
            Self::Trash => '\u{e18e}',
            Self::Plus => '\u{e13d}',
            Self::Battery => '\u{e053}',
            Self::BatteryCharging => '\u{e054}',
            Self::BatteryWarning => '\u{e3ac}',
            Self::Mouse => '\u{e28e}',
            Self::Keyboard => '\u{e284}',
            Self::Headphones => '\u{e0f1}',
            Self::Check => '\u{e06c}',
            Self::Close => '\u{e1b2}',
            Self::Back => '\u{e048}',
            Self::Lightbulb => '\u{e1c2}',
            Self::Power => '\u{e140}',
            Self::History => '\u{e1f5}',
            Self::Alert => '\u{e077}',
            Self::Revert => '\u{e148}',
            Self::Plug => '\u{e37f}',
            Self::Info => '\u{e0f9}',
            Self::Crosshair => '\u{e0ac}',
            Self::Lock => '\u{e10b}',
            Self::LockOpen => '\u{e10c}',
        }
    }

    /// A device-type icon for OpenRazer's free-form type string.
    pub fn for_kind(kind: Option<&str>) -> Self {
        match kind.unwrap_or_default().to_ascii_lowercase().as_str() {
            "keyboard" | "keypad" => Self::Keyboard,
            "headset" => Self::Headphones,
            "mouse" | "mousemat" => Self::Mouse,
            _ => Self::Plug,
        }
    }
}

/// Surface colors that the iced palette does not describe.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub page: Color,
    pub sidebar: Color,
    pub card: Color,
    pub raised: Color,
    pub inset: Color,
    pub line: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub on_accent: Color,
    /// Second data series (charging) in charts.
    pub info: Color,
    pub warning: Color,
    pub danger: Color,
}

const DARK: Tokens = Tokens {
    page: Color::from_rgb8(0x13, 0x15, 0x18),
    sidebar: Color::from_rgb8(0x0D, 0x0E, 0x10),
    card: Color::from_rgb8(0x1B, 0x1D, 0x21),
    raised: Color::from_rgb8(0x25, 0x28, 0x2D),
    inset: Color::from_rgb8(0x12, 0x14, 0x17),
    line: Color::from_rgb8(0x2C, 0x2F, 0x35),
    text: Color::from_rgb8(0xE7, 0xE9, 0xEC),
    muted: Color::from_rgb8(0x8C, 0x92, 0x9B),
    accent: Color::from_rgb8(0x44, 0xD6, 0x2C),
    accent_soft: Color::from_rgba8(0x44, 0xD6, 0x2C, 0.13),
    on_accent: Color::from_rgb8(0x07, 0x1A, 0x04),
    info: Color::from_rgb8(0x38, 0xBD, 0xF8),
    warning: Color::from_rgb8(0xF5, 0xB5, 0x44),
    danger: Color::from_rgb8(0xF0, 0x5A, 0x5A),
};

const LIGHT: Tokens = Tokens {
    page: Color::from_rgb8(0xF3, 0xF4, 0xF6),
    sidebar: Color::from_rgb8(0xE8, 0xEA, 0xED),
    card: Color::WHITE,
    raised: Color::from_rgb8(0xEE, 0xF0, 0xF2),
    inset: Color::from_rgb8(0xF7, 0xF8, 0xF9),
    line: Color::from_rgb8(0xDA, 0xDD, 0xE2),
    text: Color::from_rgb8(0x16, 0x18, 0x1C),
    muted: Color::from_rgb8(0x68, 0x6F, 0x7A),
    accent: Color::from_rgb8(0x15, 0x9A, 0x3C),
    accent_soft: Color::from_rgba8(0x15, 0x9A, 0x3C, 0.11),
    on_accent: Color::WHITE,
    info: Color::from_rgb8(0x02, 0x84, 0xC7),
    warning: Color::from_rgb8(0xB4, 0x53, 0x09),
    danger: Color::from_rgb8(0xD9, 0x2D, 0x2D),
};

fn palette(tokens: &Tokens) -> Palette {
    Palette {
        background: tokens.page,
        text: tokens.text,
        primary: tokens.accent,
        success: tokens.accent,
        warning: tokens.warning,
        danger: tokens.danger,
    }
}

static DARK_THEME: LazyLock<Theme> =
    LazyLock::new(|| Theme::custom("razerr-rs dark", palette(&DARK)));
static LIGHT_THEME: LazyLock<Theme> =
    LazyLock::new(|| Theme::custom("razerr-rs light", palette(&LIGHT)));

pub fn theme(dark: bool) -> Theme {
    if dark {
        DARK_THEME.clone()
    } else {
        LIGHT_THEME.clone()
    }
}

pub fn tokens(theme: &Theme) -> Tokens {
    if theme.extended_palette().is_dark {
        DARK
    } else {
        LIGHT
    }
}

// --- Text -----------------------------------------------------------------

pub fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).muted),
    }
}

pub fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).accent),
    }
}

pub fn warning_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).warning),
    }
}

pub fn danger_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).danger),
    }
}

// --- Containers -----------------------------------------------------------

pub fn page(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(tokens(theme).page.into()),
        text_color: Some(tokens(theme).text),
        ..container::Style::default()
    }
}

pub fn sidebar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(tokens(theme).sidebar.into()),
        ..container::Style::default()
    }
}

pub fn card(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.card.into()),
        border: Border {
            color: tokens.line,
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
        ..container::Style::default()
    }
}

/// An inset well inside a card, for the DPI pad and the stage matrix.
pub fn well(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.inset.into()),
        border: Border {
            color: tokens.line,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn divider(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(tokens(theme).line.into()),
        ..container::Style::default()
    }
}

pub fn accent_line(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(tokens(theme).accent.into()),
        ..container::Style::default()
    }
}

/// The bar at the bottom of a card with unapplied edits.
pub fn pending_bar(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.warning.scale_alpha(0.08).into()),
        border: border::rounded(border::bottom(RADIUS_CARD - 1.0)),
        ..container::Style::default()
    }
}

pub fn chip(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.raised.into()),
        text_color: Some(tokens.muted),
        border: border::rounded(999),
        ..container::Style::default()
    }
}

pub fn warning_chip(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.warning.scale_alpha(0.14).into()),
        text_color: Some(tokens.warning),
        border: border::rounded(999),
        ..container::Style::default()
    }
}

pub fn error_banner(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.danger.scale_alpha(0.12).into()),
        text_color: Some(tokens.text),
        border: Border {
            color: tokens.danger.scale_alpha(0.45),
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn notice(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.warning.scale_alpha(0.10).into()),
        text_color: Some(tokens.text),
        border: Border {
            color: tokens.warning.scale_alpha(0.40),
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn tooltip(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.raised.into()),
        text_color: Some(tokens.text),
        border: Border {
            color: tokens.line,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.25),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

pub fn dialog(theme: &Theme) -> container::Style {
    container::Style {
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.35),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..card(theme)
    }
}

pub fn scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::BLACK.scale_alpha(0.55).into()),
        ..container::Style::default()
    }
}

pub fn swatch_fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(color.into()),
        border: Border {
            color: tokens(theme).line,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

// --- Buttons --------------------------------------------------------------

fn control(background: Option<Color>, text: Color, border: Border) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color: text,
        border,
        ..button::Style::default()
    }
}

fn disabled(tokens: &Tokens, style: &button::Style) -> button::Style {
    button::Style {
        background: style
            .background
            .map(|_| tokens.raised.scale_alpha(0.6).into()),
        text_color: tokens.muted.scale_alpha(0.7),
        border: Border {
            color: Color::TRANSPARENT,
            ..style.border
        },
        ..*style
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let base = control(
        Some(tokens.accent),
        tokens.on_accent,
        border::rounded(RADIUS_CONTROL),
    );
    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(theme.extended_palette().primary.strong.color.into()),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(tokens.accent.scale_alpha(0.85).into()),
            ..base
        },
        button::Status::Disabled => disabled(&tokens, &base),
    }
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let base = control(
        Some(tokens.raised),
        tokens.text,
        Border {
            color: tokens.line,
            width: 1.0,
            radius: RADIUS_CONTROL.into(),
        },
    );
    match status {
        button::Status::Active => base,
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(tokens.line.into()),
            ..base
        },
        button::Status::Disabled => disabled(&tokens, &base),
    }
}

/// Borderless until hovered; for toolbars, headers and row actions.
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let base = control(None, tokens.text, border::rounded(RADIUS_CONTROL));
    match status {
        button::Status::Active => base,
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(tokens.raised.into()),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: tokens.muted.scale_alpha(0.6),
            ..base
        },
    }
}

pub fn ghost_danger(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let base = control(None, tokens.muted, border::rounded(RADIUS_CONTROL));
    match status {
        button::Status::Active => base,
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(tokens.danger.scale_alpha(0.14).into()),
            text_color: tokens.danger,
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: tokens.muted.scale_alpha(0.35),
            ..base
        },
    }
}

pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let base = control(
        Some(tokens.danger),
        Color::WHITE,
        border::rounded(RADIUS_CONTROL),
    );
    match status {
        button::Status::Active => base,
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(tokens.danger.scale_alpha(0.85).into()),
            ..base
        },
        button::Status::Disabled => disabled(&tokens, &base),
    }
}

/// The axis lock: accent when locked, muted when free.
pub fn lock(locked: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let tokens = tokens(theme);
        let base = control(
            locked.then_some(tokens.accent_soft),
            if locked { tokens.accent } else { tokens.muted },
            Border {
                color: if locked {
                    tokens.accent.scale_alpha(0.5)
                } else {
                    tokens.line
                },
                width: 1.0,
                radius: RADIUS_CONTROL.into(),
            },
        );
        match status {
            button::Status::Hovered | button::Status::Pressed => button::Style {
                background: Some(
                    if locked {
                        tokens.accent_soft
                    } else {
                        tokens.raised
                    }
                    .into(),
                ),
                text_color: if locked { tokens.accent } else { tokens.text },
                ..base
            },
            button::Status::Disabled => disabled(&tokens, &base),
            button::Status::Active => base,
        }
    }
}

/// A sidebar device entry.
pub fn tile(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let tokens = tokens(theme);
        let base = if selected {
            control(
                Some(tokens.accent_soft),
                tokens.text,
                Border {
                    color: tokens.accent.scale_alpha(0.45),
                    width: 1.0,
                    radius: 10.0.into(),
                },
            )
        } else {
            control(None, tokens.text, border::rounded(10))
        };
        match status {
            button::Status::Hovered | button::Status::Pressed if !selected => button::Style {
                background: Some(tokens.raised.into()),
                ..base
            },
            _ => base,
        }
    }
}

/// Where a button sits in a segmented control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Only,
    First,
    Middle,
    Last,
}

impl Segment {
    pub fn at(index: usize, count: usize) -> Self {
        match (index, count) {
            (_, 0 | 1) => Self::Only,
            (0, _) => Self::First,
            (index, count) if index + 1 == count => Self::Last,
            _ => Self::Middle,
        }
    }
    fn radius(self) -> border::Radius {
        let r = RADIUS_CONTROL;
        match self {
            Self::Only => r.into(),
            Self::First => border::left(r),
            Self::Middle => 0.0.into(),
            Self::Last => border::right(r),
        }
    }
}

pub fn segment(
    selected: bool,
    position: Segment,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let tokens = tokens(theme);
        let border = Border {
            color: if selected { tokens.accent } else { tokens.line },
            width: 1.0,
            radius: position.radius(),
        };
        let base = if selected {
            control(Some(tokens.accent), tokens.on_accent, border)
        } else {
            control(Some(tokens.raised), tokens.text, border)
        };
        match status {
            // The selected option has no action of its own, but must not look disabled.
            _ if selected => base,
            button::Status::Hovered | button::Status::Pressed => button::Style {
                background: Some(tokens.line.into()),
                ..base
            },
            button::Status::Disabled => disabled(&tokens, &base),
            button::Status::Active => base,
        }
    }
}

pub fn swatch(color: Color, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let tokens = tokens(theme);
        let ring = match status {
            _ if selected => tokens.text,
            button::Status::Hovered | button::Status::Pressed => tokens.muted,
            _ => tokens.line,
        };
        button::Style {
            background: Some(color.into()),
            border: Border {
                color: ring,
                width: if selected { 2.0 } else { 1.0 },
                radius: 999.0.into(),
            },
            ..button::Style::default()
        }
    }
}

// --- Inputs ---------------------------------------------------------------

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let tokens = tokens(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => tokens.accent,
        text_input::Status::Hovered => tokens.muted.scale_alpha(0.6),
        text_input::Status::Active | text_input::Status::Disabled => tokens.line,
    };
    text_input::Style {
        background: tokens.inset.into(),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: RADIUS_CONTROL.into(),
        },
        icon: tokens.muted,
        placeholder: tokens.muted.scale_alpha(0.7),
        value: if status == text_input::Status::Disabled {
            tokens.muted
        } else {
            tokens.text
        },
        selection: tokens.accent.scale_alpha(0.3),
    }
}

pub fn slider(theme: &Theme, status: iced::widget::slider::Status) -> iced::widget::slider::Style {
    let tokens = tokens(theme);
    let radius = match status {
        iced::widget::slider::Status::Active => 7.0,
        iced::widget::slider::Status::Hovered | iced::widget::slider::Status::Dragged => 8.0,
    };
    iced::widget::slider::Style {
        rail: iced::widget::slider::Rail {
            backgrounds: (tokens.accent.into(), tokens.line.into()),
            width: 4.0,
            border: border::rounded(2),
        },
        handle: iced::widget::slider::Handle {
            shape: iced::widget::slider::HandleShape::Circle { radius },
            background: tokens.accent.into(),
            border_width: 3.0,
            border_color: tokens.card,
        },
    }
}

pub fn switch(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let tokens = tokens(theme);
    let (on, hovered, enabled) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false, true),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true, true),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false, false),
    };
    let track = if on { tokens.accent } else { tokens.line };
    toggler::Style {
        background: if enabled {
            track
        } else {
            track.scale_alpha(0.5)
        }
        .into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: if on { tokens.on_accent } else { tokens.text }
            .scale_alpha(if hovered { 1.0 } else { 0.92 })
            .into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.18,
    }
}

pub fn select(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let tokens = tokens(theme);
    let border_color = match status {
        pick_list::Status::Active => tokens.line,
        pick_list::Status::Hovered => tokens.muted.scale_alpha(0.6),
        pick_list::Status::Opened { .. } => tokens.accent,
    };
    pick_list::Style {
        text_color: tokens.text,
        placeholder_color: tokens.muted,
        handle_color: tokens.muted,
        background: tokens.raised.into(),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: RADIUS_CONTROL.into(),
        },
    }
}

pub fn select_menu(theme: &Theme) -> menu::Style {
    let tokens = tokens(theme);
    menu::Style {
        background: tokens.raised.into(),
        border: Border {
            color: tokens.line,
            width: 1.0,
            radius: RADIUS_CONTROL.into(),
        },
        text_color: tokens.text,
        selected_text_color: tokens.on_accent,
        selected_background: tokens.accent.into(),
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.3),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 16.0,
        },
    }
}

pub fn rgb(color: razerr_model::Rgb) -> Color {
    Color::from_rgb8(color.red, color.green, color.blue)
}

/// Theme adapter for `iced_audio`'s DPI sliders.
pub struct DpiSlider;

impl iced_audio::h_slider::StyleSheet for DpiSlider {
    type Style = Theme;

    fn idle(&self, theme: &Theme) -> iced_audio::h_slider::Appearance {
        let tokens = tokens(theme);
        iced_audio::h_slider::Appearance::Rect(iced_audio::h_slider::RectAppearance {
            back_color: tokens.line,
            back_border_width: 0.0,
            back_border_radius: 4.0,
            back_border_color: Color::TRANSPARENT,
            filled_color: tokens.accent,
            handle_color: tokens.text,
            handle_width: 4,
            handle_filled_gap: 0.0,
        })
    }
}

pub fn number_input(
    theme: &Theme,
    status: iced_aw::style::Status,
) -> iced_aw::style::number_input::Style {
    let tokens = tokens(theme);
    iced_aw::style::number_input::Style {
        button_background: Some(tokens.raised.into()),
        icon_color: match status {
            iced_aw::style::Status::Disabled => tokens.muted.scale_alpha(0.4),
            iced_aw::style::Status::Hovered | iced_aw::style::Status::Pressed => tokens.accent,
            _ => tokens.muted,
        },
    }
}
