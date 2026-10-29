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
                    Some((track.node.clone(), track.prop, track.sample(value)?))
                })
            })
            .collect();
        for (id, prop, value) in changes {
            if let Some(i) = self.index_of(&id) {
                self.nodes[i].set(prop, value);
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
