//! The interface check: the `ui` pieces of a board answer the pointer by themselves, the way
//! their Godot controls will. Buttons play their `Hover` and `Press` animations, a check box
//! flips, a slider and a scroll bar follow a drag, and every value can also be set by hand.

use crate::{Animation, Document, ShapeKind, UiPart, UiRole};
use kurbo::{Point, Rect, Shape, Vec2};

/// The animations that set each state, as the UI kit reads them: hover, pressed, disabled.
const STATES: [&[&str]; 3] = [&["hover"], &["press", "pressed", "down"], &["disabled"]];
/// The animation of the normal look, if the board has one.
const NORMAL: &[&str] = &["normal", "idle"];

fn named<'d>(doc: &'d Document, names: &[&str]) -> Option<&'d Animation> {
    doc.animations.iter().find(|a| names.contains(&a.name.to_lowercase().as_str()))
}

/// The piece (`ui`) node `k` is a part of: its nearest ancestor with `ui`.
fn owner(doc: &Document, k: usize) -> Option<usize> {
    let mut up = doc.nodes[k].parent;
    while let Some(p) = up {
        if doc.nodes[p].ui {
            return Some(p);
        }
        up = doc.nodes[p].parent;
    }
    None
}

/// Node `k` and everything inside it.
fn within(doc: &Document, k: usize, i: usize) -> bool {
    k == i || doc.is_inside(k, i)
}

/// The box of node `i` and its children on the artboard, without bones.
fn piece_box(doc: &Document, i: usize) -> Option<Rect> {
    (0..doc.nodes.len())
        .filter(|&k| within(doc, k, i) && doc.nodes[k].kind != ShapeKind::Bone)
        .filter_map(|k| Some(doc.world_transform(k).transform_rect_bbox(doc.nodes[k].path()?.bounding_box())))
        .reduce(|a, b| a.union(b))
}

/// One piece under check.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub node: String,
    /// What it works as: a `ui` piece without a role is a button when an animation of a state
    /// changes it, a panel when none does.
    pub role: UiRole,
    pub hover: bool,
    pub pressed: bool,
    pub disabled: bool,
    /// Check boxes: whether the check mark shows.
    pub checked: Option<bool>,
    /// Progress bars, sliders and scroll bars: where they stand, 0 to 1.
    pub value: Option<f64>,
    /// How many times it was clicked.
    pub clicks: u32,
    /// How far the animation of each state has played in, 0 to 1: hover, pressed, disabled.
    shown: [f64; 3],
    /// A piece drawn like an animated one plays that one's states, as both share one style in
    /// Godot: the node each key is on there, and the node of this piece it goes to.
    lend: Vec<(String, String)>,
}

impl Control {
    /// Whether the pointer works it: a panel, a label and a progress bar only show.
    pub fn interactive(&self) -> bool {
        matches!(self.role, UiRole::Button | UiRole::Check | UiRole::Slider | UiRole::Scroll)
    }

    /// The state it shows, in words.
    pub fn state_word(&self) -> &'static str {
        if self.disabled {
            "недоступно"
        } else if self.pressed {
            "нажатие"
        } else if self.hover {
            "наведение"
        } else {
            "обычное"
        }
    }
}

/// Where a slider or a scroll bar runs: its track, which way, and how long its grabber is.
struct Run {
    track: Rect,
    horizontal: bool,
    /// The grabber's length along the track.
    size: f64,
    /// Scroll bars: how far the grabber keeps from the ends of the track.
    inset: f64,
}

impl Run {
    fn start(&self) -> f64 {
        (if self.horizontal { self.track.x0 } else { self.track.y0 }) + self.inset
    }

    /// How far the grabber's start can travel.
    fn travel(&self) -> f64 {
        let len = if self.horizontal { self.track.width() } else { self.track.height() };
        (len - 2.0 * self.inset - self.size).max(1e-9)
    }

    /// The grabber's centre along the track at `v`. A vertical slider has 0 at the bottom, as in Godot.
    fn center(&self, v: f64, slider: bool) -> f64 {
        let u = if slider && !self.horizontal { 1.0 - v } else { v };
        self.start() + self.size / 2.0 + u * self.travel()
    }

    /// The value with the grabber's centre at `at` along the track.
    fn value_at(&self, at: f64, slider: bool) -> f64 {
        let u = ((at - self.start() - self.size / 2.0) / self.travel()).clamp(0.0, 1.0);
        if slider && !self.horizontal { 1.0 - u } else { u }
    }

    fn along(&self, p: Point) -> f64 {
        if self.horizontal { p.x } else { p.y }
    }
}

/// The pieces of a board and what the pointer did to them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiCheck {
    pub controls: Vec<Control>,
    /// The control the button went down on.
    grab: Option<usize>,
    /// Where on the grabber it was taken, from the grabber's centre along the track.
    hold: f64,
    down: bool,
}

impl UiCheck {
    /// The pieces of `doc` as they are drawn: values and check marks are read from the drawing.
    pub fn new(doc: &Document) -> UiCheck {
        let mut rigged = doc.clone();
        rigged.rig();
        let doc = &rigged;
        let mut controls = Vec::new();
        let pieces: Vec<usize> = (0..doc.nodes.len()).filter(|&i| doc.nodes[i].ui && doc.nodes[i].part.is_none() && doc.is_visible(i)).collect();
        let moves =
            |i: usize| STATES.iter().filter_map(|s| named(doc, s)).flat_map(|a| &a.tracks).any(|t| doc.index_of(&t.node).is_some_and(|k| within(doc, k, i)));
        let animated: Vec<usize> = pieces.iter().copied().filter(|&i| moves(i)).collect();
        for &i in &pieces {
            let n = &doc.nodes[i];
            let lend = if animated.contains(&i) {
                Vec::new()
            } else {
                animated
                    .iter()
                    .find_map(|&a| alike(doc, a, i))
                    .map(|pairs| pairs.into_iter().map(|(a, b)| (doc.nodes[a].id.clone(), doc.nodes[b].id.clone())).collect())
                    .unwrap_or_default()
            };
            let role = match n.ui_role {
                UiRole::Auto if animated.contains(&i) || !lend.is_empty() => UiRole::Button,
                UiRole::Auto => UiRole::Panel,
                r => r,
            };
            let mut c = Control {
                node: n.id.clone(),
                role,
                hover: false,
                pressed: false,
                disabled: false,
                checked: None,
                value: None,
                clicks: 0,
                shown: [0.0; 3],
                lend,
            };
            match role {
                UiRole::Check => c.checked = Some(part(doc, i, UiPart::Checked).is_some_and(|k| shows(doc, k))),
                UiRole::Progress => c.value = progress_value(doc, i),
                UiRole::Slider | UiRole::Scroll => {
                    c.value = run(doc, i, role).and_then(|r| {
                        let g = piece_box(doc, part(doc, i, UiPart::Grabber)?)?;
                        Some(r.value_at(r.along(g.center()), role == UiRole::Slider))
                    })
                }
                _ => {}
            }
            controls.push(c);
        }
        UiCheck { controls, ..UiCheck::default() }
    }

    /// The control on node `id`.
    pub fn control(&self, id: &str) -> Option<&Control> {
        self.controls.iter().find(|c| c.node == id)
    }

    /// Passes the pointer: `at` is where it is in artboard space (`None` off the board), `down`
    /// whether the main button is held, `shown` the document as it is drawn now. Returns the
    /// control that was clicked or changed and what happened to it, in words.
    pub fn pointer(&mut self, shown: &Document, at: Option<Point>, down: bool) -> Option<(String, String)> {
        let over = |c: &Control| at.is_some_and(|p| shown.index_of(&c.node).is_some_and(|i| shown.hits_tree(i, p)));
        let pressed = down && !self.down;
        let released = !down && self.down;
        self.down = down;
        // The topmost control under the pointer takes it.
        let top = self.controls.iter().rposition(|c| c.interactive() && !c.disabled && over(c));
        if pressed {
            self.grab = top;
            self.hold = 0.0;
            if let (Some(k), Some(p)) = (top, at) {
                let c = &self.controls[k];
                if matches!(c.role, UiRole::Slider | UiRole::Scroll)
                    && let Some(i) = shown.index_of(&c.node)
                    && let Some(r) = run(shown, i, c.role)
                    && let Some(g) = part(shown, i, UiPart::Grabber).and_then(|g| piece_box(shown, g))
                    && g.contains(p)
                {
                    // Taken on the grabber: it keeps under the pointer where it was taken.
                    self.hold = r.along(p) - r.along(g.center());
                }
            }
        }
        let mut said = None;
        for k in 0..self.controls.len() {
            let held = self.grab == Some(k);
            let c = &mut self.controls[k];
            c.hover = top == Some(k) || (held && down);
            c.pressed = held && down && (top == Some(k) || matches!(c.role, UiRole::Slider | UiRole::Scroll));
        }
        if let (Some(k), Some(p)) = (self.grab, at)
            && down
        {
            let c = &self.controls[k];
            if matches!(c.role, UiRole::Slider | UiRole::Scroll)
                && let Some(i) = shown.index_of(&c.node)
                && let Some(r) = run(shown, i, c.role)
            {
                let v = (r.value_at(r.along(p) - self.hold, c.role == UiRole::Slider) * 1000.0).round() / 1000.0;
                let c = &mut self.controls[k];
                if c.value != Some(v) {
                    c.value = Some(v);
                    said = Some((c.node.clone(), format!("{:.0}%", v * 100.0)));
                }
            }
        }
        if released {
            if let Some(k) = self.grab.filter(|&k| top == Some(k)) {
                let c = &mut self.controls[k];
                c.clicks += 1;
                if let Some(on) = &mut c.checked {
                    *on = !*on;
                    said = Some((c.node.clone(), if *on { "включён" } else { "выключен" }.to_string()));
                } else if c.role == UiRole::Button {
                    said = Some((c.node.clone(), format!("нажата ({})", c.clicks)));
                }
            }
            self.grab = None;
        }
        said
    }

    /// Whether a control the pointer works is under `at`.
    pub fn over_control(&self, shown: &Document, at: Point) -> bool {
        self.controls.iter().any(|c| c.interactive() && !c.disabled && shown.index_of(&c.node).is_some_and(|i| shown.hits_tree(i, at)))
    }

    /// Plays the animations of the states on by `dt` seconds: in while a state is on, back out
    /// when it goes off. Returns whether anything is still moving.
    pub fn advance(&mut self, doc: &Document, dt: f64) -> bool {
        let mut moving = false;
        for c in &mut self.controls {
            for (s, names) in STATES.iter().enumerate() {
                let on = [c.hover, c.pressed, c.disabled][s];
                let target = if on { 1.0 } else { 0.0 };
                let len = named(doc, names).map_or(0.0, |a| a.duration);
                let step = if len > 0.0 { dt / len } else { 1.0 };
                let v = &mut c.shown[s];
                *v = if *v < target { (*v + step).min(target) } else { (*v - step).max(target) };
                moving |= *v != target;
            }
        }
        moving
    }

    /// `base` with every control drawn as it is now: the keys of its state's animation on its
    /// own nodes, its check mark, its grabber and fill where its value puts them. `scene` is
    /// the document the animations are read from.
    pub fn pose(&self, scene: &Document, base: &Document) -> Document {
        let mut out = base.clone();
        let normal = named(scene, NORMAL);
        for c in &self.controls {
            let Some(i) = out.index_of(&c.node) else { continue };
            let mut keys = |a: &Animation, t: f64| {
                for track in &a.tracks {
                    let target = if c.lend.is_empty() {
                        out.index_of(&track.node).filter(|&k| within(&out, k, i))
                    } else {
                        c.lend.iter().find(|(from, _)| *from == track.node).and_then(|(_, to)| out.index_of(to))
                    };
                    let Some(k) = target else { continue };
                    if let Some(v) = track.sample(a.local_time(t)) {
                        out.nodes[k].set(track.prop, v);
                    }
                }
            };
            if let Some(a) = normal {
                keys(a, if a.looping { 0.0 } else { a.duration });
            }
            // The highest state that shows wins: disabled, then pressed, then hover.
            // A state without an animation looks like the one below it.
            if let Some((s, a)) = (0..3).rev().filter(|&s| c.shown[s] > 0.0).find_map(|s| Some((s, named(scene, STATES[s])?))) {
                keys(a, c.shown[s] * a.duration);
            }
            if let Some(on) = c.checked {
                for (p, show) in [(UiPart::Checked, on), (UiPart::Unchecked, !on)] {
                    if let Some(k) = part(&out, i, p) {
                        out.nodes[k].visible = show;
                    }
                }
            }
            if let Some(v) = c.value {
                out.rig();
                match c.role {
                    UiRole::Progress => set_progress(&mut out, i, v),
                    UiRole::Slider | UiRole::Scroll => set_run(&mut out, i, c.role, v),
                    _ => {}
                }
            }
        }
        out
    }
}

/// The nodes of piece `b` matched to those of piece `a`, when `b` is drawn like `a`: the same
/// shapes in the same places inside, whatever the captions say and wherever the pieces stand.
fn alike(doc: &Document, a: usize, b: usize) -> Option<Vec<(usize, usize)>> {
    let tree = |r: usize| (0..doc.nodes.len()).filter(|&k| within(doc, k, r)).collect::<Vec<_>>();
    let (ta, tb) = (tree(a), tree(b));
    if ta.len() != tb.len() {
        return None;
    }
    let plain = |k: usize, root: bool| {
        let mut n = doc.nodes[k].clone();
        n.id.clear();
        n.name = None;
        n.parent = None;
        n.deformed = None;
        if n.kind == ShapeKind::Text {
            n.text = None;
            n.geometry = Default::default();
            (n.x, n.width, n.height) = (0.0, 0.0, 0.0);
        }
        if root {
            (n.x, n.y, n.offset) = (0.0, 0.0, Vec2::ZERO);
        }
        n
    };
    let place = |t: &[usize], k: usize| doc.nodes[k].parent.and_then(|p| t.iter().position(|&q| q == p));
    for (&x, &y) in ta.iter().zip(&tb) {
        if plain(x, x == a) != plain(y, y == b) || (x != a && place(&ta, x) != place(&tb, y)) {
            return None;
        }
    }
    Some(ta.into_iter().zip(tb).collect())
}

/// The part `p` of piece `i`.
fn part(doc: &Document, i: usize, p: UiPart) -> Option<usize> {
    (0..doc.nodes.len()).find(|&k| doc.nodes[k].part == Some(p) && owner(doc, k) == Some(i))
}

fn shows(doc: &Document, k: usize) -> bool {
    doc.is_visible(k) && doc.nodes[k].opacity > 0.0
}

/// The track of slider or scroll bar `i`, from how it is drawn.
fn run(doc: &Document, i: usize, role: UiRole) -> Option<Run> {
    let track = piece_box(doc, part(doc, i, UiPart::Track)?)?;
    let g = piece_box(doc, part(doc, i, UiPart::Grabber)?)?;
    let horizontal = track.width() >= track.height();
    let size = if horizontal { g.width() } else { g.height() };
    // A scroll bar's thumb keeps as far from the ends as from the sides.
    let inset = if role == UiRole::Scroll {
        let (thick, own) = if horizontal { (track.height(), g.height()) } else { (track.width(), g.width()) };
        ((thick - own) / 2.0).max(0.0)
    } else {
        0.0
    };
    Some(Run { track, horizontal, size, inset })
}

/// The shortest a fill can be: its rounded ends.
fn least(doc: &Document, k: usize, horizontal: bool, b: Rect) -> f64 {
    let n = &doc.nodes[k];
    let ends = if n.kind == ShapeKind::Rect { 2.0 * n.radius } else { 0.0 };
    ends.min(if horizontal { b.width() } else { b.height() })
}

/// Where a progress bar's fill runs: the start it is drawn at, how far it can reach, and the
/// shortest it can be. The fill keeps the inset it is drawn with at both ends.
fn progress_run(doc: &Document, i: usize) -> Option<(usize, bool, f64, f64, f64)> {
    let bg = piece_box(doc, part(doc, i, UiPart::Background)?)?;
    let k = part(doc, i, UiPart::Fill)?;
    let fill = piece_box(doc, k)?;
    let horizontal = bg.width() >= bg.height();
    let min = least(doc, k, horizontal, fill);
    Some(if horizontal {
        let inset = (fill.x0 - bg.x0).max(0.0);
        (k, true, fill.x0, bg.width() - 2.0 * inset, min)
    } else {
        // A vertical bar fills from the bottom.
        let inset = (bg.y1 - fill.y1).max(0.0);
        (k, false, fill.y1, bg.height() - 2.0 * inset, min)
    })
}

fn progress_value(doc: &Document, i: usize) -> Option<f64> {
    let (k, horizontal, _, full, min) = progress_run(doc, i)?;
    let b = piece_box(doc, k)?;
    let have = if horizontal { b.width() } else { b.height() };
    Some(((have - min) / (full - min).max(1e-9)).clamp(0.0, 1.0))
}

/// Moves node `k` by `d` on the artboard.
fn shift(doc: &mut Document, k: usize, d: Vec2) {
    let parent = doc.nodes[k].parent.map_or(kurbo::Affine::IDENTITY, |p| doc.world_transform(p));
    let inv = parent.inverse();
    let local = inv * Point::new(d.x, d.y) - inv * Point::ZERO;
    doc.nodes[k].x += local.x;
    doc.nodes[k].y += local.y;
}

/// Stretches node `k` (a rect or an ellipse) along one axis so its box runs from `from` to `to`.
fn span(doc: &mut Document, k: usize, horizontal: bool, from: f64, to: f64) {
    let Some(b) = piece_box(doc, k) else { return };
    let (have, want) = if horizontal { (b.width(), to - from) } else { (b.height(), to - from) };
    if have <= 1e-9 || want <= 1e-9 {
        doc.nodes[k].visible = want > 1e-9 && doc.nodes[k].visible;
        return;
    }
    let n = &mut doc.nodes[k];
    if horizontal {
        n.width *= want / have;
    } else {
        n.height *= want / have;
    }
    let Some(b) = piece_box(doc, k) else { return };
    let d = if horizontal { Vec2::new(from - b.x0, 0.0) } else { Vec2::new(0.0, from - b.y0) };
    shift(doc, k, d);
}

fn set_progress(doc: &mut Document, i: usize, v: f64) {
    let Some((k, horizontal, start, full, min)) = progress_run(doc, i) else { return };
    let len = min + v * (full - min);
    if horizontal {
        span(doc, k, true, start, start + len);
    } else {
        span(doc, k, false, start - len, start);
    }
}

fn set_run(doc: &mut Document, i: usize, role: UiRole, v: f64) {
    let Some(r) = run(doc, i, role) else { return };
    let slider = role == UiRole::Slider;
    let at = r.center(v, slider);
    if let Some(g) = part(doc, i, UiPart::Grabber)
        && let Some(b) = piece_box(doc, g)
    {
        let d = if r.horizontal { Vec2::new(at - b.center().x, 0.0) } else { Vec2::new(0.0, at - b.center().y) };
        shift(doc, g, d);
    }
    // A slider's fill runs from where the value starts to the grabber's centre.
    if slider && let Some(k) = part(doc, i, UiPart::Fill) {
        if r.horizontal {
            span(doc, k, true, r.track.x0, at.max(r.track.x0 + 1e-6));
        } else {
            span(doc, k, false, at.min(r.track.y1 - 1e-6), r.track.y1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Keyframe, Prop, Track, Value};

    fn node(id: &str, kind: ShapeKind, parent: Option<usize>, x: f64, w: f64, h: f64) -> crate::Node {
        let mut n = crate::Node::new(id, kind);
        n.parent = parent;
        n.x = x;
        n.width = w;
        n.height = h;
        n
    }

    /// A slider 200 wide at x 100 with its grabber at the middle, and a button with a Hover.
    fn board() -> Document {
        let mut d = Document::new(crate::Artboard::default());
        let mut s = node("volume", ShapeKind::Group, None, 100.0, 0.0, 0.0);
        s.ui = true;
        s.ui_role = UiRole::Slider;
        d.nodes.push(s);
        let mut t = node("rail", ShapeKind::Rect, Some(0), 0.0, 200.0, 10.0);
        t.part = Some(UiPart::Track);
        d.nodes.push(t);
        let mut f = node("level", ShapeKind::Rect, Some(0), -50.0, 100.0, 10.0);
        f.part = Some(UiPart::Fill);
        d.nodes.push(f);
        let mut g = node("knob", ShapeKind::Ellipse, Some(0), 0.0, 20.0, 20.0);
        g.part = Some(UiPart::Grabber);
        d.nodes.push(g);
        let mut b = node("play", ShapeKind::Rect, None, 100.0, 80.0, 30.0);
        b.y = 100.0;
        b.ui = true;
        d.nodes.push(b);
        d.animations.push(Animation {
            name: "Hover".into(),
            duration: 0.2,
            looping: false,
            tracks: vec![Track {
                node: "play".into(),
                prop: Prop::Opacity,
                keys: vec![
                    Keyframe { time: 0.0, value: Value::Num(1.0), ease: crate::Ease::Linear },
                    Keyframe { time: 0.2, value: Value::Num(0.5), ease: crate::Ease::Linear },
                ],
            }],
            events: Vec::new(),
        });
        d
    }

    #[test]
    fn values_are_read_from_the_drawing() {
        let check = UiCheck::new(&board());
        let v = check.control("volume").unwrap();
        assert_eq!(v.role, UiRole::Slider);
        assert!((v.value.unwrap() - 0.5).abs() < 1e-9, "{v:?}");
        // A piece a state animation changes is a button.
        assert_eq!(check.control("play").unwrap().role, UiRole::Button);
    }

    #[test]
    fn a_drag_moves_the_grabber_and_the_fill() {
        let doc = board();
        let mut check = UiCheck::new(&doc);
        let mut shown = check.pose(&doc, &doc);
        shown.rig();
        // Take the grabber 4 px right of its centre and drag to x 170.
        check.pointer(&shown, Some(Point::new(104.0, 0.0)), true);
        let said = check.pointer(&shown, Some(Point::new(170.0, 0.0)), true);
        assert!(said.is_some_and(|s| s.0 == "volume"));
        let v = check.control("volume").unwrap().value.unwrap();
        // The centre goes to 166 of a run from 10 to 190: (166 − 10) / 180.
        assert!((v - 156.0 / 180.0).abs() < 2e-3, "{v}");
        let mut out = check.pose(&doc, &doc);
        out.rig();
        let knob = piece_box(&out, 3).unwrap();
        assert!((knob.center().x - 166.0).abs() < 0.5, "{knob:?}");
        let fill = piece_box(&out, 2).unwrap();
        assert!((fill.x0 - 0.0).abs() < 1e-6 && (fill.x1 - 166.0).abs() < 0.5, "{fill:?}");
        check.pointer(&out, Some(Point::new(170.0, 0.0)), false);
        assert!(!check.control("volume").unwrap().pressed);
    }

    #[test]
    fn hover_plays_in_and_out() {
        let doc = board();
        let mut check = UiCheck::new(&doc);
        let at = Some(Point::new(100.0, 100.0));
        check.pointer(&doc, at, false);
        assert!(check.control("play").unwrap().hover);
        check.advance(&doc, 0.1);
        let half = check.pose(&doc, &doc);
        assert!((half.node("play").unwrap().opacity - 0.75).abs() < 1e-6);
        check.advance(&doc, 0.2);
        assert!((check.pose(&doc, &doc).node("play").unwrap().opacity - 0.5).abs() < 1e-6);
        // A click counts; the pointer gone, the hover plays back out.
        check.pointer(&doc, at, true);
        assert!(check.control("play").unwrap().pressed);
        let said = check.pointer(&doc, at, false);
        assert_eq!(said, Some(("play".into(), "нажата (1)".into())));
        check.pointer(&doc, None, false);
        assert!(check.advance(&doc, 0.1));
        assert!(!check.advance(&doc, 0.2));
        assert!((check.pose(&doc, &doc).node("play").unwrap().opacity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_piece_drawn_alike_plays_the_same_states() {
        let mut doc = board();
        let mut quit = doc.nodes[4].clone();
        quit.id = "quit".into();
        quit.y = 160.0;
        doc.nodes.push(quit);
        let mut odd = doc.nodes[4].clone();
        odd.id = "odd".into();
        odd.y = 220.0;
        odd.width = 60.0;
        doc.nodes.push(odd);
        let mut check = UiCheck::new(&doc);
        assert_eq!(check.control("quit").unwrap().role, UiRole::Button);
        assert_eq!(check.control("odd").unwrap().role, UiRole::Panel);
        check.pointer(&doc, Some(Point::new(100.0, 160.0)), false);
        check.advance(&doc, 0.3);
        let shown = check.pose(&doc, &doc);
        assert!((shown.node("quit").unwrap().opacity - 0.5).abs() < 1e-6);
        assert!((shown.node("play").unwrap().opacity - 1.0).abs() < 1e-6);
    }
}
