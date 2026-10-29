//! Bounded CSS animation timing and keyframes. Values remain CSS until the importer
//! turns each supported property into ordinary editable Motif keys.
use crate::css;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Timing {
    Linear,
    Cubic(f64, f64, f64, f64),
    Steps(u32, StepPosition),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StepPosition {
    Start,
    End,
    None,
    Both,
}

impl Default for Timing {
    fn default() -> Self {
        Self::Cubic(0.25, 0.1, 0.25, 1.0)
    }
}

impl Timing {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value.trim() {
            "linear" => Self::Linear,
            "ease" => Self::default(),
            "ease-in" => Self::Cubic(0.42, 0.0, 1.0, 1.0),
            "ease-out" => Self::Cubic(0.0, 0.0, 0.58, 1.0),
            "ease-in-out" => Self::Cubic(0.42, 0.0, 0.58, 1.0),
            "step-start" => Self::Steps(1, StepPosition::Start),
            "step-end" => Self::Steps(1, StepPosition::End),
            value => {
                if let Some(body) = value.strip_prefix("cubic-bezier(").and_then(|v| v.strip_suffix(')')) {
                    let nums: Vec<_> = css::split(body, ',').iter().map(|v| v.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
                    if nums.len() != 4 || nums.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&nums[0]) || !(0.0..=1.0).contains(&nums[2]) {
                        return None;
                    }
                    Self::Cubic(nums[0], nums[1], nums[2], nums[3])
                } else {
                    let body = value.strip_prefix("steps(").and_then(|v| v.strip_suffix(')'))?;
                    let parts = css::split(body, ',');
                    if !(1..=2).contains(&parts.len()) {
                        return None;
                    }
                    let n = parts[0].trim().parse::<u32>().ok().filter(|v| (1..=1000).contains(v))?;
                    let pos = match parts.get(1).map_or("end", |v| v.trim()) {
                        "start" | "jump-start" => StepPosition::Start,
                        "end" | "jump-end" => StepPosition::End,
                        "jump-none" if n > 1 => StepPosition::None,
                        "jump-both" => StepPosition::Both,
                        _ => return None,
                    };
                    Self::Steps(n, pos)
                }
            }
        })
    }

    pub(crate) fn sample(self, input: f64) -> f64 {
        let x = input.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::Cubic(x1, y1, x2, y2) => {
                if x == 0.0 || x == 1.0 {
                    return x;
                }
                let curve = |t: f64, a: f64, b: f64| 3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
                let (mut low, mut high) = (0.0, 1.0);
                for _ in 0..40 {
                    let mid = (low + high) * 0.5;
                    if curve(mid, x1, x2) < x {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                curve((low + high) * 0.5, y1, y2)
            }
            Self::Steps(n, pos) => {
                let count = n as f64;
                let floor = (x * count).floor();
                match pos {
                    StepPosition::Start => ((floor + 1.0) / count).min(1.0),
                    StepPosition::End => (floor / count).min(1.0),
                    StepPosition::None => (floor / (count - 1.0)).clamp(0.0, 1.0),
                    StepPosition::Both => ((floor + 1.0) / (count + 1.0)).clamp(0.0, 1.0),
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Direction {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Fill {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnimationSpec {
    pub name: String,
    pub duration: f64,
    pub delay: f64,
    /// None means infinite. A finite number may include a partial final iteration.
    pub iterations: Option<f64>,
    pub direction: Direction,
    pub fill: Fill,
    pub timing: Timing,
    pub paused: bool,
}
impl Default for AnimationSpec {
    fn default() -> Self {
        Self {
            name: "none".into(),
            duration: 0.0,
            delay: 0.0,
            iterations: Some(1.0),
            direction: Direction::Normal,
            fill: Fill::None,
            timing: Timing::default(),
            paused: false,
        }
    }
}
impl AnimationSpec {
    pub(crate) fn end_time(&self) -> Option<f64> {
        self.iterations.map(|n| self.delay + self.duration * n)
    }
    pub(crate) fn period(&self) -> f64 {
        self.duration * if matches!(self.direction, Direction::Alternate | Direction::AlternateReverse) { 2.0 } else { 1.0 }
    }
    fn directed(&self, iteration: f64, progress: f64) -> f64 {
        let odd = iteration.rem_euclid(2.0) >= 1.0;
        let reverse = match self.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => odd,
            Direction::AlternateReverse => !odd,
        };
        if reverse { 1.0 - progress } else { progress }
    }
    /// Directed iteration progress, before per-keyframe easing. None means this animation
    /// has no effect and the underlying state (or an earlier animation) is used.
    pub(crate) fn progress(&self, time: f64) -> Option<f64> {
        let time = if self.paused { 0.0 } else { time };
        if time < self.delay {
            return matches!(self.fill, Fill::Backwards | Fill::Both).then(|| self.directed(0.0, 0.0));
        }
        let elapsed = time - self.delay;
        let total = self.iterations.map(|n| self.duration * n);
        if self.duration <= 0.0 || total.is_some_and(|t| elapsed >= t) {
            if !matches!(self.fill, Fill::Forwards | Fill::Both) {
                return None;
            }
            let n = self.iterations.unwrap_or(1.0);
            if n <= 0.0 {
                return Some(self.directed(0.0, 0.0));
            }
            let whole = n.floor();
            let (iteration, progress) = if (n - whole).abs() < 1e-12 { (whole - 1.0, 1.0) } else { (whole, n - whole) };
            return Some(self.directed(iteration, progress));
        }
        let position = elapsed / self.duration;
        Some(self.directed(position.floor(), position.fract()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TransitionSpec {
    pub property: String,
    pub duration: f64,
    pub delay: f64,
    pub timing: Timing,
}
impl Default for TransitionSpec {
    fn default() -> Self {
        Self { property: "all".into(), duration: 0.0, delay: 0.0, timing: Timing::default() }
    }
}
impl TransitionSpec {
    pub(crate) fn progress(&self, time: f64) -> f64 {
        if time < self.delay {
            0.0
        } else if self.duration <= 0.0 {
            1.0
        } else {
            self.timing.sample((time - self.delay) / self.duration)
        }
    }
    pub(crate) fn end_time(&self) -> f64 {
        (self.delay + self.duration).max(0.0)
    }
    pub(crate) fn applies_to(&self, property: &str) -> bool {
        self.property == "all"
            || self.property == property
            || self.property == "background" && property.starts_with("background-")
            || self.property == "border" && property.starts_with("border-")
            || self.property == "-webkit-transform" && property == "transform"
    }
}

fn time(value: &str) -> Option<f64> {
    let (number, k) = if let Some(v) = value.strip_suffix("ms") { (v, 0.001) } else { (value.strip_suffix('s')?, 1.0) };
    let n = number.parse::<f64>().ok()? * k;
    n.is_finite().then_some(n)
}
fn ident(value: &str) -> bool {
    !value.is_empty() && !value.starts_with(|c: char| c.is_ascii_digit()) && value.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn animation_shorthand(value: &str, warnings: &mut Vec<String>) -> Option<AnimationSpec> {
    let mut spec = AnimationSpec::default();
    let mut times = 0;
    let mut named = false;
    let mut timing = false;
    let mut direction = false;
    let mut fill = false;
    let mut iterations = false;
    let mut play = false;
    for word in css::words(value) {
        if let Some(t) = time(word) {
            match times {
                0 if t >= 0.0 => spec.duration = t,
                1 => spec.delay = t,
                _ => return None,
            };
            times += 1;
        } else if let Some(t) = Timing::parse(word) {
            if timing {
                return None;
            }
            spec.timing = t;
            timing = true;
        } else if ["normal", "reverse", "alternate", "alternate-reverse"].contains(&word) {
            if direction {
                return None;
            }
            spec.direction = match word {
                "reverse" => Direction::Reverse,
                "alternate" => Direction::Alternate,
                "alternate-reverse" => Direction::AlternateReverse,
                _ => Direction::Normal,
            };
            direction = true;
        } else if ["none", "forwards", "backwards", "both"].contains(&word) {
            if fill {
                return None;
            }
            spec.fill = match word {
                "none" => Fill::None,
                "forwards" => Fill::Forwards,
                "backwards" => Fill::Backwards,
                _ => Fill::Both,
            };
            fill = true;
        } else if word == "infinite" || word.parse::<f64>().is_ok() {
            if iterations {
                return None;
            }
            spec.iterations = if word == "infinite" { None } else { Some(word.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0)?) };
            iterations = true;
        } else if ["paused", "running"].contains(&word) {
            if play {
                return None;
            }
            spec.paused = word == "paused";
            play = true;
        } else if !named && (ident(word) || word.starts_with(['\'', '"']) && word.ends_with(['\'', '"'])) {
            spec.name = word.trim_matches(['\'', '"']).into();
            named = true;
        } else {
            warnings.push(format!("CSS animation: значение {word} не поддерживается"));
            return None;
        }
    }
    Some(spec)
}
fn transition_shorthand(value: &str) -> Option<TransitionSpec> {
    let mut spec = TransitionSpec::default();
    let mut times = 0;
    let mut property = false;
    let mut timing = false;
    for word in css::words(value) {
        if let Some(t) = time(word) {
            match times {
                0 if t >= 0.0 => spec.duration = t,
                1 => spec.delay = t,
                _ => return None,
            };
            times += 1;
        } else if let Some(t) = Timing::parse(word) {
            if timing {
                return None;
            }
            spec.timing = t;
            timing = true;
        } else if !property && ident(word) {
            spec.property = word.into();
            property = true;
        } else {
            return None;
        }
    }
    Some(spec)
}
fn timing_text(t: Timing) -> String {
    match t {
        Timing::Linear => "linear".into(),
        Timing::Cubic(a, b, c, d) => format!("cubic-bezier({a},{b},{c},{d})"),
        Timing::Steps(n, p) => format!(
            "steps({n},{})",
            match p {
                StepPosition::Start => "start",
                StepPosition::End => "end",
                StepPosition::None => "jump-none",
                StepPosition::Both => "jump-both",
            }
        ),
    }
}
pub(crate) fn expand_shorthand(key: &str, value: &str, warnings: &mut Vec<String>) -> Option<Vec<(String, String)>> {
    if key == "animation" {
        let specs: Vec<_> = css::split(value, ',').iter().map(|v| animation_shorthand(v.trim(), warnings)).collect::<Option<_>>()?;
        let join = |f: fn(&AnimationSpec) -> String| specs.iter().map(f).collect::<Vec<_>>().join(", ");
        Some(vec![
            ("animation-name".into(), join(|s| format!("\"{}\"", s.name.replace('\\', "\\\\").replace('"', "\\\"")))),
            ("animation-duration".into(), join(|s| format!("{}s", s.duration))),
            ("animation-delay".into(), join(|s| format!("{}s", s.delay))),
            ("animation-timing-function".into(), join(|s| timing_text(s.timing))),
            ("animation-iteration-count".into(), join(|s| s.iterations.map_or("infinite".into(), |n| n.to_string()))),
            (
                "animation-direction".into(),
                join(|s| {
                    match s.direction {
                        Direction::Normal => "normal",
                        Direction::Reverse => "reverse",
                        Direction::Alternate => "alternate",
                        Direction::AlternateReverse => "alternate-reverse",
                    }
                    .into()
                }),
            ),
            (
                "animation-fill-mode".into(),
                join(|s| {
                    match s.fill {
                        Fill::None => "none",
                        Fill::Forwards => "forwards",
                        Fill::Backwards => "backwards",
                        Fill::Both => "both",
                    }
                    .into()
                }),
            ),
            ("animation-play-state".into(), join(|s| if s.paused { "paused" } else { "running" }.into())),
        ])
    } else if key == "transition" {
        let specs: Vec<_> = css::split(value, ',').iter().map(|v| transition_shorthand(v.trim())).collect::<Option<_>>()?;
        if specs.len() > 1 && specs.iter().any(|s| s.property == "none") {
            return None;
        }
        let join = |f: fn(&TransitionSpec) -> String| specs.iter().map(f).collect::<Vec<_>>().join(", ");
        Some(vec![
            ("transition-property".into(), join(|s| s.property.clone())),
            ("transition-duration".into(), join(|s| format!("{}s", s.duration))),
            ("transition-delay".into(), join(|s| format!("{}s", s.delay))),
            ("transition-timing-function".into(), join(|s| timing_text(s.timing))),
        ])
    } else {
        None
    }
}

fn expanded(declarations: &BTreeMap<String, String>, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for key in ["animation", "transition"] {
        if let Some(value) = declarations.get(key) {
            if let Some(props) = expand_shorthand(key, value, warnings) {
                result.extend(props);
            } else {
                warnings.push(format!("CSS {key}: неподдерживаемое или неверное значение {value}"));
            }
        }
    }
    result.extend(declarations.iter().filter(|(key, _)| !["animation", "transition"].contains(&key.as_str())).map(|(k, v)| (k.clone(), v.clone())));
    result
}
fn list<'a>(declarations: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> Vec<&'a str> {
    css::split(declarations.get(key).map_or(default, String::as_str), ',').into_iter().map(str::trim).collect()
}
fn cycle<'a>(values: &[&'a str], i: usize) -> &'a str {
    values[i % values.len()]
}
pub(crate) fn parse_animations(declarations: &BTreeMap<String, String>, warnings: &mut Vec<String>) -> Vec<AnimationSpec> {
    let d = expanded(declarations, warnings);
    let names = list(&d, "animation-name", "none");
    let durations = list(&d, "animation-duration", "0s");
    let delays = list(&d, "animation-delay", "0s");
    let timings = list(&d, "animation-timing-function", "ease");
    let counts = list(&d, "animation-iteration-count", "1");
    let directions = list(&d, "animation-direction", "normal");
    let fills = list(&d, "animation-fill-mode", "none");
    let play = list(&d, "animation-play-state", "running");
    let mut out = Vec::new();
    for (i, name) in names.into_iter().enumerate().take(32) {
        let parsed = (|| {
            Some(AnimationSpec {
                name: name.trim_matches(['\'', '"']).into(),
                duration: time(cycle(&durations, i)).filter(|v| *v >= 0.0)?,
                delay: time(cycle(&delays, i))?,
                timing: Timing::parse(cycle(&timings, i))?,
                iterations: if cycle(&counts, i) == "infinite" {
                    None
                } else {
                    Some(cycle(&counts, i).parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0)?)
                },
                direction: match cycle(&directions, i) {
                    "normal" => Direction::Normal,
                    "reverse" => Direction::Reverse,
                    "alternate" => Direction::Alternate,
                    "alternate-reverse" => Direction::AlternateReverse,
                    _ => return None,
                },
                fill: match cycle(&fills, i) {
                    "none" => Fill::None,
                    "forwards" => Fill::Forwards,
                    "backwards" => Fill::Backwards,
                    "both" => Fill::Both,
                    _ => return None,
                },
                paused: match cycle(&play, i) {
                    "running" => false,
                    "paused" => true,
                    _ => return None,
                },
            })
        })();
        match parsed {
            Some(spec) if spec.name != "none" => out.push(spec),
            Some(_) => {}
            None => warnings.push(format!("CSS animation {name}: неверные параметры анимации")),
        }
    }
    if list(&d, "animation-name", "none").len() > 32 {
        warnings.push("CSS animation: импортированы первые 32 анимации элемента".into());
    }
    out
}
pub(crate) fn parse_transitions(declarations: &BTreeMap<String, String>, warnings: &mut Vec<String>) -> Vec<TransitionSpec> {
    let d = expanded(declarations, warnings);
    let properties = list(&d, "transition-property", "all");
    let durations = list(&d, "transition-duration", "0s");
    let delays = list(&d, "transition-delay", "0s");
    let timings = list(&d, "transition-timing-function", "ease");
    let mut out = Vec::new();
    for (i, property) in properties.into_iter().enumerate().take(32) {
        let parsed = (|| {
            Some(TransitionSpec {
                property: property.into(),
                duration: time(cycle(&durations, i)).filter(|v| *v >= 0.0)?,
                delay: time(cycle(&delays, i))?,
                timing: Timing::parse(cycle(&timings, i))?,
            })
        })();
        match parsed {
            Some(spec) if spec.property != "none" => out.push(spec),
            Some(_) => {}
            None => warnings.push(format!("CSS transition {property}: неверные параметры перехода")),
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Keyframe {
    pub offset: f64,
    pub declarations: BTreeMap<String, String>,
    pub timing: Option<Timing>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Keyframes {
    pub frames: Vec<Keyframe>,
}
pub(crate) struct PropertySample<'a> {
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub mix: f64,
}
impl Keyframes {
    pub(crate) fn properties(&self) -> BTreeSet<&str> {
        self.frames.iter().flat_map(|f| f.declarations.keys().map(String::as_str)).collect()
    }
    pub(crate) fn offsets(&self) -> Vec<f64> {
        self.frames.iter().map(|f| f.offset).collect()
    }
    pub(crate) fn property_sample<'a>(&'a self, property: &str, progress: f64, timing: Timing) -> PropertySample<'a> {
        let p = progress.clamp(0.0, 1.0);
        let matching: Vec<_> = self.frames.iter().filter(|f| f.declarations.contains_key(property)).collect();
        let before = matching.iter().rev().find(|f| f.offset <= p).copied();
        let after = matching.iter().find(|f| f.offset >= p).copied();
        let (a, b) = (before.map_or(0.0, |f| f.offset), after.map_or(1.0, |f| f.offset));
        let from = before.and_then(|f| f.declarations.get(property)).map(String::as_str);
        let to = after.and_then(|f| f.declarations.get(property)).map(String::as_str);
        let mix = if (b - a).abs() < 1e-12 { 0.0 } else { before.and_then(|f| f.timing).unwrap_or(timing).sample((p - a) / (b - a)) };
        PropertySample { from, to, mix }
    }
}

pub(crate) fn parse_keyframes(body: &str, warnings: &mut Vec<String>) -> Result<Keyframes, String> {
    let mut frames: Vec<Keyframe> = Vec::new();
    let mut rest = body;
    while !rest.trim().is_empty() {
        let Some(open) = rest.find('{') else {
            warnings.push("CSS keyframes: текст вне блока пропущен".into());
            break;
        };
        let Some(end) = css::block_end(rest, open) else {
            return Err("CSS keyframes: незакрытый блок".into());
        };
        let decls = css::declarations(&rest[open + 1..end], warnings);
        let mut values = BTreeMap::new();
        let mut timing = None;
        for (key, value, important) in decls {
            if important {
                warnings.push(format!("CSS keyframes {key}: !important пропущен согласно CSS"));
                continue;
            }
            if key == "animation-timing-function" {
                timing = Timing::parse(&value);
                if timing.is_none() {
                    warnings.push(format!("CSS keyframes: неверная timing-function {value}"));
                }
            } else if key.starts_with("animation-") || key.starts_with("transition-") {
                warnings.push(format!("CSS keyframes: {key} пропущен"));
            } else {
                values.insert(key, value);
            }
        }
        for selector in css::split(rest[..open].trim(), ',') {
            let offset = match selector.trim() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                s => s.strip_suffix('%').and_then(|n| n.parse::<f64>().ok()).map(|n| n / 100.0),
            };
            let Some(offset) = offset.filter(|p| p.is_finite() && (0.0..=1.0).contains(p)) else {
                warnings.push(format!("CSS keyframes: неверная позиция {}", selector.trim()));
                continue;
            };
            if let Some(frame) = frames.iter_mut().find(|f| (f.offset - offset).abs() < 1e-12) {
                frame.declarations.extend(values.clone());
                if timing.is_some() {
                    frame.timing = timing;
                }
            } else {
                frames.push(Keyframe { offset, declarations: values.clone(), timing });
            }
        }
        if frames.len() > 256 {
            return Err("CSS keyframes: не больше 256 опор на анимацию".into());
        }
        rest = &rest[end + 1..];
    }
    frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    Ok(Keyframes { frames })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
    #[test]
    fn uiverse_animation_and_transition_lists_preserve_delays_and_composition_order() {
        let mut warnings = Vec::new();
        let specs = parse_animations(&d(&[("animation", "0.1s ease-in-out 0.05s both changeColor8, 0.2s linear 0.4s both lightEffect8")]), &mut warnings);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "changeColor8");
        assert_eq!(specs[0].delay, 0.05);
        assert_eq!(specs[1].delay, 0.4);
        assert_eq!(specs[1].timing, Timing::Linear);
        assert_eq!(specs[1].fill, Fill::Both);
        let arrow = parse_animations(&d(&[("animation", "0.6s ease-in-out both infinite alternate rightArrow8")]), &mut warnings).remove(0);
        assert_eq!(arrow.iterations, None);
        assert_eq!(arrow.period(), 1.2);
        assert_eq!(arrow.progress(0.3), Some(0.5));
        assert!((arrow.progress(0.9).unwrap() - 0.5).abs() < 1e-12);
        let transition = parse_transitions(&d(&[("transition", "left 200ms ease-out 50ms, background-color 0.1s linear")]), &mut warnings);
        assert_eq!(transition.len(), 2);
        assert_eq!(transition[0].duration, 0.2);
        assert_eq!(transition[0].delay, 0.05);
        assert!(transition[0].applies_to("left"));
        assert!(!transition[0].applies_to("top"));
        assert_eq!(transition[1].timing, Timing::Linear);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
    #[test]
    fn fill_direction_fractional_iterations_and_negative_delay_are_distinct() {
        let mut spec =
            AnimationSpec { duration: 2.0, delay: 1.0, iterations: Some(2.5), direction: Direction::Alternate, fill: Fill::Both, ..Default::default() };
        assert_eq!(spec.progress(0.0), Some(0.0));
        assert_eq!(spec.progress(3.0), Some(1.0));
        assert_eq!(spec.progress(4.0), Some(0.5));
        assert_eq!(spec.progress(8.0), Some(0.5));
        assert_eq!(spec.end_time(), Some(6.0));
        spec.fill = Fill::None;
        assert_eq!(spec.progress(0.0), None);
        assert_eq!(spec.progress(6.0), None);
        spec.delay = -1.0;
        assert_eq!(spec.progress(0.0), Some(0.5));
        spec.direction = Direction::AlternateReverse;
        spec.fill = Fill::Backwards;
        spec.delay = 1.0;
        assert_eq!(spec.progress(0.0), Some(1.0));
    }
    #[test]
    fn keyframes_merge_duplicate_offsets_and_use_underlying_missing_endpoints() {
        let mut warnings = Vec::new();
        let frames = parse_keyframes(
            "from {color:red} 50%, 75% {opacity:.5;animation-timing-function:linear} 50% {color:blue} to {opacity:1;color:green!important}",
            &mut warnings,
        )
        .unwrap();
        assert_eq!(frames.offsets(), vec![0.0, 0.5, 0.75, 1.0]);
        assert_eq!(frames.properties(), BTreeSet::from(["color", "opacity"]));
        let sample = frames.property_sample("opacity", 0.25, Timing::Linear);
        assert_eq!(sample.from, None);
        assert_eq!(sample.to, Some(".5"));
        assert_eq!(sample.mix, 0.5);
        let color = frames.property_sample("color", 0.75, Timing::Linear);
        assert_eq!(color.from, Some("blue"));
        assert_eq!(color.to, None);
        assert_eq!(color.mix, 0.5);
        assert!(warnings.iter().any(|w| w.contains("!important")));
    }
    #[test]
    fn easing_handles_nonlinear_curves_and_steps_at_exact_boundaries() {
        assert_eq!(Timing::default().sample(0.0), 0.0);
        assert_eq!(Timing::default().sample(1.0), 1.0);
        assert!(Timing::default().sample(0.5) > 0.8);
        let symmetric = Timing::parse("cubic-bezier(.42,0,.58,1)").unwrap();
        assert!((symmetric.sample(0.5) - 0.5).abs() < 1e-10);
        assert_eq!(Timing::parse("steps(4,end)").unwrap().sample(0.24), 0.0);
        assert_eq!(Timing::parse("steps(4,end)").unwrap().sample(0.25), 0.25);
        assert_eq!(Timing::parse("step-start").unwrap().sample(0.0), 1.0);
        assert!(Timing::parse("cubic-bezier(-1,0,1,1)").is_none());
    }
    #[test]
    fn longhand_lists_repeat_and_invalid_times_do_not_create_nan_timelines() {
        let mut warnings = Vec::new();
        let specs = parse_animations(
            &d(&[("animation-name", "one,two,three"), ("animation-duration", ".1s,.2s"), ("animation-timing-function", "linear")]),
            &mut warnings,
        );
        assert_eq!(specs.iter().map(|s| s.duration).collect::<Vec<_>>(), vec![0.1, 0.2, 0.1]);
        assert!(warnings.is_empty());
        assert!(parse_animations(&d(&[("animation", "-1s bad")]), &mut warnings).is_empty());
        assert!(parse_transitions(&d(&[("transition-duration", "NaNs")]), &mut warnings).is_empty());
        assert!(!warnings.is_empty());
    }
}
