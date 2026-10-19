use super::{
    DiagramError,
    profile::{MONO, SANS},
};
use resvg::usvg::{
    self,
    fontdb::{Database, Family, ID, Query, Style, Weight},
};
use std::sync::Arc;

pub(super) struct Fonts {
    db: Arc<Database>,
}
impl Fonts {
    pub fn new() -> Result<Self, DiagramError> {
        let mut db = Database::new();
        for bytes in [
            include_bytes!("../../../assets/product-fonts/TauInter-400.ttf").as_slice(),
            include_bytes!("../../../assets/product-fonts/TauInter-500.ttf").as_slice(),
            include_bytes!("../../../assets/product-fonts/TauInter-600.ttf").as_slice(),
            include_bytes!("../../../assets/product-fonts/TauInter-650.ttf").as_slice(),
            include_bytes!("../../../assets/product-fonts/TauInter-700.ttf").as_slice(),
        ] {
            db.load_font_data(bytes.to_vec());
        }
        // Explicit local fallback, never a directory scan. Apple fonts are not redistributed.
        #[cfg(target_os = "macos")]
        for name in [
            "Arial Unicode.ttf",
            "Arial.ttf",
            "Arial Bold.ttf",
            "Arial Italic.ttf",
            "Arial Bold Italic.ttf",
        ] {
            let bytes = std::fs::read(format!("/System/Library/Fonts/Supplemental/{name}"))
                .map_err(|_| DiagramError::FontUnavailable)?;
            db.load_font_data(bytes);
        }
        #[cfg(target_os = "macos")]
        db.load_font_data(
            std::fs::read("/System/Library/Fonts/Menlo.ttc")
                .map_err(|_| DiagramError::FontUnavailable)?,
        );
        db.set_monospace_family(MONO);
        db.set_sans_serif_family(SANS);
        db.set_serif_family("TauInter");
        let result = Self { db: Arc::new(db) };
        let primary = select(&result.db, SANS, 400, false).ok_or(DiagramError::FontUnavailable)?;
        // Real fixtures depend on these glyphs. Never silently substitute tofu.
        for c in ['α', '✓', '≤'] {
            if !has(&result.db, primary, c) && fallback(&result.db, c, &[primary]).is_none() {
                return Err(DiagramError::FontUnavailable);
            }
        }
        Ok(result)
    }
    pub fn covers(&self, text: &str) -> bool {
        text.chars()
            .all(|c| c.is_whitespace() || self.db.faces().any(|f| has(&self.db, f.id, c)))
    }
    pub fn measure(&self, text: &str, style: &merman::svg::TextStyle) -> f64 {
        let weight = match style.font_weight.as_deref() {
            Some("bold") => 700,
            Some(s) => s.parse().unwrap_or(400),
            None => 400,
        };
        let italic = matches!(style.font_style.as_deref(), Some("italic" | "oblique"));
        let Some(primary) = select(
            &self.db,
            style.font_family.as_deref().unwrap_or(SANS),
            weight,
            italic,
        ) else {
            return 0.;
        };
        let mut runs: Vec<(ID, String)> = Vec::new();
        for c in text.chars() {
            let id = if has(&self.db, primary, c) {
                primary
            } else {
                fallback(&self.db, c, &[primary]).unwrap_or(primary)
            };
            if let Some((_, run)) = runs.last_mut().filter(|(last, _)| *last == id) {
                run.push(c);
            } else {
                runs.push((id, c.to_string()));
            }
        }
        runs.iter()
            .map(|(id, text)| {
                self.db
                    .with_face_data(*id, |data, index| {
                        let Some(face) = rustybuzz::Face::from_slice(data, index) else {
                            return 0.;
                        };
                        let mut buffer = rustybuzz::UnicodeBuffer::new();
                        buffer.push_str(text);
                        buffer.guess_segment_properties();
                        let shaped = rustybuzz::shape(&face, &[], buffer);
                        shaped
                            .glyph_positions()
                            .iter()
                            .map(|g| g.x_advance as f64)
                            .sum::<f64>()
                            / face.units_per_em() as f64
                            * style.font_size
                    })
                    .unwrap_or(0.)
            })
            .sum()
    }
    pub fn options(&self) -> usvg::Options<'static> {
        usvg::Options {
            fontdb: self.db.clone(),
            font_family: "TauInter".into(),
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            font_resolver: usvg::FontResolver {
                select_font: Box::new(|font, db| {
                    let family = if font.families().iter().any(|f| {
                        matches!(f, usvg::FontFamily::Monospace)
                            || matches!(f, usvg::FontFamily::Named(n) if n == MONO)
                    }) {
                        MONO
                    } else {
                        SANS
                    };
                    select(
                        db,
                        family,
                        font.weight(),
                        font.style() != usvg::FontStyle::Normal,
                    )
                }),
                select_fallback: Box::new(|c, used, db| fallback(db, c, used)),
            },
            ..Default::default()
        }
    }
}
fn select(db: &Database, family: &str, weight: u16, italic: bool) -> Option<ID> {
    let mono = family
        .split(',')
        .any(|s| matches!(s.trim().trim_matches(['\'', '"']), "Menlo" | "monospace"));
    db.query(&Query {
        families: &[Family::Name(if mono {
            MONO
        } else if italic {
            "Arial"
        } else {
            SANS
        })],
        weight: Weight(weight),
        style: if italic { Style::Italic } else { Style::Normal },
        ..Default::default()
    })
}
fn has(db: &Database, id: ID, c: char) -> bool {
    db.with_face_data(id, |data, index| {
        rustybuzz::Face::from_slice(data, index).is_some_and(|f| f.glyph_index(c).is_some())
    })
    .unwrap_or(false)
}
fn fallback(db: &Database, c: char, used: &[ID]) -> Option<ID> {
    // Match the nearest available fallback weight/style to the primary face in both paths.
    let primary = used.first().and_then(|id| db.face(*id));
    db.faces()
        .filter(|f| !used.contains(&f.id) && has(db, f.id, c))
        .min_by_key(|f| {
            let weight = primary.map_or(400, |p| p.weight.0);
            let style = primary.map_or(Style::Normal, |p| p.style);
            (
                u16::from(f.style != style),
                f.weight.0.abs_diff(weight),
                u16::from(!f.families.iter().any(|(n, _)| n == "Arial Unicode MS")),
            )
        })
        .map(|f| f.id)
}
