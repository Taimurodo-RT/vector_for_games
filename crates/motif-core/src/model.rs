use crate::Color;
use kurbo::{Affine, BezPath, Ellipse, ParamCurveNearest, Point, RoundedRect, Shape, Vec2};

/// The whole scene: one artboard, its nodes in paint order, and its animations.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub artboard: Artboard,
    /// Nodes in paint order (parents always come before their children).
    pub nodes: Vec<Node>,
    pub animations: Vec<Animation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Artboard {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub background: Color,
}

impl Default for Artboard {
    fn default() -> Self {
        Artboard { name: "Main".into(), width: 480.0, height: 360.0, background: Color::WHITE }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Group,
    Rect,
    Ellipse,
    Star,
    Polygon,
    /// Free-form outline drawn with the pen: `path id at x y d "M 0 0 C …"`.
    Path,
}

impl ShapeKind {
    pub fn keyword(self) -> &'static str {
        match self {
            ShapeKind::Group => "group",
            ShapeKind::Rect => "rect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Star => "star",
            ShapeKind::Polygon => "polygon",
            ShapeKind::Path => "path",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Some(match s {
            "group" => ShapeKind::Group,
            "rect" => ShapeKind::Rect,
            "ellipse" => ShapeKind::Ellipse,
            "star" => ShapeKind::Star,
            "polygon" => ShapeKind::Polygon,
            "path" => ShapeKind::Path,
            _ => return None,
        })
    }
}

/// Every property that can be set from the scene script, the inspector or an animation track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Prop {
    X,
    Y,
    Width,
    Height,
    Rotation,
    ScaleX,
    ScaleY,
    Opacity,
    Radius,
    Points,
    Inner,
    Fill,
    StrokeColor,
    StrokeWidth,
}

impl Prop {
    pub const ALL: [Prop; 14] = [
        Prop::X,
        Prop::Y,
        Prop::Width,
        Prop::Height,
        Prop::Rotation,
        Prop::ScaleX,
        Prop::ScaleY,
        Prop::Opacity,
        Prop::Radius,
        Prop::Points,
        Prop::Inner,
        Prop::Fill,
        Prop::StrokeColor,
        Prop::StrokeWidth,
    ];

    /// Name used in `node.prop` paths of the scene script (`ball.y`, `star.rot`).
    pub fn path_name(self) -> &'static str {
        match self {
            Prop::X => "x",
            Prop::Y => "y",
            Prop::Width => "w",
            Prop::Height => "h",
            Prop::Rotation => "rot",
            Prop::ScaleX => "sx",
            Prop::ScaleY => "sy",
            Prop::Opacity => "opacity",
            Prop::Radius => "r",
            Prop::Points => "points",
            Prop::Inner => "inner",
            Prop::Fill => "fill",
            Prop::StrokeColor => "stroke",
            Prop::StrokeWidth => "stroke_w",
        }
    }

    pub fn from_path_name(s: &str) -> Option<Self> {
        Prop::ALL.into_iter().find(|p| p.path_name() == s).or(match s {
            "rotation" => Some(Prop::Rotation),
            "width" => Some(Prop::Width),
            "height" => Some(Prop::Height),
            "radius" => Some(Prop::Radius),
            _ => None,
        })
    }

    pub fn is_color(self) -> bool {
        matches!(self, Prop::Fill | Prop::StrokeColor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Num(f64),
    Color(Color),
    /// No paint (`fill none`).
    None,
}

impl Value {
    pub fn as_num(self) -> Option<f64> {
        match self {
            Value::Num(v) => Some(v),
            _ => None,
        }
    }

    pub fn lerp(self, other: Value, t: f64) -> Value {
        match (self, other) {
            (Value::Num(a), Value::Num(b)) => Value::Num(a + (b - a) * t),
            (Value::Color(a), Value::Color(b)) => Value::Color(a.lerp(b, t)),
            _ if t < 1.0 => self,
            _ => other,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// Stable id used by the script, animations and the game runtime.
    pub id: String,
    /// Optional display name shown in the layers panel.
    pub name: Option<String>,
    pub kind: ShapeKind,
    pub parent: Option<usize>,
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Degrees, clockwise.
    pub rotation: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub opacity: f64,
    pub radius: f64,
    pub points: f64,
    pub inner: f64,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f64,
    /// Outline of a `Path` node in local space (empty for other kinds).
    pub geometry: BezPath,
}

impl Node {
    pub fn new(id: impl Into<String>, kind: ShapeKind) -> Self {
        let (w, h) = if kind == ShapeKind::Group { (0.0, 0.0) } else { (100.0, 100.0) };
        Node {
            id: id.into(),
            name: None,
            kind,
            parent: None,
            visible: true,
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            opacity: 1.0,
            radius: 0.0,
            points: if kind == ShapeKind::Polygon { 6.0 } else { 5.0 },
            inner: 0.45,
            fill: if kind == ShapeKind::Group { None } else { Some(Color::rgb(0xcc, 0xcc, 0xcc)) },
            stroke: None,
            stroke_width: 0.0,
            geometry: BezPath::new(),
        }
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    pub fn get(&self, prop: Prop) -> Value {
        let paint = |c: Option<Color>| c.map_or(Value::None, Value::Color);
        match prop {
            Prop::X => Value::Num(self.x),
            Prop::Y => Value::Num(self.y),
            Prop::Width => Value::Num(self.width),
            Prop::Height => Value::Num(self.height),
            Prop::Rotation => Value::Num(self.rotation),
            Prop::ScaleX => Value::Num(self.scale_x),
            Prop::ScaleY => Value::Num(self.scale_y),
            Prop::Opacity => Value::Num(self.opacity),
            Prop::Radius => Value::Num(self.radius),
            Prop::Points => Value::Num(self.points),
            Prop::Inner => Value::Num(self.inner),
            Prop::Fill => paint(self.fill),
            Prop::StrokeColor => paint(self.stroke),
            Prop::StrokeWidth => Value::Num(self.stroke_width),
        }
    }

    /// Sets a property; a value of the wrong type is ignored.
    pub fn set(&mut self, prop: Prop, value: Value) {
        let paint = match value {
            Value::Color(c) => Some(Some(c)),
            Value::None => Some(None),
            Value::Num(_) => None,
        };
        if prop.is_color() {
            if let Some(p) = paint {
                if prop == Prop::Fill { self.fill = p } else { self.stroke = p }
            }
            return;
        }
        let Value::Num(v) = value else { return };
        match prop {
            Prop::X => self.x = v,
            Prop::Y => self.y = v,
            Prop::Width => self.width = v,
            Prop::Height => self.height = v,
            Prop::Rotation => self.rotation = v,
            Prop::ScaleX => self.scale_x = v,
            Prop::ScaleY => self.scale_y = v,
            Prop::Opacity => self.opacity = v,
            Prop::Radius => self.radius = v,
            Prop::Points => self.points = v,
            Prop::Inner => self.inner = v,
            Prop::StrokeWidth => self.stroke_width = v,
            Prop::Fill | Prop::StrokeColor => unreachable!(),
        }
    }

    /// Transform from this node's local space into its parent's space.
    /// The local origin is the node's center, which is also the pivot for rotation and scale.
    pub fn local_transform(&self) -> Affine {
        Affine::translate(Vec2::new(self.x, self.y)) * Affine::rotate(self.rotation.to_radians()) * Affine::scale_non_uniform(self.scale_x, self.scale_y)
    }

    /// Geometry in local space, centered on the origin. Groups have none.
    pub fn path(&self) -> Option<BezPath> {
        let (w, h) = (self.width.abs(), self.height.abs());
        let rect = kurbo::Rect::new(-w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
        Some(match self.kind {
            ShapeKind::Group => return None,
            ShapeKind::Rect => {
                let r = self.radius.clamp(0.0, w.min(h) / 2.0);
                RoundedRect::from_rect(rect, r).to_path(0.1)
            }
            ShapeKind::Ellipse => Ellipse::from_rect(rect).to_path(0.1),
            ShapeKind::Star => star_path(w, h, self.points.round().max(3.0) as usize, self.inner.clamp(0.05, 1.0)),
            ShapeKind::Polygon => polygon_path(w, h, self.points.round().max(3.0) as usize),
            ShapeKind::Path => self.geometry.clone(),
        })
    }
}

fn star_path(w: f64, h: f64, points: usize, inner: f64) -> BezPath {
    let mut path = BezPath::new();
    for i in 0..points * 2 {
        let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / points as f64;
        let r = if i % 2 == 0 { 1.0 } else { inner };
        let p = Point::new(a.cos() * w / 2.0 * r, a.sin() * h / 2.0 * r);
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    path.close_path();
    path
}

fn polygon_path(w: f64, h: f64, sides: usize) -> BezPath {
    let mut path = BezPath::new();
    for i in 0..sides {
        let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::TAU / sides as f64;
        let p = Point::new(a.cos() * w / 2.0, a.sin() * h / 2.0);
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    path.close_path();
    path
}

impl Document {
    pub fn new(artboard: Artboard) -> Self {
        Document { artboard, nodes: Vec::new(), animations: Vec::new() }
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Transform from a node's local space into artboard space.
    pub fn world_transform(&self, index: usize) -> Affine {
        let node = &self.nodes[index];
        let local = node.local_transform();
        match node.parent {
            Some(p) => self.world_transform(p) * local,
            None => local,
        }
    }

    /// Effective opacity including all parent groups.
    pub fn world_opacity(&self, index: usize) -> f64 {
        let node = &self.nodes[index];
        node.opacity * node.parent.map_or(1.0, |p| self.world_opacity(p))
    }

    pub fn is_visible(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        node.visible && node.parent.is_none_or(|p| self.is_visible(p))
    }

    /// Depth in the hierarchy, 0 for top-level nodes.
    pub fn depth(&self, index: usize) -> usize {
        self.nodes[index].parent.map_or(0, |p| self.depth(p) + 1)
    }

    /// Topmost visible shape under an artboard-space point.
    pub fn hit_test(&self, point: Point) -> Option<usize> {
        (0..self.nodes.len()).rev().find(|&i| {
            let node = &self.nodes[i];
            if !self.is_visible(i) {
                return false;
            }
            let Some(path) = node.path() else { return false };
            let local = self.world_transform(i).inverse() * point;
            path.contains(local) || (node.stroke.is_some() && near_outline(&path, local, node.stroke_width.max(4.0)))
        })
    }

    pub fn animation(&self, name: &str) -> Option<&Animation> {
        self.animations.iter().find(|a| a.name == name)
    }
}

fn near_outline(path: &BezPath, p: Point, width: f64) -> bool {
    path.segments().any(|seg| seg.nearest(p, 0.1).distance_sq <= (width / 2.0).powi(2))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ease {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Hold,
    Spring,
}

impl Ease {
    pub const ALL: [Ease; 6] = [Ease::Linear, Ease::EaseIn, Ease::EaseOut, Ease::EaseInOut, Ease::Hold, Ease::Spring];

    pub fn keyword(self) -> &'static str {
        match self {
            Ease::Linear => "linear",
            Ease::EaseIn => "ease-in",
            Ease::EaseOut => "ease-out",
            Ease::EaseInOut => "ease-in-out",
            Ease::Hold => "hold",
            Ease::Spring => "spring",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Ease::ALL.into_iter().find(|e| e.keyword() == s)
    }

    /// Maps linear progress `u` in 0..=1 to eased progress.
    pub fn apply(self, u: f64) -> f64 {
        match self {
            Ease::Linear => u,
            Ease::EaseIn => u * u * u,
            Ease::EaseOut => 1.0 - (1.0 - u).powi(3),
            Ease::EaseInOut => {
                if u < 0.5 {
                    4.0 * u * u * u
                } else {
                    1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
                }
            }
            Ease::Hold => {
                if u >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Ease::Spring => 1.0 - (u * 4.5 * std::f64::consts::PI).cos() * (-6.0 * u).exp(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    pub time: f64,
    pub value: Value,
    /// Curve used from this key to the next one.
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub node: String,
    pub prop: Prop,
    /// Sorted by time.
    pub keys: Vec<Keyframe>,
}

impl Track {
    pub fn sample(&self, t: f64) -> Option<Value> {
        let first = self.keys.first()?;
        if t <= first.time {
            return Some(first.value);
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t < b.time {
                let span = (b.time - a.time).max(1e-9);
                return Some(a.value.lerp(b.value, a.ease.apply((t - a.time) / span)));
            }
        }
        self.keys.last().map(|k| k.value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration: f64,
    pub looping: bool,
    pub tracks: Vec<Track>,
}

impl Animation {
    /// Wraps or clamps an unbounded playback time into the animation's range.
    pub fn local_time(&self, t: f64) -> f64 {
        if self.duration <= 0.0 {
            0.0
        } else if self.looping {
            t.rem_euclid(self.duration)
        } else {
            t.clamp(0.0, self.duration)
        }
    }

    /// Returns a copy of the document posed at `t` seconds.
    pub fn pose(&self, doc: &Document, t: f64) -> Document {
        let t = self.local_time(t);
        let mut out = doc.clone();
        for track in &self.tracks {
            if let (Some(i), Some(v)) = (out.index_of(&track.node), track.sample(t)) {
                out.nodes[i].set(track.prop, v);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_sampling_eases_between_keys() {
        let track = Track {
            node: "ball".into(),
            prop: Prop::Y,
            keys: vec![
                Keyframe { time: 0.0, value: Value::Num(0.0), ease: Ease::Linear },
                Keyframe { time: 1.0, value: Value::Num(100.0), ease: Ease::Hold },
                Keyframe { time: 2.0, value: Value::Num(50.0), ease: Ease::Linear },
            ],
        };
        assert_eq!(track.sample(-1.0), Some(Value::Num(0.0)));
        assert_eq!(track.sample(0.5), Some(Value::Num(50.0)));
        assert_eq!(track.sample(1.5), Some(Value::Num(100.0)));
        assert_eq!(track.sample(3.0), Some(Value::Num(50.0)));
    }

    #[test]
    fn hit_test_respects_hierarchy_and_order() {
        let mut doc = Document::new(Artboard::default());
        let mut group = Node::new("g", ShapeKind::Group);
        group.x = 100.0;
        doc.nodes.push(group);
        let mut a = Node::new("a", ShapeKind::Rect);
        a.parent = Some(0);
        a.width = 20.0;
        a.height = 20.0;
        doc.nodes.push(a);
        let mut b = Node::new("b", ShapeKind::Ellipse);
        b.x = 105.0;
        b.width = 20.0;
        b.height = 20.0;
        doc.nodes.push(b);
        assert_eq!(doc.hit_test(Point::new(105.0, 0.0)), Some(2));
        assert_eq!(doc.hit_test(Point::new(92.0, 0.0)), Some(1));
        assert_eq!(doc.hit_test(Point::new(0.0, 0.0)), None);
    }

    #[test]
    fn looping_time_wraps() {
        let anim = Animation { name: "a".into(), duration: 2.0, looping: true, tracks: vec![] };
        assert!((anim.local_time(5.0) - 1.0).abs() < 1e-9);
        let once = Animation { looping: false, ..anim };
        assert_eq!(once.local_time(5.0), 2.0);
    }
}
