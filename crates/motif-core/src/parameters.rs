//! Numeric pose controls shared by the editor, timeline and exporters. A driver's key
//! positions are parameter values, not seconds. Drivers write absolute properties, so
//! evaluating a pose twice never accumulates a deformation.
use crate::{Document, Prop, Track, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct PoseParameter {
    pub id: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoseDriver {
    pub parameter: String,
    pub tracks: Vec<Track>,
}

/// A complete rectangular table of additive warp residuals, in parameter coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectiveKey {
    pub x: f64,
    pub y: f64,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorrectiveTrack {
    pub node: String,
    pub prop: Prop,
    pub keys: Vec<CorrectiveKey>,
}

impl CorrectiveTrack {
    pub fn axes(&self) -> (Vec<f64>, Vec<f64>) {
        let axis = |x: bool| {
            let mut a: Vec<_> = self.keys.iter().map(|k| if x { k.x } else { k.y }).collect();
            a.sort_by(f64::total_cmp);
            a.dedup();
            a
        };
        (axis(true), axis(false))
    }
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let (xs, ys) = self.axes();
        let bracket = |a: &[f64], v: f64| -> Option<(f64, f64, f64)> {
            let first = *a.first()?;
            let last = *a.last()?;
            let v = v.clamp(first, last);
            let hi = a.partition_point(|n| *n < v).min(a.len() - 1);
            let lo = hi.saturating_sub(1);
            Some((a[lo], a[hi], if a[lo] == a[hi] { 0.0 } else { (v - a[lo]) / (a[hi] - a[lo]) }))
        };
        let (Some((x0, x1, tx)), Some((y0, y1, ty))) = (bracket(&xs, x), bracket(&ys, y)) else { return 0.0 };
        let at = |x, y| self.keys.iter().find(|k| k.x == x && k.y == y).map_or(0.0, |k| k.value);
        let lower = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
        let upper = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
        lower * (1.0 - ty) + upper * ty
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoseCorrective {
    pub id: String,
    pub parameters: [String; 2],
    pub tracks: Vec<CorrectiveTrack>,
}

impl Document {
    pub fn parameter(&self, id: &str) -> Option<&PoseParameter> {
        self.parameters.iter().find(|p| p.id == id)
    }

    /// Changes a control, not the source document. Call `apply_parameters` before `rig`
    /// (and before `face`, which adds the character's facing to the driven local yaw).
    pub fn set_parameter(&mut self, id: &str, value: f64) -> bool {
        let Some(p) = self.parameters.iter_mut().find(|p| p.id == id) else { return false };
        if !value.is_finite() {
            return false;
        }
        p.value = value.clamp(p.min, p.max);
        true
    }

    pub fn apply_parameters(&mut self) {
        for n in &mut self.nodes {
            if let Some(l) = &mut n.lattice {
                l.correction = [kurbo::Vec2::ZERO; 9];
            }
        }
        // Drivers cannot target parameters or share a target property: the parser checks
        // both. No dependency graph, evaluation order or incremental state is hidden here.
        let changes: Vec<_> = self
            .drivers
            .iter()
            .flat_map(|driver| {
                let parameter = self.parameter(&driver.parameter);
                driver.tracks.iter().filter_map(move |track| {
                    let p = parameter?;
                    let value = if p.value.is_finite() { p.value.clamp(p.min, p.max) } else { p.default };
                    let mix = if track.prop == Prop::Contour {
                        track.sample_segment(value).and_then(|(a, b, k)| Some((a.as_num()? as usize, b.as_num()? as usize, k)))
                    } else {
                        None
                    };
                    Some((track.node.clone(), track.prop, track.sample(value)?, mix))
                })
            })
            .collect();
        for (id, prop, value, mix) in changes {
            if let Some(i) = self.index_of(&id) {
                self.nodes[i].set(prop, value);
                if let Some(forms) = self.nodes[i].forms.as_mut()
                    && prop == Prop::Contour
                {
                    forms.mix = mix;
                }
            }
        }
        let changes: Vec<_> = self
            .correctives
            .iter()
            .flat_map(|c| {
                let xy = c
                    .parameters
                    .iter()
                    .map(|id| self.parameter(id).map(|p| if p.value.is_finite() { p.value.clamp(p.min, p.max) } else { p.default }))
                    .collect::<Option<Vec<_>>>();
                c.tracks.iter().filter_map(move |t| {
                    let xy = xy.as_ref()?;
                    Some((t.node.clone(), t.prop, t.sample(xy[0], xy[1])))
                })
            })
            .collect();
        for (id, prop, value) in changes {
            if let Some(i) = self.index_of(&id)
                && let Some(l) = &mut self.nodes[i].lattice
            {
                match prop {
                    Prop::WarpX(k) if k < 9 => l.correction[k as usize].x += value,
                    Prop::WarpY(k) if k < 9 => l.correction[k as usize].y += value,
                    _ => {}
                }
            }
        }
    }

    pub fn driven_by(&self, node: &str, prop: Prop) -> Option<&str> {
        self.drivers.iter().find(|d| d.tracks.iter().any(|t| t.node == node && t.prop == prop)).map(|d| d.parameter.as_str())
    }

    pub fn animated_value(&self, id: &str, prop: Prop) -> Option<Value> {
        if prop == Prop::Parameter {
            self.parameter(id).map(|p| Value::Num(p.value))
        } else if prop.is_world() {
            Some(self.world.get(prop))
        } else {
            self.node(id).map(|n| n.get(prop))
        }
    }

    pub fn set_animated_value(&mut self, id: &str, prop: Prop, value: Value) {
        if prop == Prop::Parameter {
            if let Value::Num(value) = value {
                self.set_parameter(id, value);
            }
        } else if prop.is_world() {
            self.world.set(prop, value);
        } else if let Some(i) = self.index_of(id) {
            self.nodes[i].set(prop, value);
        }
    }
}
