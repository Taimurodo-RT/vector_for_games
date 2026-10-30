//! A small portable CSS font-family resolver. It never reads system font folders
//! or fetches @font-face resources while converting a component.
use crate::css;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Family {
    #[default]
    Plex,
    Sans,
}
impl Family {
    pub fn bytes(self, bold: bool) -> &'static [u8] {
        match self {
            Self::Plex => {
                if bold {
                    motif_core::text::BOLD_FONT
                } else {
                    motif_core::text::DEFAULT_FONT
                }
            }
            Self::Sans => motif_script::fonts::css_sans_data(bold),
        }
    }
    /// Includes its own leading space so the existing optional MSC attributes stay simple.
    pub fn attribute(self, bold: bool) -> String {
        match self {
            Self::Plex => {
                if bold {
                    " font bold".into()
                } else {
                    String::new()
                }
            }
            Self::Sans => format!(" font \"{}\"", if bold { motif_script::fonts::CSS_SANS_BOLD } else { motif_script::fonts::CSS_SANS_REGULAR }),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Plex => "IBM Plex Sans",
            Self::Sans => "Liberation Sans",
        }
    }
}

pub(crate) fn select(value: &str, inherited: Family, fallback: Family, warnings: &mut Vec<String>) -> Family {
    if matches!(value.trim(), "inherit" | "unset") {
        return inherited;
    }
    let mut skipped = Vec::new();
    for item in css::split(value, ',') {
        let name = item.trim().trim_matches(['\'', '"']).split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
        let choice = match name.as_str() {
            "arial" | "helvetica" | "helvetica neue" | "liberation sans" | "sans-serif" => Some(Family::Sans),
            "ibm plex sans" => Some(Family::Plex),
            "system-ui" | "ui-sans-serif" => {
                warnings.push(format!("CSS font-family {name}: использована переносимая Liberation Sans; системный шрифт браузера может отличаться"));
                Some(Family::Sans)
            }
            _ => None,
        };
        if let Some(choice) = choice {
            if !skipped.is_empty() {
                warnings.push(format!("CSS font-family: недоступны {}; выбран {}", skipped.join(", "), choice.name()));
            }
            return choice;
        }
        skipped.push(item.trim().to_owned());
    }
    warnings.push(format!(
        "CSS font-family {value}: семейство не встроено; использован {}. Внешние и системные шрифты автоматически не загружаются",
        fallback.name()
    ));
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_lists_preserve_order_and_css_inheritance() {
        let mut warnings = Vec::new();
        assert_eq!(select("'Not Installed', Arial, serif", Family::Plex, Family::Plex, &mut warnings), Family::Sans);
        assert!(warnings.iter().any(|w| w.contains("Not Installed") && w.contains("Liberation Sans")));
        warnings.clear();
        assert_eq!(select("'IBM Plex Sans', Arial", Family::Sans, Family::Sans, &mut warnings), Family::Plex);
        assert_eq!(select("inherit", Family::Sans, Family::Plex, &mut warnings), Family::Sans);
        assert_eq!(select("sans-serif", Family::Plex, Family::Plex, &mut warnings), Family::Sans);
        assert!(warnings.is_empty());
        assert_eq!(select("monospace", Family::Plex, Family::Plex, &mut warnings), Family::Plex);
        assert!(warnings.iter().any(|w| w.contains("monospace") && w.contains("не встроено")));
    }
    #[test]
    fn family_faces_and_script_attributes_select_the_same_embedded_font() {
        for (bold, name) in [(false, motif_script::fonts::CSS_SANS_REGULAR), (true, motif_script::fonts::CSS_SANS_BOLD)] {
            let parsed = motif_script::parse(&format!("artboard 200x100\ntext t content \"P L A Y\" font-size 13{}\n", Family::Sans.attribute(bold))).unwrap();
            assert_eq!(Family::Sans.bytes(bold), motif_script::fonts::load(Some(name), None).unwrap().as_slice());
            assert!(parsed.doc.nodes[0].text.as_ref().unwrap().font.as_ref().unwrap().name.starts_with("LiberationSans-"));
        }
        let shape = motif_core::TextShape::new("P L A Y", Family::Sans.bytes(true)).unwrap();
        let bounds = shape.bounds(&motif_core::TextStyle {
            size: 13.0,
            line_height: 1.2,
            spacing: 0.0,
            align: motif_core::TextAlign::Left,
            reveal: 1.0,
            wave: 0.0,
            phase: 0.0,
        });
        // Headless Edge with explicit Arial 700 13px and fontKerning=normal: 43.836914.
        assert!((bounds.width() - 43.836914).abs() < 0.01, "{}", bounds.width());
    }
}
