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

/// A glow: a soft halo of light around the shape, behind it (no offset, unlike a shadow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    /// How far the light reaches past the edge, in artboard units (the blur radius).
    pub size: f64,
    /// How far the halo is grown before it is softened: a brighter, wider core.
    pub spread: f64,
    pub color: Color,
}

impl Glow {
    pub const DEFAULT: Glow = Glow { size: 0.0, spread: 0.0, color: Color { r: 0xff, g: 0xd3, b: 0x4d, a: 0xcc } };
}

/// Film grain over the fill: light and dark specks that make flat colour look printed or worn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    /// 0..1, how strong the specks are.
    pub amount: f64,
    /// The side of one speck, in artboard units.
    pub size: f64,
}

impl Noise {
    pub const DEFAULT: Noise = Noise { amount: 0.0, size: 1.0 };
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

impl Shadow {
    /// A soft dark shadow, used when a shadow is added without one being written.
    pub const DEFAULT: Shadow = Shadow { dx: 0.0, dy: 0.0, blur: 0.0, color: Color { r: 0, g: 0, b: 0, a: 0x55 } };

    /// This shadow, or [`Shadow::DEFAULT`], moved and softened for an object raised `elevation`
    /// above the artboard and lit so shadows fall toward `light` degrees.
    pub fn cast(shadow: Option<Shadow>, elevation: f64, light: f64) -> Option<Shadow> {
        if elevation <= 0.0 {
            return shadow;
        }
        let mut s = shadow.unwrap_or(Shadow::DEFAULT);
        let a = light.to_radians();
        s.dx += a.cos() * elevation;
        s.dy += a.sin() * elevation;
        s.blur += elevation * 0.6;
        Some(s)
    }
}
