//! Animation curves: the CSS keywords, the 30 named functions from easings.net, and custom ones.

use std::f64::consts::PI;

/// The families of easings.net, each in three directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Sine,
    Quad,
    Cubic,
    Quart,
    Quint,
    Expo,
    Circ,
    Back,
    Elastic,
    Bounce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    In,
    Out,
    InOut,
}

impl Family {
    pub const ALL: [Family; 10] =
        [Family::Sine, Family::Quad, Family::Cubic, Family::Quart, Family::Quint, Family::Expo, Family::Circ, Family::Back, Family::Elastic, Family::Bounce];

    pub fn name(self) -> &'static str {
        NAMES[self as usize][0].trim_start_matches("ease-in-")
    }
}

impl Dir {
    pub const ALL: [Dir; 3] = [Dir::In, Dir::Out, Dir::InOut];
}

/// Script keywords of the named functions, by family and direction.
const NAMES: [[&str; 3]; 10] = [
    ["ease-in-sine", "ease-out-sine", "ease-in-out-sine"],
    ["ease-in-quad", "ease-out-quad", "ease-in-out-quad"],
    ["ease-in-cubic", "ease-out-cubic", "ease-in-out-cubic"],
    ["ease-in-quart", "ease-out-quart", "ease-in-out-quart"],
    ["ease-in-quint", "ease-out-quint", "ease-in-out-quint"],
    ["ease-in-expo", "ease-out-expo", "ease-in-out-expo"],
    ["ease-in-circ", "ease-out-circ", "ease-in-out-circ"],
    ["ease-in-back", "ease-out-back", "ease-in-out-back"],
    ["ease-in-elastic", "ease-out-elastic", "ease-in-out-elastic"],
    ["ease-in-bounce", "ease-out-bounce", "ease-in-out-bounce"],
];

/// Cubic Bézier approximations from easings.net (elastic and bounce have none).
const HANDLES: [[[f64; 4]; 3]; 8] = [
    [[0.12, 0.0, 0.39, 0.0], [0.61, 1.0, 0.88, 1.0], [0.37, 0.0, 0.63, 1.0]],
    [[0.11, 0.0, 0.5, 0.0], [0.5, 1.0, 0.89, 1.0], [0.45, 0.0, 0.55, 1.0]],
    [[0.32, 0.0, 0.67, 0.0], [0.33, 1.0, 0.68, 1.0], [0.65, 0.0, 0.35, 1.0]],
    [[0.5, 0.0, 0.75, 0.0], [0.25, 1.0, 0.5, 1.0], [0.76, 0.0, 0.24, 1.0]],
    [[0.64, 0.0, 0.78, 0.0], [0.22, 1.0, 0.36, 1.0], [0.83, 0.0, 0.17, 1.0]],
    [[0.7, 0.0, 0.84, 0.0], [0.16, 1.0, 0.3, 1.0], [0.87, 0.0, 0.13, 1.0]],
    [[0.55, 0.0, 1.0, 0.45], [0.0, 0.55, 0.45, 1.0], [0.85, 0.0, 0.15, 1.0]],
    [[0.36, 0.0, 0.66, -0.56], [0.34, 1.56, 0.64, 1.0], [0.68, -0.6, 0.32, 1.6]],
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    /// CSS `ease`: a gentle start and a long soft stop.
    Css,
    EaseIn,
    EaseOut,
    EaseInOut,
    Hold,
    Spring,
    /// One of the easings.net functions, `ease-out-bounce` and the like.
    Named(Family, Dir),
    /// A custom curve like CSS `cubic-bezier(x1, y1, x2, y2)`: handles from (0,0) and to (1,1).
    Cubic([f64; 4]),
    /// Jumps in N equal steps, like CSS `steps(N)`: handy for pixel-art timing.
    Steps(u32),
}

impl Ease {
    /// The curves without parameters, in menu order.
    pub const BASIC: [Ease; 7] = [Ease::Linear, Ease::Css, Ease::EaseIn, Ease::EaseOut, Ease::EaseInOut, Ease::Hold, Ease::Spring];

    pub fn keyword(self) -> &'static str {
        match self {
            Ease::Linear => "linear",
            Ease::Css => "ease",
            Ease::EaseIn => "ease-in",
            Ease::EaseOut => "ease-out",
            Ease::EaseInOut => "ease-in-out",
            Ease::Hold => "hold",
            Ease::Spring => "spring",
            Ease::Named(f, d) => NAMES[f as usize][d as usize],
            Ease::Cubic(_) => "cubic",
            Ease::Steps(_) => "steps",
        }
    }

    /// How the curve is written after a key: the keyword, plus the numbers of `cubic` and `steps`.
    pub fn script(self) -> String {
        match self {
            Ease::Cubic(c) => format!("cubic {}", c.map(|v| format!("{}", (v * 100.0).round() / 100.0)).join(" ")),
            Ease::Steps(n) => format!("steps {n}"),
            e => e.keyword().to_string(),
        }
    }

    /// A curve without parameters by its keyword.
    pub fn from_keyword(s: &str) -> Option<Self> {
        if let Some(e) = Ease::BASIC.into_iter().find(|e| e.keyword() == s) {
            return Some(e);
        }
        Family::ALL.into_iter().flat_map(|f| Dir::ALL.map(|d| Ease::Named(f, d))).find(|e| e.keyword() == s)
    }

    /// The handles of the curve as a cubic Bézier, where it has one (or a close approximation).
    pub fn handles(self) -> Option<[f64; 4]> {
        Some(match self {
            Ease::Linear => [0.0, 0.0, 1.0, 1.0],
            Ease::Css => [0.25, 0.1, 0.25, 1.0],
            Ease::EaseIn => HANDLES[2][0],
            Ease::EaseOut => HANDLES[2][1],
            Ease::EaseInOut => HANDLES[2][2],
            Ease::Named(f, d) if (f as usize) < HANDLES.len() => HANDLES[f as usize][d as usize],
            Ease::Cubic(c) => c,
            _ => return None,
        })
    }

    /// Maps linear progress `u` in 0..=1 to eased progress.
    pub fn apply(self, u: f64) -> f64 {
        match self {
            Ease::Linear => u,
            Ease::Css => bezier([0.25, 0.1, 0.25, 1.0], u),
            Ease::EaseIn => named(Family::Cubic, Dir::In, u),
            Ease::EaseOut => named(Family::Cubic, Dir::Out, u),
            Ease::EaseInOut => named(Family::Cubic, Dir::InOut, u),
            Ease::Hold => {
                if u >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Ease::Spring => 1.0 - (u * 4.5 * PI).cos() * (-6.0 * u).exp(),
            Ease::Named(f, d) => named(f, d, u),
            Ease::Cubic(c) => bezier(c, u),
            Ease::Steps(n) => {
                let n = n.max(1) as f64;
                if u >= 1.0 { 1.0 } else { (u * n).floor() / n }
            }
        }
    }
}

/// CSS `cubic-bezier`: y at the point of the curve whose x is `u`.
fn bezier([x1, y1, x2, y2]: [f64; 4], u: f64) -> f64 {
    if u <= 0.0 || u >= 1.0 {
        return u.clamp(0.0, 1.0);
    }
    let bez = |a: f64, b: f64, t: f64| 3.0 * a * t * (1.0 - t).powi(2) + 3.0 * b * t * t * (1.0 - t) + t * t * t;
    // x(t) rises from 0 to 1 when x1, x2 are in 0..=1: find t with x(t) = u by bisection.
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < u {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

/// The easings.net formulas.
fn named(f: Family, d: Dir, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ease_in = |x: f64| -> f64 {
        match f {
            Family::Sine => 1.0 - (x * PI / 2.0).cos(),
            Family::Quad => x.powi(2),
            Family::Cubic => x.powi(3),
            Family::Quart => x.powi(4),
            Family::Quint => x.powi(5),
            Family::Expo => 2f64.powf(10.0 * x - 10.0),
            Family::Circ => 1.0 - (1.0 - x * x).max(0.0).sqrt(),
            Family::Back => {
                let c1 = 1.70158;
                (c1 + 1.0) * x.powi(3) - c1 * x * x
            }
            Family::Elastic => {
                let c4 = 2.0 * PI / 3.0;
                -(2f64.powf(10.0 * x - 10.0)) * ((10.0 * x - 10.75) * c4).sin()
            }
            Family::Bounce => 1.0 - bounce_out(1.0 - x),
        }
    };
    match d {
        Dir::In => ease_in(x),
        Dir::Out => 1.0 - ease_in(1.0 - x),
        Dir::InOut => {
            if x < 0.5 {
                ease_in(2.0 * x) / 2.0
            } else {
                1.0 - ease_in(2.0 - 2.0 * x) / 2.0
            }
        }
    }
}

fn bounce_out(x: f64) -> f64 {
    let (n1, d1) = (7.5625, 2.75);
    if x < 1.0 / d1 {
        n1 * x * x
    } else if x < 2.0 / d1 {
        let x = x - 1.5 / d1;
        n1 * x * x + 0.75
    } else if x < 2.5 / d1 {
        let x = x - 2.25 / d1;
        n1 * x * x + 0.9375
    } else {
        let x = x - 2.625 / d1;
        n1 * x * x + 0.984375
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_curve_starts_and_ends_in_place() {
        for f in Family::ALL {
            for d in Dir::ALL {
                let e = Ease::Named(f, d);
                assert_eq!(Ease::from_keyword(e.keyword()), Some(e));
                assert!(e.apply(0.0).abs() < 1e-9 && (e.apply(1.0) - 1.0).abs() < 1e-9, "{}", e.keyword());
                // Near the ends the curve is near 0 and 1 (no jumps).
                assert!(e.apply(0.001).abs() < 0.05 && (e.apply(0.999) - 1.0).abs() < 0.05, "{}", e.keyword());
            }
        }
        // In-out curves are symmetric around the middle.
        assert!((Ease::Named(Family::Sine, Dir::InOut).apply(0.5) - 0.5).abs() < 1e-9);
        // Back overshoots, bounce does not leave 0..=1.
        assert!(Ease::Named(Family::Back, Dir::Out).apply(0.7) > 1.0);
        assert!((0..=100).map(|i| Ease::Named(Family::Bounce, Dir::Out).apply(i as f64 / 100.0)).all(|v| (0.0..=1.0).contains(&v)));
        assert_eq!(Ease::Steps(4).apply(0.3), 0.25);
        assert_eq!(Ease::from_keyword("ease"), Some(Ease::Css));
    }
}
