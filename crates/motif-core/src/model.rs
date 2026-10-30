use crate::{Blend, Cap, Color, Ease, Glow, Join, Noise, Shadow};
use kurbo::{Affine, BezPath, Ellipse, ParamCurveNearest, Point, RoundedRect, Shape, Vec2};

/// The whole scene: one artboard, its nodes in paint order, and its animations.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub artboard: Artboard,
    /// Nodes in paint order (parents always come before their children).
    pub nodes: Vec<Node>,
    pub animations: Vec<Animation>,
    pub parameters: Vec<crate::PoseParameter>,
    pub drivers: Vec<crate::PoseDriver>,
    pub correctives: Vec<crate::PoseCorrective>,
    /// State machines that switch between the animations.
    pub machines: Vec<crate::Machine>,
    /// Direction shadows fall from raised objects (`elevation`), in degrees: 90 is straight down.
    pub light: f64,
    /// Gravity and wind of the scene (`world`).
    pub world: crate::World,
    /// Pulls and fans where they are on the frame being simulated.
    pub(crate) fields: Vec<crate::field::Source>,
    /// Seconds into the animation this pose is from: boiling lines redraw with it.
    pub time: f64,
    /// What the scene is for (`artboard … ui` or `game`): the editor shows the tools for it.
    pub purpose: Purpose,
    /// The sides a character is shot from for a top-down game (`directions 8 turn …`).
    pub directions: Option<crate::Directions>,
    /// Hidden ids (`id=…`) of inputs, states, animations, events and parameters.
    pub ids: crate::Ids,
    /// Seed the game gives to what is random in the scene (particles, poured fluid, rough
    /// lines), so two copies of one character differ and a replay repeats. 0 is the scene as
    /// Motif shows it.
    pub seed: u64,
}

/// What a scene is made for. The editor hides what the other kind of work needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Purpose {
    /// Everything at hand.
    #[default]
    Any,
    /// Game interface: kit export and states first; physics, bones and 3D out of the way.
    Ui,
    /// Game art and effects: no interface kit.
    Game,
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
    /// Letters as outlines: `text title at x y content "Привет" font-size 32`.
    Text,
    /// A bone of a skeleton: starts at its position, points along its rotation, `len` long.
    /// Shapes inside it move with it; it is drawn only in the editor.
    Bone,
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
            ShapeKind::Text => "text",
            ShapeKind::Bone => "bone",
        }
    }

    /// Group and bone: they hold other nodes and draw nothing of their own.
    pub fn is_container(self) -> bool {
        matches!(self, ShapeKind::Group | ShapeKind::Bone)
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Some(match s {
            "group" => ShapeKind::Group,
            "rect" => ShapeKind::Rect,
            "ellipse" => ShapeKind::Ellipse,
            "star" => ShapeKind::Star,
            "polygon" => ShapeKind::Polygon,
            "path" => ShapeKind::Path,
            "text" => ShapeKind::Text,
            "bone" => ShapeKind::Bone,
            _ => return None,
        })
    }
}

/// Every property that can be set from the scene script, the inspector or an animation track.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Skew in degrees, like CSS `skewX` / `skewY`.
    SkewX,
    SkewY,
    /// Text only.
    FontSize,
    LetterSpacing,
    /// 0..1, letters appear one by one.
    Reveal,
    /// Height of a wave through the letters, and its position (1 = one cycle).
    Wave,
    Phase,
    /// Gaussian blur of the whole object, in artboard units.
    Blur,
    /// Shadow offset and softness.
    ShadowX,
    ShadowY,
    ShadowBlur,
    ShadowColor,
    /// How far the glow reaches.
    Glow,
    /// Inner shadow offset and softness.
    InsetX,
    InsetY,
    InsetBlur,
    InsetColor,
    /// Strength of the film grain, 0..1.
    Noise,
    /// Height above the artboard: casts a shadow away from the scene light, longer and softer when higher.
    Elevation,
    /// Place on a ball: latitude, longitude (degrees) and height above the surface.
    Lat,
    Lon,
    Alt,
    /// Turn of a ball, degrees.
    Yaw,
    Pitch,
    /// Strength of a pull and of a fan.
    Pull,
    Fan,
    /// How far a morph has gone: 0 is the shape itself, 1 its first target, 2 the second.
    Morph,
    /// A body's depth and how much its back narrows.
    Depth,
    Taper,
    /// How far a hand-drawn outline wanders.
    Rough,
    /// The scene's gravity and wind (`world.gravity-y`), keyed on the world, not on a node.
    GravityX,
    GravityY,
    WindX,
    WindY,
    Gust,
    /// Numeric pose control, keyed as `turn.value`; never a drawable node property.
    Parameter,
    WarpX(u8),
    WarpY(u8),
    Contour,
    Visible,
}

impl Prop {
    pub const ALL: [Prop; 64] = [
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
        Prop::SkewX,
        Prop::SkewY,
        Prop::FontSize,
        Prop::LetterSpacing,
        Prop::Reveal,
        Prop::Wave,
        Prop::Phase,
        Prop::Blur,
        Prop::ShadowX,
        Prop::ShadowY,
        Prop::ShadowBlur,
        Prop::ShadowColor,
        Prop::Glow,
        Prop::InsetX,
        Prop::InsetY,
        Prop::InsetBlur,
        Prop::InsetColor,
        Prop::Noise,
        Prop::Elevation,
        Prop::Lat,
        Prop::Lon,
        Prop::Alt,
        Prop::Yaw,
        Prop::Pitch,
        Prop::Pull,
        Prop::Fan,
        Prop::Morph,
        Prop::Depth,
        Prop::Taper,
        Prop::Rough,
        Prop::WarpX(0),
        Prop::WarpY(0),
        Prop::WarpX(1),
        Prop::WarpY(1),
        Prop::WarpX(2),
        Prop::WarpY(2),
        Prop::WarpX(3),
        Prop::WarpY(3),
        Prop::WarpX(4),
        Prop::WarpY(4),
        Prop::WarpX(5),
        Prop::WarpY(5),
        Prop::WarpX(6),
        Prop::WarpY(6),
        Prop::WarpX(7),
        Prop::WarpY(7),
        Prop::WarpX(8),
        Prop::WarpY(8),
        Prop::Contour,
        Prop::Visible,
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
            Prop::SkewX => "kx",
            Prop::SkewY => "ky",
            Prop::FontSize => "font-size",
            Prop::LetterSpacing => "letter-spacing",
            Prop::Reveal => "reveal",
            Prop::Wave => "wave",
            Prop::Phase => "phase",
            Prop::Blur => "blur",
            Prop::ShadowX => "shadow-x",
            Prop::ShadowY => "shadow-y",
            Prop::ShadowBlur => "shadow-blur",
            Prop::ShadowColor => "shadow-color",
            Prop::Glow => "glow",
            Prop::InsetX => "inset-x",
            Prop::InsetY => "inset-y",
            Prop::InsetBlur => "inset-blur",
            Prop::InsetColor => "inset-color",
            Prop::Noise => "noise",
            Prop::Elevation => "elevation",
            Prop::Lat => "lat",
            Prop::Lon => "lon",
            Prop::Alt => "alt",
            Prop::Yaw => "yaw",
            Prop::Pitch => "pitch",
            Prop::Pull => "pull",
            Prop::Fan => "fan",
            Prop::Morph => "morph",
            Prop::Depth => "depth",
            Prop::Taper => "taper",
            Prop::Rough => "rough",
            Prop::GravityX => "gravity-x",
            Prop::GravityY => "gravity-y",
            Prop::WindX => "wind-x",
            Prop::WindY => "wind-y",
            Prop::Gust => "gust",
            Prop::Parameter => "value",
            Prop::WarpX(i) => ["warp-x0", "warp-x1", "warp-x2", "warp-x3", "warp-x4", "warp-x5", "warp-x6", "warp-x7", "warp-x8"]
                .get(i as usize)
                .copied()
                .unwrap_or("invalid-warp"),
            Prop::WarpY(i) => ["warp-y0", "warp-y1", "warp-y2", "warp-y3", "warp-y4", "warp-y5", "warp-y6", "warp-y7", "warp-y8"]
                .get(i as usize)
                .copied()
                .unwrap_or("invalid-warp"),
            Prop::Contour => "contour",
            Prop::Visible => "visible",
        }
    }

    pub fn from_path_name(s: &str) -> Option<Self> {
        Prop::ALL.into_iter().chain(crate::World::PROPS).chain([Prop::Parameter]).find(|p| p.path_name() == s).or(match s {
            "rotation" => Some(Prop::Rotation),
            "width" | "len" => Some(Prop::Width),
            "height" => Some(Prop::Height),
            "radius" => Some(Prop::Radius),
            "skew_x" => Some(Prop::SkewX),
            "skew_y" => Some(Prop::SkewY),
            _ => None,
        })
    }

    /// True for the scene's gravity and wind, keyed as `world.gravity-x`.
    pub fn is_world(self) -> bool {
        crate::World::PROPS.contains(&self)
    }

    pub fn is_color(self) -> bool {
        matches!(self, Prop::Fill | Prop::StrokeColor | Prop::ShadowColor | Prop::InsetColor)
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
            // An absent paint is the transparent endpoint of the other paint, not a
            // discrete type change. Keep its RGB so it never fades through black.
            (Value::None, Value::Color(b)) => {
                if t <= 0.0 {
                    Value::None
                } else if t >= 1.0 {
                    Value::Color(b)
                } else {
                    Value::Color(Color { a: (b.a as f64 * t).round() as u8, ..b })
                }
            }
            (Value::Color(a), Value::None) => {
                if t <= 0.0 {
                    Value::Color(a)
                } else if t >= 1.0 {
                    Value::None
                } else {
                    Value::Color(Color { a: (a.a as f64 * (1.0 - t)).round() as u8, ..a })
                }
            }
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
    /// Gradient fill; when set it paints instead of `fill`.
    pub gradient: Option<Gradient>,
    /// Group only: the first child is a mask that clips the other children and is not drawn.
    pub clip: bool,
    /// Fill with the even-odd rule, so overlapping parts of the outline become holes (SVG `fill-rule`).
    pub even_odd: bool,
    /// Degrees.
    pub skew_x: f64,
    pub skew_y: f64,
    /// Point in local space that rotation, skew and scale turn around (CSS `transform-origin`).
    pub pivot: Vec2,
    /// Rectangle corners clockwise from top left, when they differ (`r 8 8 0 0`).
    pub radii: Option<[f64; 4]>,
    /// Dash and gap lengths of the stroke; empty for a solid line.
    pub dash: Vec<f64>,
    pub cap: Cap,
    pub join: Join,
    pub shadow: Option<Shadow>,
    /// A soft halo of light around the shape (`glow 12 #ffd34d`).
    pub glow: Option<Glow>,
    /// A shadow cast inside the shape from its edge, as if pressed in (CSS `box-shadow: inset`).
    pub inset: Option<Shadow>,
    /// Gradient stroke; when set it paints the stroke instead of `stroke`.
    pub stroke_gradient: Option<Gradient>,
    /// Film grain over the fill (`noise 0.15`).
    pub noise: Option<Noise>,
    pub blend: Blend,
    /// Extra strokes under the shape, widest first: thick dark outside, thin light inside.
    pub outlines: Vec<(Color, f64)>,
    /// Text only: the letters read from the font, and how they are laid out.
    pub text: Option<std::sync::Arc<crate::text::TextShape>>,
    pub text_style: crate::text::TextStyle,
    /// Gaussian blur of the whole object (a group blurs its children together), in artboard units.
    pub blur: f64,
    /// Height above the artboard for the procedural shadow; 0 casts none.
    pub elevation: f64,
    /// Bone only: turn this bone and the ones above it so that its tip reaches a target.
    pub ik: Option<Ik>,
    /// Bends the outline with bones: each point follows the bones near it.
    pub skin: Option<Skin>,
    /// The outline bent by `skin` or by a soft-body simulation; `path` returns it when present.
    pub deformed: Option<BezPath>,
    pub lattice: Option<crate::Lattice>,
    pub forms: Option<crate::Forms>,
    pub(crate) lattice_path: Option<BezPath>,
    /// How the cages of parent groups bend this shape, when they do: `lattice_path` is the
    /// outline before them mapped through it. A renderer may bend shapes cut once instead.
    pub(crate) lattice_map: Option<std::sync::Arc<crate::LatticeMap>>,
    /// A soft body: the outline is simulated as an elastic ring while an animation plays.
    pub soft: Option<crate::Soft>,
    /// Soft bodies cannot pass through this shape.
    pub solid: bool,
    /// Keyed position, size and rotation follow on a spring: they overshoot and settle.
    pub spring: Option<crate::Spring>,
    /// Bone only: the bone lags behind and swings after the motion of the bone it hangs from.
    pub jiggle: Option<crate::Spring>,
    /// Stretches along the motion and squashes on a stop, keeping the area.
    pub squash: Option<crate::Squash>,
    /// Stretch set by the simulation for `squash`, in the parent's space about the pivot.
    pub warp: Option<Affine>,
    /// Sits on this ball (a shape's id): `lat`, `lon` and `alt` place it, `wrap` lays it on
    /// the surface.
    pub on: Option<String>,
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    pub wrap: bool,
    /// As a ball: turn around the vertical axis and tip towards the viewer, in degrees.
    pub yaw: f64,
    pub pitch: f64,
    /// Set by `place_orbits`: the ball this hides behind (`false`) or is clipped to (`true`).
    pub behind: Option<(usize, bool)>,
    /// An emitter: copies of this shape fly out of it while an animation plays.
    pub emit: Option<crate::Emitter>,
    /// Set by the simulation: where each particle is drawn (on the artboard) and its opacity.
    /// The shape itself is drawn only while this is `None`.
    pub particles: Option<Vec<(Affine, f64)>>,
    /// A rope or a cloth: the outline hangs, swings and drapes while an animation plays.
    pub rope: Option<crate::Rope>,
    /// Rope only: objects holding its ends, by id: the end, or the start and the end.
    pub tie: Vec<String>,
    /// Hangs on this rope (an id), at this part of its length: 0 its start, 1 its end.
    pub hang: Option<(String, f64)>,
    /// Drawn as the links of a chain along this rope (an id) instead of itself.
    pub links: Option<String>,
    /// Extra turn in degrees set by the simulation: hanging at a rope's end, rolling on a ball.
    pub swing: f64,
    /// A fluid: the shape fills with water, or pours it with `flow`, while an animation plays.
    pub fluid: Option<crate::Fluid>,
    /// On a ball (`on`): rolls over it under gravity instead of sitting where its keys put it.
    pub roll: Option<crate::Roll>,
    /// How heavy it is for the scene's gravity (`weight 2`): 0 floats. Unset, each simulation
    /// keeps its own (`gravity`).
    pub weight: Option<f64>,
    /// How much wind it catches (`sail 0.5`): 0 ignores it. Unset, 1.
    pub sail: Option<f64>,
    /// Pulls everything simulated towards itself, or pushes it away (`pull 2000 range 300`).
    pub pull: Option<crate::Field>,
    /// Blows along its own direction (`fan 600 range 300 cone 40`).
    pub fan: Option<crate::Field>,
    /// Flows into the shapes of other objects (`morph star heart`) as `morph` goes 0 → 1 → 2.
    pub morph: Option<crate::Morph>,
    /// Pushed into depth as a 3D body (`extrude 60`), turned by `yaw` and `pitch`.
    pub body: Option<crate::Body>,
    /// A piece of interface (`ui`): exported on its own, in each state, for the game's UI.
    pub ui: bool,
    /// The Godot control the piece is styled for (`ui progress`).
    pub ui_role: crate::UiRole,
    /// The part of a composite piece this object draws (`part grabber`).
    pub part: Option<crate::UiPart>,
    /// A slider drawn by its `Value` animation: how the pointer moves it and where it stands.
    pub dial: crate::Dial,
    /// A check box in a group of radio buttons (`radio difficulty`): one of the group is on.
    pub radio: Option<String>,
    /// Locked (`locked`): drawn as usual, but clicks on the canvas pass through it.
    pub locked: bool,
    /// Nine-slice margins (`slice 16`): left, top, right, bottom, in artboard units from the
    /// edges of its box. The corners keep their size when the game stretches it.
    pub slice: Option<[f64; 4]>,
    /// Hand-drawn look: wobbly outline, hatching, grain, banded light.
    pub brush: crate::Brush,
    /// Group only: lays the children out in a row or a column (`stack column center gap 8 pad 12`).
    pub stack: Option<crate::Stack>,
    /// Sized to the stack box of its group and put behind its middle (`fit`): a panel that
    /// grows with what is stacked on it.
    pub fit: bool,
    /// Measured from a point of the artboard, or of its group's box (`anchor right bottom`):
    /// 0 is the left or top edge, 0.5 the middle, 1 the right or bottom edge.
    pub anchor: Option<(f64, f64)>,
    /// Set by `layout`: where a stack, a panel or an anchor moves the object, added to `x`, `y`.
    pub offset: Vec2,
    /// Set by `layout` on a stack: the box of the stacked children with the padding, in local space.
    pub frame: Option<kurbo::Rect>,
}

/// Bones that bend a shape. Every point of the outline follows a mix of the bones: mostly the
/// nearest one, and both of them around a joint, so an arm bends smoothly at the elbow.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub bones: Vec<String>,
    /// How fast a bone's pull fades with distance: higher keeps the bend tight around the joint.
    pub sharp: f64,
    /// The pose the outline was drawn in, set by `Document::bind_skins`.
    pub bind: Vec<SkinBind>,
    /// The shape's own transform into the artboard in that pose.
    pub bind_world: Affine,
    /// Brush strokes over the distance weights, in the order they were painted.
    pub paint: Vec<WeightStroke>,
}

impl Skin {
    pub const SHARP: f64 = 4.0;

    pub fn new(bones: Vec<String>) -> Self {
        Skin { bones, sharp: Skin::SHARP, bind: Vec::new(), bind_world: Affine::IDENTITY, paint: Vec::new() }
    }
}

/// One stroke of the weight brush: near its points the shape follows `bone` more (positive
/// `amount`) or less (negative), fading to nothing at `radius`.
#[derive(Clone, Debug, PartialEq)]
pub struct WeightStroke {
    pub bone: String,
    pub radius: f64,
    /// −1..1: how much of the way to go towards (or away from) the bone at the stroke's centre.
    pub amount: f64,
    /// In the shape's own space, as it was drawn.
    pub points: Vec<Point>,
}

/// One bone of a skin at bind time.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinBind {
    pub bone: String,
    /// From the shape's space into the bone's space.
    pub to_bone: Affine,
    /// The bone from joint to tip, in the shape's space.
    pub a: Point,
    pub b: Point,
}

/// Inverse kinematics on a bone: the chain of `chain` bones ending in this one bends so that
/// its tip reaches the origin of `target`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ik {
    pub target: String,
    pub chain: usize,
    /// Bend the other way (an elbow or a knee can point either side).
    pub flip: bool,
}

/// A two-or-more colour gradient spread over the shape's bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub radial: bool,
    /// Direction of a linear gradient in degrees: 0 left to right, 90 top to bottom.
    pub angle: f64,
    /// Colours evenly spaced from start to end.
    pub colors: Vec<Color>,
}

impl Gradient {
    /// Start and end points of a linear gradient across `bounds` (or centre and radius point
    /// for a radial one), in the same space as `bounds`.
    pub fn line(&self, bounds: kurbo::Rect) -> (Point, Point) {
        let c = bounds.center();
        if self.radial {
            let r = bounds.width().max(bounds.height()) / 2.0;
            return (c, c + Vec2::new(r, 0.0));
        }
        let a = self.angle.to_radians();
        let d = Vec2::new(a.cos(), a.sin());
        // Half the extent of the bounds along the direction, so the colours reach the corners.
        let half = (bounds.width() * d.x.abs() + bounds.height() * d.y.abs()) / 2.0;
        (c - d * half, c + d * half)
    }
}

impl Node {
    pub fn new(id: impl Into<String>, kind: ShapeKind) -> Self {
        let (w, h) = match kind {
            ShapeKind::Group => (0.0, 0.0),
            ShapeKind::Bone => (60.0, 0.0),
            _ => (100.0, 100.0),
        };
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
            fill: if kind.is_container() { None } else { Some(Color::rgb(0xcc, 0xcc, 0xcc)) },
            stroke: None,
            stroke_width: 0.0,
            geometry: BezPath::new(),
            gradient: None,
            clip: false,
            even_odd: false,
            skew_x: 0.0,
            skew_y: 0.0,
            pivot: Vec2::ZERO,
            radii: None,
            dash: Vec::new(),
            cap: Cap::default(),
            join: Join::default(),
            shadow: None,
            glow: None,
            inset: None,
            stroke_gradient: None,
            noise: None,
            blend: Blend::default(),
            outlines: Vec::new(),
            text: None,
            text_style: crate::text::TextStyle {
                size: 32.0,
                spacing: 0.0,
                line_height: 1.2,
                align: crate::text::TextAlign::Center,
                reveal: 1.0,
                wave: 0.0,
                phase: 0.0,
            },
            blur: 0.0,
            elevation: 0.0,
            ik: None,
            skin: None,
            deformed: None,
            lattice: None,
            forms: None,
            lattice_path: None,
            lattice_map: None,
            soft: None,
            solid: false,
            spring: None,
            jiggle: None,
            squash: None,
            warp: None,
            on: None,
            lat: 0.0,
            lon: 0.0,
            alt: 0.0,
            wrap: false,
            yaw: 0.0,
            pitch: 0.0,
            behind: None,
            emit: None,
            particles: None,
            rope: None,
            tie: Vec::new(),
            hang: None,
            links: None,
            swing: 0.0,
            fluid: None,
            roll: None,
            weight: None,
            sail: None,
            pull: None,
            fan: None,
            morph: None,
            body: None,
            ui: false,
            ui_role: crate::UiRole::Auto,
            part: None,
            dial: crate::Dial::default(),
            radio: None,
            locked: false,
            brush: crate::Brush::default(),
            slice: None,
            stack: None,
            fit: false,
            anchor: None,
            offset: Vec2::ZERO,
            frame: None,
        }
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    pub fn get(&self, prop: Prop) -> Value {
        let paint = |c: Option<Color>| c.map_or(Value::None, Value::Color);
        match prop {
            Prop::WarpX(i) => Value::Num(self.lattice.as_ref().filter(|_| i < 9).map_or(0.0, |l| l.effective_offset(i as usize).x)),
            Prop::WarpY(i) => Value::Num(self.lattice.as_ref().filter(|_| i < 9).map_or(0.0, |l| l.effective_offset(i as usize).y)),
            Prop::Contour => Value::Num(self.forms.as_ref().map_or(0.0, |f| f.amount)),
            Prop::Visible => Value::Num(if self.visible { 1.0 } else { 0.0 }),
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
            Prop::SkewX => Value::Num(self.skew_x),
            Prop::SkewY => Value::Num(self.skew_y),
            Prop::FontSize => Value::Num(self.text_style.size),
            Prop::LetterSpacing => Value::Num(self.text_style.spacing),
            Prop::Reveal => Value::Num(self.text_style.reveal),
            Prop::Wave => Value::Num(self.text_style.wave),
            Prop::Phase => Value::Num(self.text_style.phase),
            Prop::Blur => Value::Num(self.blur),
            Prop::ShadowX => Value::Num(self.shadow.map_or(0.0, |s| s.dx)),
            Prop::ShadowY => Value::Num(self.shadow.map_or(0.0, |s| s.dy)),
            Prop::ShadowBlur => Value::Num(self.shadow.map_or(0.0, |s| s.blur)),
            Prop::ShadowColor => paint(self.shadow.map(|s| s.color)),
            Prop::Glow => Value::Num(self.glow.map_or(0.0, |g| g.size)),
            Prop::InsetX => Value::Num(self.inset.map_or(0.0, |s| s.dx)),
            Prop::InsetY => Value::Num(self.inset.map_or(0.0, |s| s.dy)),
            Prop::InsetBlur => Value::Num(self.inset.map_or(0.0, |s| s.blur)),
            Prop::InsetColor => paint(self.inset.map(|s| s.color)),
            Prop::Noise => Value::Num(self.noise.map_or(0.0, |n| n.amount)),
            Prop::Elevation => Value::Num(self.elevation),
            Prop::Lat => Value::Num(self.lat),
            Prop::Lon => Value::Num(self.lon),
            Prop::Alt => Value::Num(self.alt),
            Prop::Yaw => Value::Num(self.yaw),
            Prop::Pitch => Value::Num(self.pitch),
            Prop::Pull => Value::Num(self.pull.map_or(0.0, |f| f.strength)),
            Prop::Fan => Value::Num(self.fan.map_or(0.0, |f| f.strength)),
            Prop::Morph => Value::Num(self.morph.as_ref().map_or(0.0, |m| m.amount)),
            Prop::Depth => Value::Num(self.body.map_or(0.0, |b| b.depth)),
            Prop::Taper => Value::Num(self.body.map_or(1.0, |b| b.taper)),
            Prop::Rough => Value::Num(self.brush.rough),
            Prop::GravityX | Prop::GravityY | Prop::WindX | Prop::WindY | Prop::Gust | Prop::Parameter => Value::None,
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
                if prop == Prop::Fill {
                    self.fill = p
                } else if prop == Prop::ShadowColor || prop == Prop::InsetColor {
                    let effect = if prop == Prop::ShadowColor { &mut self.shadow } else { &mut self.inset };
                    match p {
                        Some(color) => effect.get_or_insert(Shadow::DEFAULT).color = color,
                        None => *effect = None,
                    }
                } else {
                    // Match the ordinary `stroke COLOR` authoring default when an
                    // animation first enables a previously absent outline. An existing
                    // zero-width stroke is intentional, and width keys are applied last.
                    if self.stroke.is_none() && self.stroke_gradient.is_none() && p.is_some() && self.stroke_width == 0.0 {
                        self.stroke_width = 2.0;
                    }
                    self.stroke = p
                }
                if matches!(prop, Prop::Fill | Prop::StrokeColor) {
                    self.forget_morph_paint();
                }
            }
            return;
        }
        let Value::Num(v) = value else { return };
        match prop {
            Prop::WarpX(i) => {
                if let Some(l) = self.lattice.as_mut().filter(|_| i < 9) {
                    l.offsets[i as usize].x = v;
                    l.correction[i as usize].x = 0.0;
                }
            }
            Prop::WarpY(i) => {
                if let Some(l) = self.lattice.as_mut().filter(|_| i < 9) {
                    l.offsets[i as usize].y = v;
                    l.correction[i as usize].y = 0.0;
                }
            }
            Prop::Contour => {
                if let Some(f) = &mut self.forms {
                    f.amount = v;
                    f.mix = None;
                }
            }
            Prop::Visible => self.visible = v >= 0.5,
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
            Prop::StrokeWidth => {
                self.stroke_width = v;
                self.forget_morph_paint();
            }
            Prop::SkewX => self.skew_x = v,
            Prop::SkewY => self.skew_y = v,
            Prop::FontSize => self.text_style.size = v,
            Prop::LetterSpacing => self.text_style.spacing = v,
            Prop::Reveal => self.text_style.reveal = v,
            Prop::Wave => self.text_style.wave = v,
            Prop::Phase => self.text_style.phase = v,
            Prop::Blur => self.blur = v.max(0.0),
            Prop::ShadowX => self.shadow.get_or_insert(Shadow::DEFAULT).dx = v,
            Prop::ShadowY => self.shadow.get_or_insert(Shadow::DEFAULT).dy = v,
            Prop::ShadowBlur => self.shadow.get_or_insert(Shadow::DEFAULT).blur = v.max(0.0),
            Prop::Glow => self.glow.get_or_insert(Glow::DEFAULT).size = v.max(0.0),
            Prop::InsetX => self.inset.get_or_insert(Shadow::DEFAULT).dx = v,
            Prop::InsetY => self.inset.get_or_insert(Shadow::DEFAULT).dy = v,
            Prop::InsetBlur => self.inset.get_or_insert(Shadow::DEFAULT).blur = v.max(0.0),
            Prop::Noise => self.noise.get_or_insert(Noise::DEFAULT).amount = v.clamp(0.0, 1.0),
            Prop::Elevation => self.elevation = v.max(0.0),
            Prop::Lat => self.lat = v,
            Prop::Lon => self.lon = v,
            Prop::Alt => self.alt = v,
            Prop::Yaw => self.yaw = v,
            Prop::Pitch => self.pitch = v,
            Prop::Pull => self.pull.get_or_insert_with(crate::Field::pull).strength = v,
            Prop::Fan => self.fan.get_or_insert_with(crate::Field::fan).strength = v,
            Prop::Morph => {
                if let Some(m) = &mut self.morph {
                    m.amount = v.max(0.0);
                }
            }
            Prop::Depth => self.body.get_or_insert(crate::Body::new(0.0)).depth = v,
            Prop::Taper => {
                if let Some(b) = &mut self.body {
                    b.taper = v.max(0.0);
                }
            }
            Prop::Rough => self.brush.rough = v.max(0.0),
            Prop::Fill | Prop::StrokeColor | Prop::ShadowColor | Prop::InsetColor => unreachable!(),
            Prop::GravityX | Prop::GravityY | Prop::WindX | Prop::WindY | Prop::Gust | Prop::Parameter => {}
        }
    }

    /// Transform from this node's local space into its parent's space.
    /// The local origin is the node's center; rotation, skew and scale turn around `pivot`.
    pub fn local_transform(&self) -> Affine {
        let tan = |deg: f64| deg.clamp(-89.0, 89.0).to_radians().tan();
        Affine::translate(Vec2::new(self.x, self.y) + self.offset + self.pivot)
            * self.warp.unwrap_or(Affine::IDENTITY)
            * Affine::rotate((self.rotation + self.swing).to_radians())
            * Affine::skew(tan(self.skew_x), tan(self.skew_y))
            * Affine::scale_non_uniform(self.scale_x, self.scale_y)
            * Affine::translate(-self.pivot)
    }

    /// Geometry in local space, centered on the origin, bent by bones when skinned. Groups have none.
    pub fn path(&self) -> Option<BezPath> {
        if let Some(p) = &self.lattice_path {
            return Some(p.clone());
        }
        self.unbent_path()
    }

    /// When cages of parent groups bend this shape: its outline before them, the outline they
    /// make of it (what [`Node::path`] gives), and the map that bends a point of the first.
    pub fn cage_bend(&self) -> Option<(BezPath, &BezPath, &crate::LatticeMap)> {
        let map = self.lattice_map.as_deref()?;
        let bent = self.lattice_path.as_ref()?;
        Some((self.unbent_path()?, bent, map))
    }

    /// Geometry in local space before the cages of parent groups bend it.
    fn unbent_path(&self) -> Option<BezPath> {
        match &self.deformed {
            Some(d) => Some(d.clone()),
            None => self.base_path(),
        }
    }

    /// Geometry as drawn, before bones bend it: the morphed shape while a morph is under way.
    pub fn editable_path(&self) -> BezPath {
        self.forms.as_ref().and_then(|f| f.path.clone()).or_else(|| self.morph.as_ref().and_then(|m| m.path.clone())).unwrap_or_else(|| self.geometry.clone())
    }

    pub fn base_path(&self) -> Option<BezPath> {
        if let Some(path) = self.forms.as_ref().and_then(|f| f.path.clone()) {
            return Some(path);
        }
        match self.morph.as_ref().and_then(|m| m.path.clone()) {
            Some(p) => Some(p),
            None => self.shape_path(),
        }
    }

    /// A new colour or stroke becomes the morph's starting paint.
    fn forget_morph_paint(&mut self) {
        if let Some(m) = &mut self.morph {
            m.base = None;
        }
    }

    /// The node's own shape, before morphing and bones.
    pub fn shape_path(&self) -> Option<BezPath> {
        let (w, h) = (self.width.abs(), self.height.abs());
        let rect = kurbo::Rect::new(-w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
        Some(match self.kind {
            ShapeKind::Group => return None,
            ShapeKind::Rect => {
                let max = w.min(h) / 2.0;
                let radii = self.radii.unwrap_or([self.radius; 4]).map(|r| r.clamp(0.0, max));
                RoundedRect::from_rect(rect, kurbo::RoundedRectRadii::new(radii[0], radii[1], radii[2], radii[3])).to_path(0.1)
            }
            ShapeKind::Ellipse => Ellipse::from_rect(rect).to_path(0.1),
            ShapeKind::Star => star_path(w, h, self.points.round().max(3.0) as usize, self.inner.clamp(0.05, 1.0)),
            ShapeKind::Polygon => polygon_path(w, h, self.points.round().max(3.0) as usize),
            ShapeKind::Path => self.geometry.clone(),
            ShapeKind::Text => self.text.as_ref().map(|t| t.layout(&self.text_style)).unwrap_or_default(),
            ShapeKind::Bone => {
                // A thin diamond from the joint to the tip, for picking and outlining in the editor.
                let len = self.width;
                let w = (len.abs() * 0.1).clamp(2.0, 7.0);
                let mut p = BezPath::new();
                p.move_to((0.0, 0.0));
                p.line_to((len * 0.15, w));
                p.line_to((len, 0.0));
                p.line_to((len * 0.15, -w));
                p.close_path();
                p
            }
        })
    }

    /// The tip of a bone in its own space: `(len, 0)`.
    pub fn bone_tip(&self) -> Point {
        Point::new(self.width, 0.0)
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
        Document {
            artboard,
            nodes: Vec::new(),
            animations: Vec::new(),
            parameters: Vec::new(),
            drivers: Vec::new(),
            correctives: Vec::new(),
            machines: Vec::new(),
            light: 90.0,
            world: crate::World::default(),
            fields: Vec::new(),
            time: 0.0,
            purpose: Purpose::Any,
            directions: None,
            ids: crate::Ids::default(),
            seed: 0,
        }
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

    /// Whether node `i` sits inside node `outer`, at any depth.
    pub fn is_inside(&self, i: usize, outer: usize) -> bool {
        let mut cur = self.nodes.get(i).and_then(|n| n.parent);
        while let Some(p) = cur {
            if p == outer {
                return true;
            }
            cur = self.nodes[p].parent;
        }
        false
    }

    /// Whether node `index` or a group around it is locked.
    pub fn is_locked(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        node.locked || node.parent.is_some_and(|p| self.is_locked(p))
    }

    /// Topmost visible, unlocked shape under an artboard-space point. Bones come first: they are
    /// thin and drawn over the shapes they move.
    pub fn hit_test(&self, point: Point) -> Option<usize> {
        let bones = (0..self.nodes.len()).rev().filter(|&i| self.nodes[i].kind == ShapeKind::Bone);
        let others = (0..self.nodes.len()).rev().filter(|&i| self.nodes[i].kind != ShapeKind::Bone);
        bones.chain(others).find(|&i| self.is_visible(i) && !self.is_locked(i) && self.shape_hit(i, point))
    }

    /// Whether an artboard-space point is on node `i` or on a visible shape inside it, whatever
    /// lies on top: what a pointer listener of a state machine reacts to.
    pub fn hits_tree(&self, i: usize, point: Point) -> bool {
        (0..self.nodes.len())
            .any(|k| (k == i || self.is_inside(k, i)) && self.nodes[k].kind != ShapeKind::Bone && self.is_visible(k) && self.shape_hit(k, point))
    }

    /// The artboard-space box around node `i` and everything inside it.
    pub fn world_bounds(&self, i: usize) -> Option<kurbo::Rect> {
        (0..self.nodes.len())
            .filter(|&k| k == i || self.is_inside(k, i))
            .filter_map(|k| Some(self.world_transform(k).transform_rect_bbox(self.nodes[k].path()?.bounding_box())))
            .reduce(|a, b| a.union(b))
    }

    /// Whether an artboard-space point is on the shape of node `i` itself.
    fn shape_hit(&self, i: usize, point: Point) -> bool {
        let node = &self.nodes[i];
        let Some(path) = node.path() else { return false };
        // A shape scaled to nothing (a pop-in keyed from scale 0) has no inside to hit.
        let w = self.world_transform(i);
        if w.determinant().abs() < 1e-12 {
            return false;
        }
        let local = w.inverse() * point;
        if !local.is_finite() {
            return false;
        }
        // Text is picked by its whole box, not only the letters.
        if let Some(t) = &node.text
            && node.lattice_path.is_none()
        {
            return t.bounds(&node.text_style).inflate(2.0, 2.0).contains(local);
        }
        if let Some(faces) = node.body_faces(self.light) {
            return faces.iter().any(|f| f.path.contains(local));
        }
        path.contains(local) || (node.stroke.is_some() && near_outline(&path, local, node.stroke_width.max(4.0)))
    }

    pub fn animation(&self, name: &str) -> Option<&Animation> {
        self.animations.iter().find(|a| a.name == name)
    }

    pub fn machine(&self, name: &str) -> Option<&crate::Machine> {
        self.machines.iter().find(|m| m.name == name)
    }
}

fn near_outline(path: &BezPath, p: Point, width: f64) -> bool {
    path.segments().any(|seg| seg.nearest(p, 0.1).distance_sq <= (width / 2.0).powi(2))
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
    pub fn sample_segment(&self, t: f64) -> Option<(Value, Value, f64)> {
        let first = self.keys.first()?;
        if t <= first.time {
            return Some((first.value, first.value, 0.0));
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t < b.time {
                let span = (b.time - a.time).max(1e-9);
                return Some((a.value, b.value, a.ease.apply((t - a.time) / span)));
            }
        }
        self.keys.last().map(|k| (k.value, k.value, 0.0))
    }
    pub fn sample(&self, t: f64) -> Option<Value> {
        let (a, b, k) = self.sample_segment(t)?;
        Some(if self.prop == Prop::Visible { a } else { a.lerp(b, k) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration: f64,
    pub looping: bool,
    pub tracks: Vec<Track>,
    /// Named moments the game hears about: a footstep, the frame a hit lands.
    pub events: Vec<AnimEvent>,
}

/// `event step 0.15 0.35` in an animation: the game is told «step» when playback passes 0.15 s
/// and 0.35 s.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimEvent {
    pub name: String,
    pub time: f64,
}

impl Animation {
    /// The events playback passes going from `from` to `to` seconds after the animation
    /// started, in order: each one whose time `t` (or, looping, `t` plus whole durations)
    /// satisfies `from < t <= to`. Start from a little below 0 to include events at 0.
    pub fn events_between(&self, from: f64, to: f64) -> Vec<&AnimEvent> {
        self.events_passed(from, to).into_iter().map(|(_, e)| e).collect()
    }

    /// [`events_between`](Self::events_between) with the playback time each one is passed at
    /// (counting whole loops, so it lies in `from..=to`).
    pub fn events_passed(&self, from: f64, to: f64) -> Vec<(f64, &AnimEvent)> {
        let mut out: Vec<(f64, &AnimEvent)> = Vec::new();
        if to <= from {
            return Vec::new();
        }
        for e in &self.events {
            if self.looping && self.duration > 0.0 {
                // The first pass of this event after `from`.
                let mut at = e.time + ((from - e.time) / self.duration).floor() * self.duration;
                while at <= from {
                    at += self.duration;
                }
                while at <= to {
                    out.push((at, e));
                    at += self.duration;
                }
            } else if e.time > from && e.time <= to && e.time <= self.duration + 1e-9 {
                out.push((e.time, e));
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

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

    /// Returns a copy of the document posed at `t` seconds, with bones solved and shapes bent.
    pub fn pose(&self, doc: &Document, t: f64) -> Document {
        self.pose_with(doc, t, &mut crate::SimCache::default())
    }

    /// The document with this animation's keys applied at `t` seconds, before `Document::rig`.
    pub fn keys_at(&self, doc: &Document, t: f64) -> Document {
        let t = self.local_time(t);
        let mut out = doc.clone();
        out.time = t;
        for track in self.tracks.iter().filter(|t| t.prop == Prop::Parameter) {
            if let Some(v) = track.sample(t) {
                out.set_animated_value(&track.node, track.prop, v);
            }
        }
        out.apply_parameters();
        self.apply_node_keys(&mut out, t);
        out
    }

    /// Explicit node keys take precedence over parameter responses, including while previewing a draft parameter.
    pub fn apply_node_keys(&self, out: &mut Document, t: f64) {
        let t = self.local_time(t);
        let paints_and_properties =
            self.tracks.iter().filter(|t| !matches!(t.prop, Prop::Parameter | Prop::StrokeWidth | Prop::ShadowColor | Prop::InsetColor));
        let widths = self.tracks.iter().filter(|t| t.prop == Prop::StrokeWidth);
        let effect_colors = self.tracks.iter().filter(|t| matches!(t.prop, Prop::ShadowColor | Prop::InsetColor));
        // Explicit width keys, including zero, take precedence over the implicit
        // width of a newly enabled stroke, independently of the source track order.
        // Effect colors run after their numeric geometry. A final `none` must not
        // be resurrected by a later blur/offset key in a differently ordered source.
        for track in paints_and_properties.chain(widths).chain(effect_colors) {
            if let Some(v) = track.sample(t) {
                out.set_animated_value(&track.node, track.prop, v);
            }
            if track.prop == Prop::Contour
                && let Some(i) = out.index_of(&track.node)
                && let Some(forms) = &mut out.nodes[i].forms
            {
                forms.mix = track.sample_segment(t).and_then(|(a, b, k)| Some((a.as_num()? as usize, b.as_num()? as usize, k)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_color_keys_preserve_dimensions_and_absence_independently_of_track_order() {
        let color = Color { r: 30, g: 190, b: 110, a: 200 };
        for (color_prop, x_prop, blur_prop) in [(Prop::ShadowColor, Prop::ShadowX, Prop::ShadowBlur), (Prop::InsetColor, Prop::InsetX, Prop::InsetBlur)] {
            assert!(color_prop.is_color());
            assert_eq!(Prop::from_path_name(color_prop.path_name()), Some(color_prop));
            let mut doc = Document::new(Artboard::default());
            let mut node = Node::new("well", ShapeKind::Rect);
            node.set(x_prop, Value::Num(3.0));
            node.set(blur_prop, Value::Num(7.0));
            node.set(color_prop, Value::Color(color));
            assert_eq!(node.get(x_prop), Value::Num(3.0));
            assert_eq!(node.get(blur_prop), Value::Num(7.0));
            doc.nodes.push(node);
            let track = |prop, value| Track { node: "well".into(), prop, keys: vec![Keyframe { time: 0.0, value, ease: Ease::Linear }] };
            for reversed in [false, true] {
                let mut tracks = vec![
                    Track {
                        node: "well".into(),
                        prop: color_prop,
                        keys: vec![
                            Keyframe { time: 0.0, value: Value::None, ease: Ease::Linear },
                            Keyframe { time: 1.0, value: Value::Color(color), ease: Ease::Linear },
                        ],
                    },
                    track(x_prop, Value::Num(17.0)),
                    track(blur_prop, Value::Num(8.0)),
                ];
                if reversed {
                    tracks.reverse();
                }
                let anim = Animation { name: "Hover".into(), duration: 1.0, looping: false, tracks, events: vec![] };
                assert_eq!(anim.pose(&doc, 0.0).nodes[0].get(color_prop), Value::None, "offset/blur must not resurrect a removed effect");
                let pose = anim.pose(&doc, 0.5);
                assert_eq!(pose.nodes[0].get(color_prop), Value::Color(Color { a: 100, ..color }));
                assert_eq!(pose.nodes[0].get(x_prop), Value::Num(17.0));
                assert_eq!(pose.nodes[0].get(blur_prop), Value::Num(8.0));
                assert_eq!(doc.nodes[0].get(blur_prop), Value::Num(7.0), "posing must not change the source effect");
            }
        }
    }

    #[test]
    fn absent_paint_fades_with_the_other_colors_rgb_and_exact_endpoints() {
        let color = Color { r: 40, g: 120, b: 230, a: 200 };
        assert_eq!(Value::None.lerp(Value::Color(color), 0.0), Value::None);
        assert_eq!(Value::None.lerp(Value::Color(color), 1.0), Value::Color(color));
        assert_eq!(Value::None.lerp(Value::Color(color), 0.25), Value::Color(Color { a: 50, ..color }));
        assert_eq!(Value::Color(color).lerp(Value::None, 0.5), Value::Color(Color { a: 100, ..color }));
        assert_eq!(Value::Color(color).lerp(Value::None, 1.0), Value::None);
        assert_eq!(Value::None.lerp(Value::Num(5.0), 0.5), Value::None, "other type mismatches remain discrete");
        let mut node = Node::new("button", ShapeKind::Rect);
        node.set(Prop::StrokeColor, Value::Color(color));
        assert_eq!(node.stroke_width, 2.0);
        node.stroke = None;
        node.stroke_width = 7.0;
        node.set(Prop::StrokeColor, Value::Color(color));
        assert_eq!(node.stroke_width, 7.0, "a retained width belongs to the existing outline configuration");
        node.stroke_width = 0.0;
        node.set(Prop::StrokeColor, Value::Color(Color::WHITE));
        assert_eq!(node.stroke_width, 0.0, "an existing stroke explicitly set to zero stays hidden");
        node.stroke = None;
        node.stroke_gradient = Some(Gradient { radial: false, angle: 0.0, colors: vec![Color::BLACK, Color::WHITE] });
        node.set(Prop::StrokeColor, Value::Color(color));
        assert_eq!(node.stroke_width, 0.0, "a zero-width gradient outline remains intentional too");
    }

    #[test]
    fn explicit_stroke_width_keys_win_independently_of_source_track_order() {
        let mut doc = Document::new(Artboard::default());
        doc.nodes.push(Node::new("button", ShapeKind::Rect));
        let track = |prop, value| Track { node: "button".into(), prop, keys: vec![Keyframe { time: 0.0, value, ease: Ease::Linear }] };
        for width in [0.0, 6.0] {
            for reversed in [false, true] {
                let mut tracks = vec![track(Prop::StrokeWidth, Value::Num(width)), track(Prop::StrokeColor, Value::Color(Color::WHITE))];
                if reversed {
                    tracks.reverse();
                }
                let anim = Animation { name: "Hover".into(), duration: 1.0, looping: false, tracks, events: vec![] };
                let pose = anim.pose(&doc, 0.5);
                assert_eq!(pose.nodes[0].stroke_width, width);
                assert_eq!(doc.nodes[0].stroke, None, "posing never changes the source");
            }
        }
    }

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
    fn events_fire_once_per_pass() {
        let ev = |name: &str, time| AnimEvent { name: name.into(), time };
        let run =
            Animation { name: "Run".into(), duration: 0.4, looping: true, tracks: vec![], events: vec![ev("step", 0.1), ev("step2", 0.3), ev("start", 0.0)] };
        let names = |v: Vec<&AnimEvent>| v.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(run.events_between(-1e-9, 0.15)), ["start", "step"]);
        assert_eq!(names(run.events_between(0.15, 0.3)), ["step2"]);
        // Across the loop: the end of one pass, then the start of the next, in order.
        assert_eq!(names(run.events_between(0.35, 0.95)), ["start", "step", "step2", "start", "step"]);
        assert!(run.events_between(0.3, 0.3).is_empty());
        let once = Animation { looping: false, ..run };
        assert_eq!(names(once.events_between(0.0, 5.0)), ["step", "step2"], "no repeats, and 0 only from the start");
    }

    #[test]
    fn looping_time_wraps() {
        let anim = Animation { name: "a".into(), duration: 2.0, looping: true, tracks: vec![], events: Vec::new() };
        assert!((anim.local_time(5.0) - 1.0).abs() < 1e-9);
        let once = Animation { looping: false, ..anim };
        assert_eq!(once.local_time(5.0), 2.0);
    }
}
