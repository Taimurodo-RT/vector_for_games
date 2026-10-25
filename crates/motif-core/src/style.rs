//! Shape styles borrowed from CSS: blend modes, stroke caps and joins, shadows.

use crate::Color;

/// `mix-blend-mode`: how a shape mixes with what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Blend {
    pub const ALL: [Blend; 16] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
        Blend::Darken,
        Blend::Lighten,
        Blend::ColorDodge,
        Blend::ColorBurn,
        Blend::HardLight,
        Blend::SoftLight,
        Blend::Difference,
        Blend::Exclusion,
        Blend::Hue,
        Blend::Saturation,
        Blend::Color,
        Blend::Luminosity,
    ];

    pub fn keyword(self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Multiply => "multiply",
            Blend::Screen => "screen",
            Blend::Overlay => "overlay",
            Blend::Darken => "darken",
            Blend::Lighten => "lighten",
            Blend::ColorDodge => "color-dodge",
            Blend::ColorBurn => "color-burn",
            Blend::HardLight => "hard-light",
            Blend::SoftLight => "soft-light",
            Blend::Difference => "difference",
            Blend::Exclusion => "exclusion",
            Blend::Hue => "hue",
            Blend::Saturation => "saturation",
            Blend::Color => "color",
            Blend::Luminosity => "luminosity",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Blend::ALL.into_iter().find(|b| b.keyword() == s)
    }
}

/// `stroke-linecap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cap {
    Butt,
    #[default]
    Round,
    Square,
}

impl Cap {
    pub const ALL: [Cap; 3] = [Cap::Butt, Cap::Round, Cap::Square];

    pub fn keyword(self) -> &'static str {
        match self {
            Cap::Butt => "butt",
            Cap::Round => "round",
            Cap::Square => "square",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Cap::ALL.into_iter().find(|c| c.keyword() == s)
    }
}

/// `stroke-linejoin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Join {
    Miter,
    #[default]
    Round,
    Bevel,
}

impl Join {
    pub const ALL: [Join; 3] = [Join::Miter, Join::Round, Join::Bevel];

    pub fn keyword(self) -> &'static str {
        match self {
            Join::Miter => "miter",
            Join::Round => "round",
            Join::Bevel => "bevel",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Join::ALL.into_iter().find(|j| j.keyword() == s)
    }
}

/// `box-shadow` / `drop-shadow`: an offset, softened copy of the shape under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub dx: f64,
    pub dy: f64,
    /// Blur radius in artboard pixels; 0 gives a hard shadow (handy for pixel art).
    pub blur: f64,
    pub color: Color,
}
