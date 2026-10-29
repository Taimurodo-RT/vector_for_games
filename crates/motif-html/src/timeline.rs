//! CSS time is sampled into ordinary editable MSC keys; rendering remains vector based.
use crate::animation::{AnimationSpec, TransitionSpec};
use crate::{BoxNode, Element, animation, css, emit, interpolate, layout, num, style::Style};
use motif_core::{Document, Prop, Value};
use std::collections::{BTreeMap, BTreeSet};

type Declarations = BTreeMap<String, String>;
const MAX_SECONDS: f64 = 12.0;

struct StateScene<'a> {
    nodes: &'a [Element],
    sheet: &'a css::Stylesheet,
    ids: &'a [String],
    base: &'a [Style],
    declarations: Vec<Declarations>,
    animations: Vec<Vec<AnimationSpec>>,
    transitions: Vec<Vec<TransitionSpec>>,
    normal: bool,
}

impl<'a> StateScene<'a> {
    fn new(nodes: &'a [Element], sheet: &'a css::Stylesheet, ids: &'a [String], base: &'a [Style], state: css::State, warnings: &mut Vec<String>) -> Self {
        let declarations: Vec<_> = (0..nodes.len()).map(|i| css::cascade(&sheet.rules, nodes, i, state, warnings)).collect();
        let animations: Vec<_> = declarations.iter().map(|d| animation::parse_animations(d, warnings)).collect();
        let transitions = declarations
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut transitions = animation::parse_transitions(d, warnings);
                // Keep the original Motif state blend for components without CSS timing.
                if sheet.keyframes.is_empty()
                    && !d.keys().any(|p| p.starts_with("transition"))
                    && *d != css::cascade(&sheet.rules, nodes, i, css::State::Base, warnings)
                {
                    transitions = vec![TransitionSpec { duration: 0.2, timing: animation::Timing::Linear, ..Default::default() }];
                }
                transitions
            })
            .collect();
        for specs in &animations {
            for spec in specs {
                if !sheet.keyframes.contains_key(&spec.name) {
                    warnings.push(format!("CSS animation {}: @keyframes не найден", spec.name));
                }
            }
        }
        Self { nodes, sheet, ids, base, declarations, animations, transitions, normal: state == css::State::Base }
    }

    fn durations(&self, warnings: &mut Vec<String>) -> (f64, Option<f64>) {
        let mut entrance = if self.normal { 0.0_f64 } else { 0.2_f64 };
        for t in self.transitions.iter().flatten() {
            entrance = entrance.max(t.end_time());
        }
        let mut periods = Vec::new();
        for a in self.animations.iter().flatten().filter(|a| !a.paused && self.sheet.keyframes.contains_key(&a.name)) {
            if let Some(end) = a.end_time() {
                entrance = entrance.max(end);
            } else if a.period() > 0.0 {
                periods.push(a.period());
                entrance = entrance.max(a.delay);
            }
        }
        if entrance > MAX_SECONDS {
            warnings.push(format!("CSS: входная анимация ограничена {MAX_SECONDS} секундами"));
            entrance = MAX_SECONDS;
        }
        let gcd = |mut a: u64, mut b: u64| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        };
        let mut period = 1_u64;
        for p in &periods {
            let ticks = (p * 1000.0).round().max(1.0) as u64;
            if (ticks as f64 / 1000.0 - p).abs() > 1e-6 {
                warnings.push("CSS: период цикла округлён до миллисекунд".into());
            }
            period = (period / gcd(period, ticks)).saturating_mul(ticks);
            if period > (MAX_SECONDS * 1000.0) as u64 {
                warnings.push("CSS: общий период циклов больше 12 секунд; петля не создана, входной клип содержит ограниченный фрагмент".into());
                return ((entrance + MAX_SECONDS).min(MAX_SECONDS), None);
            }
        }
        (entrance.max(0.001), (!periods.is_empty()).then_some(period as f64 / 1000.0))
    }

    fn styles(&self, time: f64, warnings: &mut Vec<String>) -> Vec<Style> {
        let mut styles: Vec<Style> = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            let parent = node.parent.map(|p| &styles[p]);
            let underlying = Style::compute(node, parent, &self.declarations[i], warnings);
            let mut current = underlying.clone();
            if !self.normal {
                for property in interpolate::PROPERTIES {
                    if let Some(transition) = self.transitions[i].iter().rev().find(|t| t.applies_to(property))
                        && !interpolate::property(property, &mut current, &self.base[i], &underlying, transition.progress(time))
                    {
                        warnings.push(format!(
                            "CSS transition {property}: сочетание auto, разных единиц или transform-функций не интерполируется; сохранено конечное значение"
                        ));
                    }
                }
            }
            for spec in &self.animations[i] {
                let Some(progress) = spec.progress(time) else { continue };
                let Some(frames) = self.sheet.keyframes.get(&spec.name) else { continue };
                for property in frames.properties() {
                    let sample = frames.property_sample(property, progress, spec.timing);
                    let endpoint = |value: Option<&str>, warnings: &mut Vec<String>| {
                        if let Some(value) = value {
                            let mut declarations = self.declarations[i].clone();
                            declarations.insert(property.to_owned(), value.to_owned());
                            Style::compute(node, parent, &declarations, warnings)
                        } else {
                            underlying.clone()
                        }
                    };
                    let from = endpoint(sample.from, warnings);
                    let to = endpoint(sample.to, warnings);
                    if !interpolate::property(property, &mut current, &from, &to, sample.mix) {
                        warnings.push(format!("CSS @keyframes {}: свойство {property} не анимируется в Motif", spec.name));
                    }
                }
            }
            current.hidden |= node.suppressed;
            if current.line_height != self.base[i].line_height {
                warnings.push(format!("{}: изменение line-height в анимации не импортировано; сохранён исходный межстрочный интервал", node.tag));
                current.line_height = self.base[i].line_height;
            }
            if self.base[i].hidden && !current.hidden {
                warnings.push(format!("{}: появление исходно скрытого HTML-элемента в состоянии требует ручной правки", node.tag));
                current.hidden = true;
            }
            if current.gradient != self.base[i].gradient {
                warnings.push(format!("{}: изменение градиента в состоянии не импортировано; сохраняется базовый градиент", node.tag));
                current.gradient = self.base[i].gradient.clone();
                current.background = self.base[i].background.clone();
            }
            if current.clip_path != self.base[i].clip_path
                || current.overflow_hidden != self.base[i].overflow_hidden
                || current.border_double != self.base[i].border_double
            {
                warnings.push(format!("{}: изменение типа маски или обводки в состоянии не импортировано", node.tag));
                current.clip_path = self.base[i].clip_path.clone();
                current.overflow_hidden = self.base[i].overflow_hidden;
                current.border_double = self.base[i].border_double;
            }
            // Native font files and text decorations are authored geometry, unlike keyed transforms.
            if current.bold != self.base[i].bold || current.underline != self.base[i].underline || current.strike != self.base[i].strike {
                warnings.push(format!("{}: смена шрифта/декорации в состоянии требует ручной правки", node.tag));
                current.bold = self.base[i].bold;
                current.underline = self.base[i].underline;
                current.strike = self.base[i].strike;
            }
            styles.push(current);
        }
        styles
    }

    fn document(&self, time: f64, warnings: &mut Vec<String>) -> Result<Document, String> {
        let styles = self.styles(time, warnings);
        let mut boxes = vec![BoxNode::default(); self.nodes.len()];
        layout::measure(0, self.nodes, &styles, &mut boxes);
        layout::position(0, 24.0, 24.0, self.nodes, &styles, &mut boxes);
        let mut source = "artboard 8192x8192 ui\n".to_owned();
        for child in &self.nodes[0].children {
            emit(*child, 0, self.nodes, &styles, &boxes, self.ids, &mut source);
        }
        motif_script::parse(&source).map(|s| s.doc).map_err(|e| format!("CSS кадр: {e:?}"))
    }

    fn times(&self, start: f64, duration: f64, warnings: &mut Vec<String>) -> Vec<f64> {
        let budget = (40_000 / self.nodes.len().max(1)).clamp(8, 720);
        let samples = ((duration * 60.0).ceil() as usize).clamp(1, budget);
        if duration * 60.0 > budget as f64 {
            warnings.push("CSS: частота опорных ключей снижена для ограничения размера сцены".into());
        }
        let mut times: Vec<_> = (0..=samples).map(|i| duration * i as f64 / samples as f64).collect();
        let mut add = |t: f64| {
            let t = t - start;
            if t >= 0.0 && t <= duration {
                times.push(t);
            }
        };
        for t in self.transitions.iter().flatten() {
            add(t.delay);
            add(t.delay + t.duration);
        }
        for a in self.animations.iter().flatten() {
            add(a.delay);
            if let Some(end) = a.end_time() {
                add(end);
            }
            if a.duration > 0.0
                && let Some(frames) = self.sheet.keyframes.get(&a.name)
            {
                for cycle in 0..(((start + duration - a.delay) / a.duration).ceil().max(0.0) as usize).min(128) {
                    for offset in frames.offsets() {
                        add(a.delay + a.duration * (cycle as f64 + offset));
                    }
                }
            }
        }
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        times
    }

    fn clip(&self, name: &str, start: f64, duration: f64, looping: bool, base: &Document, warnings: &mut Vec<String>) -> Result<String, String> {
        let times = self.times(start, duration, warnings);
        let mut tracks: BTreeMap<(String, Prop), Vec<(f64, Value)>> = BTreeMap::new();
        let mut previous_angles: BTreeMap<String, f64> = BTreeMap::new();
        let mut properties = Prop::ALL.to_vec();
        // Preserve the familiar paint-first ordering in scripts produced before CSS animation support.
        properties.sort_by_key(|p| if *p == Prop::Fill { 0 } else { 1 });
        for t in &times {
            let document = self.document(start + t, warnings)?;
            for original in &base.nodes {
                let Some(node) = document.node(&original.id) else {
                    tracks.entry((original.id.clone(), Prop::Visible)).or_default().push((*t, Value::Num(0.0)));
                    continue;
                };
                let fixed_path = node.kind == motif_core::ShapeKind::Path && node.geometry != original.geometry;
                if fixed_path {
                    warnings.push(format!("{}: изменение геометрии CSS-маски/контура в анимации не импортировано; сохранён исходный контур", original.id));
                }
                for property in &properties {
                    let mut value = if fixed_path && matches!(property, Prop::Width | Prop::Height) { original.get(*property) } else { node.get(*property) };
                    if *property == Prop::Rotation
                        && let Value::Num(mut angle) = value
                    {
                        if let Some(last) = previous_angles.get(&original.id) {
                            angle += ((last - angle) / 360.0).round() * 360.0;
                        }
                        previous_angles.insert(original.id.clone(), angle);
                        value = Value::Num(angle);
                    }
                    tracks.entry((original.id.clone(), *property)).or_default().push((*t, value));
                }
            }
        }
        let mut text = String::new();
        let mut entries: Vec<_> =
            tracks.into_iter().filter(|((id, p), keys)| base.node(id).is_some_and(|n| keys.iter().any(|(_, v)| *v != n.get(*p)))).collect();
        entries.sort_by_key(|((id, p), _)| (if *p == Prop::Fill { 0 } else { 1 }, id.clone(), *p));
        for ((id, p), keys) in entries {
            let keys = simplify(keys);
            text.push_str(&format!(
                "  {id}.{} {}\n",
                p.path_name(),
                keys.into_iter()
                    .map(|(t, v)| format!(
                        "{}: {}{}",
                        num(t),
                        match v {
                            Value::Num(v) => num(v),
                            _ => motif_script::fmt_value(v),
                        },
                        if p == Prop::Visible { " hold" } else { "" }
                    ))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if text.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("\nanim {name} {}s{} {{\n{text}}}\n", num(duration), if looping { " loop" } else { "" }))
    }
}

fn simplify(keys: Vec<(f64, Value)>) -> Vec<(f64, Value)> {
    let mut out: Vec<(f64, Value)> = Vec::new();
    for key in keys {
        while out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let k = (b.0 - a.0) / (key.0 - a.0).max(1e-12);
            let expected = a.1.lerp(key.1, k);
            let close = match (expected, b.1) {
                (Value::Num(a), Value::Num(b)) => (a - b).abs() < 1e-7,
                (a, b) => a == b,
            };
            if !close {
                break;
            }
            out.pop();
        }
        out.push(key);
    }
    out
}

pub(crate) fn emit_clips(
    nodes: &[Element],
    sheet: &css::Stylesheet,
    ids: &[String],
    base_styles: &[Style],
    source: &mut String,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let base = motif_script::parse(source).map_err(|e| format!("HTML сцена: {e:?}"))?.doc;
    let mut states = Vec::new();
    let normal = StateScene::new(nodes, sheet, ids, base_styles, css::State::Base, warnings);
    if normal.animations.iter().flatten().next().is_some() {
        states.push((css::State::Base, normal));
    }
    for state in css::State::ANIMATED {
        if sheet.rules.iter().any(|r| r.stateful_for(state)) {
            states.push((state, StateScene::new(nodes, sheet, ids, base_styles, state, warnings)));
        }
    }
    let mut notices = BTreeSet::new();
    for (state, scene) in states {
        let (entrance, period) = scene.durations(warnings);
        let name = if state == css::State::Base { "Normal" } else { state.clip() };
        let mut local = Vec::new();
        if scene.animations.iter().flatten().any(|a| {
            matches!(a.timing, animation::Timing::Steps(..))
                || a.fill == animation::Fill::None
                || a.iterations.is_none() && matches!(a.direction, animation::Direction::Normal | animation::Direction::Reverse)
        }) {
            warnings.push("CSS: дискретные скачки steps/fill/обычного цикла приближены опорной сеткой; на границе возможен переход в один интервал".into());
        }
        source.push_str(&scene.clip(name, 0.0, entrance, false, &base, &mut local)?);
        if let Some(period) = period {
            source.push_str(&scene.clip(&format!("{name}Loop"), entrance, period, true, &base, &mut local)?);
            warnings.push(format!("CSS: {name} — входной клип; {name}Loop — непрерывная петля после него. Проверка UI воспроизводит их вместе; screen/kit сохраняет статические состояния"));
        }
        notices.extend(local.into_iter().map(|s| if s.contains("состоянии") { format!("{s} (:{} )", state.pseudo()) } else { s }));
    }
    warnings.extend(notices);
    Ok(())
}
