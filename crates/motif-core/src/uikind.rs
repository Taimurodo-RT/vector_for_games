//! What a piece of game interface becomes in Godot (`ui progress`), and the parts it is drawn
//! from (`part fill`).

/// The Godot control a piece of interface (`ui …`) is styled for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiRole {
    /// `ui` alone: a `Button` when it has states, a `PanelContainer` when not.
    #[default]
    Auto,
    Button,
    Panel,
    /// `ProgressBar`: `part background` and `part fill`.
    Progress,
    /// `CheckBox`: `part unchecked` and `part checked`, the label as `part text`.
    Check,
    /// `HSlider` or `VSlider`: `part track`, `part grabber` and, if any, `part fill`.
    Slider,
    /// `HScrollBar` or `VScrollBar`: `part track` and `part grabber`.
    Scroll,
    /// `Label`: the color, size, font, outline and shadow of a text.
    Label,
}

/// One part of a composite piece (`part grabber`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiPart {
    Background,
    Fill,
    Unchecked,
    Checked,
    Track,
    Grabber,
    /// The caption: drawn by Godot with the theme's font, not baked into the picture.
    Text,
}

impl UiRole {
    pub const ALL: [UiRole; 8] = [UiRole::Auto, UiRole::Button, UiRole::Panel, UiRole::Progress, UiRole::Check, UiRole::Slider, UiRole::Scroll, UiRole::Label];

    /// The word after `ui`; empty for [`UiRole::Auto`].
    pub fn keyword(self) -> &'static str {
        match self {
            UiRole::Auto => "",
            UiRole::Button => "button",
            UiRole::Panel => "panel",
            UiRole::Progress => "progress",
            UiRole::Check => "check",
            UiRole::Slider => "slider",
            UiRole::Scroll => "scroll",
            UiRole::Label => "label",
        }
    }

    pub fn from_keyword(w: &str) -> Option<UiRole> {
        UiRole::ALL.into_iter().find(|r| *r != UiRole::Auto && r.keyword() == w)
    }

    /// The parts this kind of piece is made of; the required ones first.
    pub fn parts(self) -> &'static [UiPart] {
        match self {
            UiRole::Auto | UiRole::Button => &[UiPart::Text],
            UiRole::Panel => &[],
            UiRole::Progress => &[UiPart::Background, UiPart::Fill],
            UiRole::Check => &[UiPart::Unchecked, UiPart::Checked, UiPart::Text],
            UiRole::Slider => &[UiPart::Track, UiPart::Grabber, UiPart::Fill],
            UiRole::Scroll => &[UiPart::Track, UiPart::Grabber],
            UiRole::Label => &[UiPart::Text],
        }
    }

    /// The parts the piece cannot do without.
    pub fn required(self) -> &'static [UiPart] {
        match self {
            UiRole::Progress => &[UiPart::Background, UiPart::Fill],
            UiRole::Check => &[UiPart::Unchecked, UiPart::Checked],
            UiRole::Slider | UiRole::Scroll => &[UiPart::Track, UiPart::Grabber],
            _ => &[],
        }
    }
}

impl UiPart {
    pub const ALL: [UiPart; 7] = [UiPart::Background, UiPart::Fill, UiPart::Unchecked, UiPart::Checked, UiPart::Track, UiPart::Grabber, UiPart::Text];

    pub fn keyword(self) -> &'static str {
        match self {
            UiPart::Background => "background",
            UiPart::Fill => "fill",
            UiPart::Unchecked => "unchecked",
            UiPart::Checked => "checked",
            UiPart::Track => "track",
            UiPart::Grabber => "grabber",
            UiPart::Text => "text",
        }
    }

    pub fn from_keyword(w: &str) -> Option<UiPart> {
        UiPart::ALL.into_iter().find(|p| p.keyword() == w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_go_both_ways() {
        for r in UiRole::ALL.into_iter().skip(1) {
            assert_eq!(UiRole::from_keyword(r.keyword()), Some(r));
        }
        assert_eq!(UiRole::from_keyword(""), None);
        for p in UiPart::ALL {
            assert_eq!(UiPart::from_keyword(p.keyword()), Some(p));
        }
        for r in UiRole::ALL {
            assert!(r.required().iter().all(|p| r.parts().contains(p)));
        }
    }
}
