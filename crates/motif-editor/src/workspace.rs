//! User workspaces and panel preferences. None of this state belongs to an MSC document.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Default)]
pub(crate) struct DraftPose {
    pub at: Option<(String, f64)>,
    pub values: std::collections::BTreeMap<(String, motif_core::Prop), motif_core::Value>,
}
impl DraftPose {
    pub fn clear(&mut self) {
        self.at = None;
        self.values.clear();
    }
    pub fn matches(&self, at: &Option<(String, f64)>) -> bool {
        self.at.is_some() && &self.at == at
    }
    pub fn set(&mut self, at: (String, f64), node: &str, prop: motif_core::Prop, value: motif_core::Value) {
        if self.at.as_ref() != Some(&at) {
            self.clear();
        }
        self.at = Some(at);
        self.values.insert((node.into(), prop), value);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Workspace {
    #[default]
    Drawing,
    Setup,
    Animate,
}

impl Workspace {
    pub const ALL: [Self; 3] = [Self::Drawing, Self::Setup, Self::Animate];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Drawing => "Рисование",
            Self::Setup => "Настройка",
            Self::Animate => "Анимация",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Tools,
    Layers,
    Script,
    Inspector,
    Timeline,
    Preview,
}

impl Panel {
    pub const ALL: [Self; 6] = [Self::Tools, Self::Layers, Self::Script, Self::Inspector, Self::Timeline, Self::Preview];
    pub fn name(self) -> &'static str {
        match self {
            Self::Tools => "Инструменты",
            Self::Layers => "Слои",
            Self::Script => "Скрипт",
            Self::Inspector => "Свойства",
            Self::Timeline => "Таймлайн",
            Self::Preview => "Предпросмотр",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Layout {
    pub tools: bool,
    pub layers: bool,
    pub script: bool,
    pub inspector: bool,
    pub timeline: bool,
    pub preview: bool,
    pub preview_width: f32,
    #[serde(skip)]
    pub right_tabs: bool,
    pub layers_width: f32,
    pub script_width: f32,
    pub inspector_width: f32,
    pub timeline_height: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            tools: true,
            layers: true,
            script: false,
            inspector: true,
            timeline: false,
            preview: false,
            preview_width: 320.0,
            right_tabs: false,
            layers_width: 210.0,
            script_width: 440.0,
            inspector_width: 264.0,
            timeline_height: 236.0,
        }
    }
}

impl Layout {
    pub fn for_workspace(space: Workspace) -> Self {
        Self { timeline: space == Workspace::Animate, ..Self::default() }
    }
    pub fn shown(self, panel: Panel) -> bool {
        match panel {
            Panel::Tools => self.tools,
            Panel::Layers => self.layers,
            Panel::Script => self.script,
            Panel::Inspector => self.inspector,
            Panel::Timeline => self.timeline,
            Panel::Preview => self.preview,
        }
    }
    pub fn set(&mut self, panel: Panel, shown: bool) {
        *match panel {
            Panel::Tools => &mut self.tools,
            Panel::Layers => &mut self.layers,
            Panel::Script => &mut self.script,
            Panel::Inspector => &mut self.inspector,
            Panel::Timeline => &mut self.timeline,
            Panel::Preview => &mut self.preview,
        } = shown;
    }
    fn focused(mut self, space: Workspace) -> Self {
        for panel in Panel::ALL {
            self.set(panel, false);
        }
        self.tools = true;
        self.timeline = space == Workspace::Animate;
        self
    }
    fn sanitize(&mut self) {
        fn bound(n: &mut f32, default: f32, lo: f32, hi: f32) {
            *n = if n.is_finite() { n.clamp(lo, hi) } else { default };
        }
        bound(&mut self.layers_width, 210.0, 160.0, 360.0);
        bound(&mut self.script_width, 440.0, 240.0, 900.0);
        bound(&mut self.inspector_width, 264.0, 240.0, 560.0);
        bound(&mut self.preview_width, 320.0, 240.0, 560.0);
        bound(&mut self.timeline_height, 236.0, 120.0, 520.0);
    }

    /// Temporarily compact sidebars, preserving the user's preferred layout and canvas space.
    #[cfg(test)]
    pub fn fit(self, width: f32, height: f32) -> Self {
        self.fit_prioritizing(width, height, Panel::Script)
    }
    pub fn fit_prioritizing(mut self, width: f32, height: f32, priority: Panel) -> Self {
        self.sanitize();
        let occupied = |s: &Self| {
            (if s.tools { 48.0 } else { 0.0 })
                + (if s.layers { s.layers_width } else { 28.0 })
                + (if s.script { s.script_width } else { 28.0 })
                + if s.right_tabs {
                    s.inspector_width.max(s.preview_width)
                } else {
                    (if s.inspector { s.inspector_width } else { 28.0 }) + (if s.preview { s.preview_width } else { 28.0 })
                }
        };
        if occupied(&self) + 320.0 > width && self.inspector && self.preview {
            self.right_tabs = true;
        }
        // Code is an explicit request: if open, prefer it to the layer list on small screens.
        if occupied(&self) + 320.0 > width && matches!(priority, Panel::Layers | Panel::Inspector | Panel::Preview) {
            self.script = false;
        }
        if occupied(&self) + 320.0 > width && self.layers {
            self.layers = false;
        }
        if occupied(&self) + 320.0 > width && self.script {
            self.script_width = (self.script_width - (occupied(&self) + 320.0 - width)).max(240.0);
        }
        if occupied(&self) + 320.0 > width && self.inspector {
            self.inspector_width = (self.inspector_width - (occupied(&self) + 320.0 - width)).max(240.0);
        }
        if self.right_tabs {
            self.preview_width = self.inspector_width;
        }
        if occupied(&self) + 320.0 > width && self.preview {
            self.preview_width = (self.preview_width - (occupied(&self) + 320.0 - width)).max(240.0);
        }
        if occupied(&self) + 320.0 > width && self.script {
            self.inspector = false;
            if priority != Panel::Preview {
                self.preview = false;
            }
            self.right_tabs = false;
        }
        if occupied(&self) + 320.0 > width {
            self.script = false;
        }
        self.timeline_height = self.timeline_height.min((height - 220.0).max(120.0));
        if height < 340.0 {
            self.timeline = false;
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub version: u32,
    pub workspace: Workspace,
    pub layouts: [Layout; 3],
    /// The live link with a game is on at start, on this port.
    pub link: bool,
    pub link_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self { version: 1, workspace: Workspace::Drawing, layouts: Workspace::ALL.map(Layout::for_workspace), link: false, link_port: motif_link::DEFAULT_PORT }
    }
}

pub(crate) struct Panels {
    pub settings: Settings,
    // A temporary layout while focused. Never serialized or written over a saved workspace.
    pub focus: Option<Layout>,
    pub priority: Panel,
}

impl Default for Panels {
    fn default() -> Self {
        Self { settings: Settings::default(), focus: None, priority: Panel::Script }
    }
}

impl Panels {
    pub fn layout(&self) -> Layout {
        self.focus.unwrap_or(self.settings.layouts[self.settings.workspace.index()])
    }
    pub fn layout_mut(&mut self) -> &mut Layout {
        self.focus.as_mut().unwrap_or(&mut self.settings.layouts[self.settings.workspace.index()])
    }
    pub fn switch(&mut self, space: Workspace) {
        if self.settings.workspace == space {
            return;
        }
        self.settings.workspace = space;
        if self.focus.is_some() {
            self.focus = Some(self.settings.layouts[space.index()].focused(self.settings.workspace));
        }
    }
    pub fn toggle_focus(&mut self) {
        self.focus = if self.focus.is_some() { None } else { Some(self.layout().focused(self.settings.workspace)) };
    }
    pub fn set(&mut self, panel: Panel, shown: bool) {
        self.layout_mut().set(panel, shown);
        if shown {
            self.priority = panel;
        }
    }
    pub fn toggle(&mut self, panel: Panel) {
        self.set(panel, !self.layout().shown(panel));
    }
    pub fn reset(&mut self) {
        self.settings.layouts[self.settings.workspace.index()] = Layout::for_workspace(self.settings.workspace);
        if self.focus.is_some() {
            self.focus = Some(self.settings.layouts[self.settings.workspace.index()].focused(self.settings.workspace));
        }
    }
}

pub(crate) struct Store {
    pub path: PathBuf,
    saved: String,
}
impl Store {
    pub fn load(path: PathBuf) -> (Self, Settings, Option<String>) {
        let result = std::fs::read_to_string(&path);
        let (settings, warning) = match result {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(mut s) if s.version == 1 => {
                    for layout in &mut s.layouts {
                        layout.sanitize();
                    }
                    (s, None)
                }
                _ => (Settings::default(), Some("Не удалось прочитать раскладку: использована стандартная".into())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(format!("Раскладка недоступна: {e}"))),
        };
        let saved = serde_json::to_string(&settings).unwrap_or_default();
        (Self { path, saved }, settings, warning)
    }
    pub fn save(&mut self, settings: &Settings) -> Result<(), String> {
        let text = serde_json::to_string(settings).map_err(|e| e.to_string())?;
        if text == self.saved {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::storage::atomic_write(&self.path, text.as_bytes())?;
        self.saved = text;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_is_temporary_even_across_workspaces() {
        let mut p = Panels::default();
        p.set(Panel::Script, true);
        p.layout_mut().inspector_width = 333.0;
        let before = p.settings.clone();
        p.toggle_focus();
        assert!(!p.layout().script);
        p.set(Panel::Timeline, true);
        p.layout_mut().inspector_width = 450.0;
        assert_eq!(p.settings, before);
        p.switch(Workspace::Animate);
        assert!(p.layout().timeline);
        p.toggle_focus();
        assert!(p.layout().timeline);
        p.switch(Workspace::Drawing);
        assert_eq!(p.layout(), before.layouts[0]);
    }
    #[test]
    fn compact_layout_reserves_canvas_and_preserves_preferences() {
        let l = Layout { script: true, timeline: true, ..Layout::default() };
        for width in [720.0, 900.0, 1093.0, 1280.0, 1366.0, 1920.0] {
            let c = l.fit(width, 460.0);
            let used = (if c.tools { 48.0 } else { 0.0 })
                + (if c.layers { c.layers_width } else { 28.0 })
                + (if c.script { c.script_width } else { 28.0 })
                + (if c.inspector { c.inspector_width } else { 28.0 })
                + 28.0;
            assert!(width - used >= 320.0, "{width}: {c:?}");
            assert!(460.0 - c.timeline_height >= 220.0);
        }
        assert!(l.layers && l.script);
        assert_eq!(l.fit(1920.0, 900.0), l);
    }
    #[test]
    fn settings_roundtrip_does_not_store_focus() {
        let mut p = Panels::default();
        p.switch(Workspace::Setup);
        p.layout_mut().layers_width = 300.0;
        p.set(Panel::Script, true);
        p.toggle_focus();
        let text = serde_json::to_string(&p.settings).unwrap();
        let recovered: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(recovered, p.settings);
        assert!(recovered.layouts[1].script);
        assert!(!text.contains("focus"));
    }
    #[test]
    fn preferences_reload_and_recover_from_invalid_files() {
        let path = std::env::temp_dir().join(format!("motif-workspace-test-{}.json", std::process::id()));
        let (mut store, mut settings, _) = Store::load(path.clone());
        settings.workspace = Workspace::Setup;
        settings.layouts[1].script_width = 551.0;
        // Force a write also when a previous failed test left the same file behind.
        store.saved.clear();
        store.save(&settings).unwrap();
        assert_eq!(Store::load(path.clone()).1, settings);
        std::fs::write(&path, r#"{"version":999}"#).unwrap();
        let (_, fallback, warning) = Store::load(path.clone());
        assert_eq!(fallback, Settings::default());
        assert!(warning.is_some());
        std::fs::write(&path, "broken").unwrap();
        assert!(Store::load(path.clone()).2.is_some());
        std::fs::remove_file(path).unwrap();
    }
}
