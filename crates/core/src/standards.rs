//! Technical Standards (Normas Técnicas): drawing standards that govern line weights, sheet layouts,
//! margins, folding, object styles, and dimensioning conventions.
//!
//! Presets include:
//! - **NBR (Brasil — ABNT)**: NBR 16861 (Linhas e escrita), NBR 16752 (Folhas e leiautes),
//!   NBR 17006 (Princípios de representação / 1º Diedro), NBR 17067 (Dobramento de folha / Módulo A4),
//!   NBR 17068 (Cotagem), NBR 6492 (Documentação técnica de projetos arquitetônicos).
//! - **ISO (International)**: ISO 128 (General principles / lines), ISO 5457 (Drawing sheets),
//!   ISO 129-1 (Dimensioning), ISO 7200 (Title blocks), ISO 3098 (Lettering).
//! - **ANSI / AIA (United States)**: ASME Y14.2 (Line conventions), ASME Y14.1 / Y14.1M (Sheet sizes:
//!   Arch D/C/E, ANSI A-E), ASME Y14.5 (Dimensioning / 3rd angle projection), AIA CAD Guidelines,
//!   and United States National CAD Standard (NCS).
//! - **EN / Eurocodes (European Union)**: EN ISO 128, EN ISO 5457, EN ISO 129, EN ISO 7200, Eurocodes.
//! - **JIS (Japan / Asia)**: JIS Z 8310, JIS B 0001, JIS A 0150 (3rd angle projection).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pen::{LineWeights, MAX_PEN, Pen, Row, ScalePens};
use crate::styles::ObjectStyle;
use crate::units::Units;

/// A technical standard preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TechnicalStandard {
    /// NBR (Brasil — ABNT): NBR 16861, NBR 16752, NBR 17006, NBR 17067, NBR 17068, NBR 6492.
    #[default]
    #[serde(rename = "nbr")]
    Nbr,
    /// ISO (International): ISO 128, ISO 5457, ISO 129-1, ISO 7200, ISO 3098.
    #[serde(rename = "iso")]
    Iso,
    /// ANSI / AIA (United States): ASME Y14.1, ASME Y14.2, ASME Y14.5, AIA CAD Guidelines, US NCS.
    #[serde(rename = "ansi")]
    Ansi,
    /// EN / Eurocodes (European Union): EN ISO 128, EN ISO 5457, EN ISO 129, EN ISO 7200.
    #[serde(rename = "eu")]
    Eu,
    /// JIS (Japan): JIS Z 8310, JIS B 0001, JIS A 0150.
    #[serde(rename = "jis")]
    Jis,
}

/// Orthographic projection angle / method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionMethod {
    /// First-angle projection (1º Diedro: standard in Brazil NBR 17006, ISO 128, EN/Europe).
    FirstAngle,
    /// Third-angle projection (3º Diedro: standard in USA ANSI/ASME Y14.5, Japan JIS).
    ThirdAngle,
}

impl ProjectionMethod {
    pub fn name(self) -> &'static str {
        match self {
            ProjectionMethod::FirstAngle => "1st Angle (1º Diedro)",
            ProjectionMethod::ThirdAngle => "3rd Angle (3º Diedro)",
        }
    }
}

/// Sheet margin rules and title block constraints for a standard.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetMargins {
    /// Left margin in mm (filing/binding margin: e.g. 25 mm for NBR 16752, 20 mm for ISO 5457, 19.05 mm for ANSI).
    pub left: f64,
    /// Top margin in mm.
    pub top: f64,
    /// Right margin in mm.
    pub right: f64,
    /// Bottom margin in mm.
    pub bottom: f64,
    /// Title block width in mm (e.g. 178 mm for NBR 16752, 180 mm for ISO 7200).
    pub title_width: f64,
    /// Title block height in mm.
    pub title_height: f64,
    /// Whether standard folding marks (NBR 17067) should be drawn on larger sheets.
    pub folding_marks: bool,
}

/// Dimension terminator style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terminator {
    /// 45-degree oblique architectural tick (NBR 17068, NBR 6492, AIA CAD).
    #[default]
    Tick45,
    /// Filled arrowhead (ISO 129-1, ASME Y14.5).
    FilledArrow,
}

/// Dimension text placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextPlacement {
    /// Above the continuous dimension line (NBR 17068, ISO 129-1).
    #[default]
    AboveLine,
    /// Centered within a break in the dimension line (ASME Y14.5).
    InlineBreak,
}

/// Dimension and annotation styling rules.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimensionStyle {
    pub terminator: Terminator,
    pub text_placement: TextPlacement,
    /// How far the witness line extends past the dimension line (mm on paper, e.g. 2.5 - 3.0 mm).
    pub witness_overshoot_mm: f64,
    /// Gap between measured point and start of witness line (mm on paper, e.g. 1.0 - 2.0 mm).
    pub witness_gap_mm: f64,
    /// Standard dimension text height (mm on paper, e.g. 2.5 mm).
    pub text_height_mm: f64,
}

impl TechnicalStandard {
    pub const ALL: [TechnicalStandard; 5] =
        [TechnicalStandard::Nbr, TechnicalStandard::Iso, TechnicalStandard::Ansi, TechnicalStandard::Eu, TechnicalStandard::Jis];

    /// Short code: nbr, iso, ansi, eu, jis.
    pub fn code(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "nbr",
            TechnicalStandard::Iso => "iso",
            TechnicalStandard::Ansi => "ansi",
            TechnicalStandard::Eu => "eu",
            TechnicalStandard::Jis => "jis",
        }
    }

    /// Parse code string into standard.
    pub fn from_code(s: &str) -> Option<TechnicalStandard> {
        match s.trim().to_ascii_lowercase().as_str() {
            "nbr" | "abnt" | "brasil" | "brazil" => Some(TechnicalStandard::Nbr),
            "iso" | "international" => Some(TechnicalStandard::Iso),
            "ansi" | "asme" | "aia" | "us" | "usa" => Some(TechnicalStandard::Ansi),
            "eu" | "en" | "euro" | "europe" => Some(TechnicalStandard::Eu),
            "jis" | "japan" => Some(TechnicalStandard::Jis),
            _ => None,
        }
    }

    /// Display label for the standard.
    pub fn label(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "NBR (Brasil — ABNT)",
            TechnicalStandard::Iso => "ISO (International)",
            TechnicalStandard::Ansi => "ANSI / AIA (United States)",
            TechnicalStandard::Eu => "EN / Eurocodes (European Union)",
            TechnicalStandard::Jis => "JIS (Japan)",
        }
    }

    /// Short display name for tags or ribbons.
    pub fn short_name(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "NBR",
            TechnicalStandard::Iso => "ISO",
            TechnicalStandard::Ansi => "ANSI",
            TechnicalStandard::Eu => "EN/EU",
            TechnicalStandard::Jis => "JIS",
        }
    }

    /// Full formal name.
    pub fn name(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "Normas Brasileiras (ABNT / NBR)",
            TechnicalStandard::Iso => "International Organization for Standardization (ISO)",
            TechnicalStandard::Ansi => "American National Standards (ANSI / ASME / AIA)",
            TechnicalStandard::Eu => "European Standards (CEN / EN / Eurocodes)",
            TechnicalStandard::Jis => "Japanese Industrial Standards (JIS)",
        }
    }

    /// Geographic region.
    pub fn region(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "Brasil",
            TechnicalStandard::Iso => "International / Global",
            TechnicalStandard::Ansi => "United States",
            TechnicalStandard::Eu => "European Union",
            TechnicalStandard::Jis => "Japan",
        }
    }

    /// Governing normalization body.
    pub fn governing_body(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "ABNT — Associação Brasileira de Normas Técnicas",
            TechnicalStandard::Iso => "ISO — International Organization for Standardization",
            TechnicalStandard::Ansi => "ANSI / ASME / AIA (American Institute of Architects)",
            TechnicalStandard::Eu => "CEN — European Committee for Standardization",
            TechnicalStandard::Jis => "JISC — Japanese Industrial Standards Committee",
        }
    }

    /// Specific reference standard documents included in this preset.
    pub fn reference_standards(self) -> &'static [&'static str] {
        match self {
            TechnicalStandard::Nbr => &[
                "NBR 16861: Desenho técnico — Requisitos para representação de linhas e escrita",
                "NBR 16752: Desenho técnico — Formatos de folhas de desenho e leiautes (Margem 25 mm)",
                "NBR 17006: Desenho técnico — Princípios gerais de representação (1º Diedro)",
                "NBR 17067: Desenho técnico — Dobramento de folhas (Módulo final A4)",
                "NBR 17068: Desenho técnico — Cotagem (Regras de linhas e texto)",
                "NBR 6492: Documentação técnica para projetos arquitetônicos",
            ],
            TechnicalStandard::Iso => &[
                "ISO 128: Technical product documentation — General principles of representation",
                "ISO 5457: Technical product documentation — Sizes and layout of drawing sheets",
                "ISO 129-1: Technical product documentation — Presentation of dimensions and tolerances",
                "ISO 7200: Technical product documentation — Data fields in title blocks",
                "ISO 3098: Technical product documentation — Lettering",
                "ISO 7518 / ISO 7519: Technical drawings — Construction drawings",
            ],
            TechnicalStandard::Ansi => &[
                "ASME Y14.2: Line Conventions and Lettering",
                "ASME Y14.1 / Y14.1M: Drawing Sheet Size and Format (Arch & ANSI)",
                "ASME Y14.5: Dimensioning and Tolerancing (3rd Angle Projection)",
                "AIA CAD Guidelines: Layer Guidelines, Line Weights, and Sheet Layouts",
                "United States National CAD Standard (NCS)",
            ],
            TechnicalStandard::Eu => &[
                "EN ISO 128: Princípios gerais de representação técnica",
                "EN ISO 5457: Formatos e leiautes de folhas de desenho",
                "EN ISO 129: Indicação de cotas e tolerâncias",
                "EN ISO 7200: Campos de dados em legendas de desenho",
                "EN 1990 — EN 1999: Eurocodes (Regras de dimensionamento e documentação)",
            ],
            TechnicalStandard::Jis => &[
                "JIS Z 8310: Princípios gerais de desenho técnico",
                "JIS B 0001: Desenho técnico e documentação mecânica",
                "JIS A 0150: Desenhos para arquitetura e construção",
            ],
        }
    }

    /// Summary description explaining the standard rules.
    pub fn description(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => {
                "Normas ABNT brasileiras completas: NBR 16861 (espessuras escalonadas em raiz de 2), \
                 NBR 16752 (margem esquerda de 25 mm para fixação, legenda de 178 mm), NBR 17006 (1º diedro), \
                 NBR 17067 (marcas de dobra para arquivamento A4), NBR 17068 (cotagem com ticks a 45° acima da linha) \
                 e NBR 6492 (hierarquia de alvenarias e elementos arquitetônicos cortados a 0.50 mm)."
            }
            TechnicalStandard::Iso => {
                "Normas técnicas internacionais ISO: ISO 128 (grupo métrico de espessuras de linha, 1º ângulo), \
                 ISO 5457 (folhas métricas A0 a A4 com margem de arquivamento de 20 mm), ISO 129-1 (cotagem métrica \
                 com setas cheias ou traços), e ISO 7200 (legenda de 180 mm)."
            }
            TechnicalStandard::Ansi => {
                "Padrões americanos ANSI / ASME / AIA: ASME Y14.2 (espessuras em polegadas e NCS), ASME Y14.1 / AIA \
                 (formatos arquitetônicos Arch D, Arch C, Arch E e ANSI A-E), ASME Y14.5 (projeção no 3º diedro, \
                 cotagem com texto centralizado ou sobre a linha), unidades em pés e polegadas fracionárias (ft-in)."
            }
            TechnicalStandard::Eu => {
                "Normas europeias harmonizadas EN ISO e Eurocodes: EN ISO 128 e 5457 (formatos métricos, 1º diedro, \
                 margem de arquivamento de 20 mm), EN ISO 129 (cotagem com vírgula decimal) e regras construtivas dos Eurocodes."
            }
            TechnicalStandard::Jis => {
                "Normas industriais japonesas (JIS): JIS Z 8310 e JIS A 0150 (projeção no 3º diedro, formatos métricos A0 a A4, \
                 espessuras métricas padronizadas)."
            }
        }
    }

    /// Projection method (1st angle vs 3rd angle).
    pub fn projection_method(self) -> ProjectionMethod {
        match self {
            TechnicalStandard::Nbr | TechnicalStandard::Iso | TechnicalStandard::Eu => ProjectionMethod::FirstAngle,
            TechnicalStandard::Ansi | TechnicalStandard::Jis => ProjectionMethod::ThirdAngle,
        }
    }

    /// Default unit system suggested by this standard.
    pub fn default_units(self) -> Units {
        match self {
            TechnicalStandard::Nbr => Units::Metres,
            TechnicalStandard::Ansi => Units::FeetInches,
            TechnicalStandard::Iso | TechnicalStandard::Eu | TechnicalStandard::Jis => Units::Millimetres,
        }
    }

    /// Default sheet size for new projects or new sheets.
    pub fn default_sheet_size(self) -> &'static str {
        match self {
            TechnicalStandard::Nbr => "A1",
            TechnicalStandard::Ansi => "ARCH D",
            TechnicalStandard::Iso | TechnicalStandard::Eu | TechnicalStandard::Jis => "A3",
        }
    }

    /// Sheet margins and title block constraints for `size`.
    pub fn sheet_margins(self, size: &str) -> SheetMargins {
        let size_upper = crate::model::paper_name(size);
        match self {
            TechnicalStandard::Nbr => {
                // NBR 16752: Margem esquerda de 25 mm em todos os formatos.
                // Outras margens: 10 mm para A0 e A1; 7 mm para A2, A3 e A4.
                let other = if matches!(size_upper.as_str(), "A0" | "A1") { 10.0 } else { 7.0 };
                let folding_marks = matches!(size_upper.as_str(), "A0" | "A1" | "A2" | "A3");
                SheetMargins {
                    left: 25.0,
                    top: other,
                    right: other,
                    bottom: other,
                    title_width: 178.0, // NBR 16752: max 178 mm
                    title_height: 40.0,
                    folding_marks,
                }
            }
            TechnicalStandard::Iso | TechnicalStandard::Eu => {
                // ISO 5457: Filing margin on left 20 mm, others 10 mm (or 7mm for A4).
                let other = if size_upper == "A4" { 7.0 } else { 10.0 };
                SheetMargins {
                    left: 20.0,
                    top: other,
                    right: other,
                    bottom: other,
                    title_width: 180.0, // ISO 7200
                    title_height: 40.0,
                    folding_marks: false,
                }
            }
            TechnicalStandard::Ansi => {
                // ASME Y14.1 / AIA NCS: 0.75 in (19.05 mm) left binding margin, 0.5 in (12.7 mm) other borders.
                SheetMargins {
                    left: 19.05,
                    top: 12.7,
                    right: 12.7,
                    bottom: 12.7,
                    title_width: 180.0,
                    title_height: 40.0,
                    folding_marks: false,
                }
            }
            TechnicalStandard::Jis => {
                let other = if size_upper == "A4" { 5.0 } else { 10.0 };
                SheetMargins {
                    left: 20.0,
                    top: other,
                    right: other,
                    bottom: other,
                    title_width: 180.0,
                    title_height: 40.0,
                    folding_marks: false,
                }
            }
        }
    }

    /// Line weights table (model scales and annotation) conforming to this standard.
    pub fn line_weights(self, units: Units) -> LineWeights {
        match self {
            TechnicalStandard::Nbr => {
                // NBR 16861 line series:
                // 0.13, 0.15, 0.18, 0.20, 0.25, 0.30, 0.35, 0.40, 0.50, 0.60, 0.70, 0.80, 1.00, 1.20, 1.40, 2.00
                const NBR_WIDTHS: [f64; MAX_PEN as usize] =
                    [0.13, 0.15, 0.18, 0.20, 0.25, 0.30, 0.35, 0.40, 0.50, 0.60, 0.70, 0.80, 1.00, 1.20, 1.40, 2.00];
                let scales: &[f64] = &[10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 500.0];
                let row = |k: f64| -> Row {
                    let mut r = [0.0; MAX_PEN as usize];
                    for (w, base) in r.iter_mut().zip(&NBR_WIDTHS) {
                        *w = ((base * k * 100.0).round() / 100.0).clamp(0.01, 20.0);
                    }
                    r
                };
                LineWeights {
                    model: scales.iter().map(|&n| ScalePens { scale: n, widths: row((100.0 / n).powf(0.25)) }).collect(),
                    annotation: NBR_WIDTHS,
                }
            }
            TechnicalStandard::Ansi => {
                // ASME Y14.2 & AIA NCS imperial line weights
                LineWeights::default_for(if units.imperial() { units } else { Units::FeetInches })
            }
            TechnicalStandard::Iso | TechnicalStandard::Eu | TechnicalStandard::Jis => LineWeights::default_for(Units::Millimetres),
        }
    }

    /// Object styles (category projection and cut pen assignments) conforming to this standard.
    pub fn object_styles(self) -> BTreeMap<&'static str, ObjectStyle> {
        let mut map = BTreeMap::new();
        match self {
            TechnicalStandard::Nbr => {
                // NBR 6492: Documentação técnica para projetos arquitetônicos
                // Hierarquia clássica brasileira:
                // - Linha grossa (0.50 mm - Pen 10): alvenarias cortadas, pisos cortados, escadas cortadas.
                // - Linha muito grossa (0.70 mm - Pen 12): elementos estruturais cortados (pilares, vigas de concreto).
                // - Linha média (0.35 mm - Pen 7): esquadrias cortadas (portas, janelas), guarda-corpos.
                // - Linha fina / projeção (0.18 mm / 0.20 mm - Pen 3 ou 4): esquadrias vistas, projeções.
                // - Linha média-fina (0.25 mm - Pen 5): contornos de piso em vista, degraus em vista.
                map.insert("wall", ObjectStyle { projection: Pen(4), cut: Pen(10) }); // 0.20mm / 0.50mm
                map.insert("column", ObjectStyle { projection: Pen(5), cut: Pen(12) }); // 0.25mm / 0.70mm
                map.insert("floor", ObjectStyle { projection: Pen(4), cut: Pen(10) }); // 0.20mm / 0.50mm
                map.insert("roof", ObjectStyle { projection: Pen(5), cut: Pen(10) }); // 0.25mm / 0.50mm
                map.insert("door", ObjectStyle { projection: Pen(3), cut: Pen(7) }); // 0.18mm / 0.35mm
                map.insert("window", ObjectStyle { projection: Pen(3), cut: Pen(7) }); // 0.18mm / 0.35mm
                map.insert("railing", ObjectStyle { projection: Pen(5), cut: Pen(7) }); // 0.25mm / 0.35mm
                map.insert("stair", ObjectStyle { projection: Pen(5), cut: Pen(10) }); // 0.25mm / 0.50mm
                map.insert("beam", ObjectStyle { projection: Pen(5), cut: Pen(12) }); // 0.25mm / 0.70mm
                map.insert("footing", ObjectStyle { projection: Pen(5), cut: Pen(10) }); // 0.25mm / 0.50mm
                map.insert("ceiling", ObjectStyle { projection: Pen(3), cut: Pen(7) }); // 0.18mm / 0.35mm
                map.insert("mass", ObjectStyle { projection: Pen(5), cut: Pen(5) }); // 0.25mm / 0.25mm
            }
            TechnicalStandard::Ansi => {
                // AIA CAD Layering & National CAD Standard
                map.insert("wall", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("column", ObjectStyle { projection: Pen(5), cut: Pen(11) });
                map.insert("floor", ObjectStyle { projection: Pen(4), cut: Pen(8) });
                map.insert("roof", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("door", ObjectStyle { projection: Pen(2), cut: Pen(6) });
                map.insert("window", ObjectStyle { projection: Pen(2), cut: Pen(6) });
                map.insert("railing", ObjectStyle { projection: Pen(4), cut: Pen(7) });
                map.insert("stair", ObjectStyle { projection: Pen(4), cut: Pen(9) });
                map.insert("beam", ObjectStyle { projection: Pen(4), cut: Pen(11) });
                map.insert("footing", ObjectStyle { projection: Pen(4), cut: Pen(9) });
                map.insert("ceiling", ObjectStyle { projection: Pen(2), cut: Pen(6) });
                map.insert("mass", ObjectStyle { projection: Pen(4), cut: Pen(4) });
            }
            TechnicalStandard::Iso | TechnicalStandard::Eu | TechnicalStandard::Jis => {
                // ISO 128 standard defaults
                map.insert("wall", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("column", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("floor", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("roof", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("door", ObjectStyle { projection: Pen(4), cut: Pen(6) });
                map.insert("window", ObjectStyle { projection: Pen(4), cut: Pen(6) });
                map.insert("railing", ObjectStyle { projection: Pen(6), cut: Pen(10) });
                map.insert("stair", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("beam", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("footing", ObjectStyle { projection: Pen(4), cut: Pen(10) });
                map.insert("ceiling", ObjectStyle { projection: Pen(4), cut: Pen(6) });
                map.insert("mass", ObjectStyle { projection: Pen(4), cut: Pen(4) });
            }
        }
        map
    }

    /// Dimension and annotation styling rules.
    pub fn dimension_style(self) -> DimensionStyle {
        match self {
            TechnicalStandard::Nbr => DimensionStyle {
                terminator: Terminator::Tick45,           // NBR 17068: traço oblíquo a 45°
                text_placement: TextPlacement::AboveLine, // NBR 17068: texto acima da linha de cota
                witness_overshoot_mm: 3.0,                // NBR 17068: prolongamento de 2 mm a 3 mm
                witness_gap_mm: 2.0,                      // NBR 17068: afastamento de 1 mm a 2 mm
                text_height_mm: 2.5,                      // NBR 16861: altura nominal de texto 2.5 mm
            },
            TechnicalStandard::Iso | TechnicalStandard::Eu => DimensionStyle {
                terminator: Terminator::Tick45,
                text_placement: TextPlacement::AboveLine,
                witness_overshoot_mm: 3.0,
                witness_gap_mm: 2.0,
                text_height_mm: 2.5,
            },
            TechnicalStandard::Ansi => DimensionStyle {
                terminator: Terminator::Tick45,
                text_placement: TextPlacement::InlineBreak, // ASME Y14.5: centered in line break
                witness_overshoot_mm: 3.2,                  // ~1/8 in
                witness_gap_mm: 1.6,                        // ~1/16 in
                text_height_mm: 2.4,                        // ~3/32 in
            },
            TechnicalStandard::Jis => DimensionStyle {
                terminator: Terminator::FilledArrow,
                text_placement: TextPlacement::AboveLine,
                witness_overshoot_mm: 2.5,
                witness_gap_mm: 2.0,
                text_height_mm: 2.5,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn technical_standards_have_codes_and_can_be_parsed() {
        for std in TechnicalStandard::ALL {
            assert_eq!(TechnicalStandard::from_code(std.code()), Some(std));
            assert!(!std.name().is_empty());
            assert!(!std.reference_standards().is_empty());
            assert!(!std.description().is_empty());
        }
        assert_eq!(TechnicalStandard::from_code("abnt"), Some(TechnicalStandard::Nbr));
        assert_eq!(TechnicalStandard::from_code("brasil"), Some(TechnicalStandard::Nbr));
        assert_eq!(TechnicalStandard::from_code("usa"), Some(TechnicalStandard::Ansi));
    }

    #[test]
    fn nbr_sheet_margins_follow_nbr_16752_and_nbr_17067() {
        let a0 = TechnicalStandard::Nbr.sheet_margins("A0");
        assert_eq!(a0.left, 25.0, "NBR 16752 requires 25mm binding margin on all formats");
        assert_eq!(a0.top, 10.0);
        assert_eq!(a0.title_width, 178.0, "NBR 16752 title block max width 178mm");
        assert!(a0.folding_marks, "NBR 17067 folding marks for A0");

        let a3 = TechnicalStandard::Nbr.sheet_margins("A3");
        assert_eq!(a3.left, 25.0);
        assert_eq!(a3.top, 7.0, "NBR 16752 requires 7mm other margins for A2-A4");
        assert_eq!(a3.right, 7.0);
        assert_eq!(a3.bottom, 7.0);
        assert!(a3.folding_marks);

        let a4 = TechnicalStandard::Nbr.sheet_margins("A4");
        assert_eq!(a4.left, 25.0);
        assert_eq!(a4.top, 7.0);
        assert!(!a4.folding_marks, "A4 is the folded target size, no internal folds");
    }

    #[test]
    fn nbr_object_styles_reflect_nbr_6492() {
        let styles = TechnicalStandard::Nbr.object_styles();
        assert_eq!(styles["wall"].cut, Pen(10), "Walls cut are heavy (0.50mm) in NBR 6492");
        assert_eq!(styles["column"].cut, Pen(12), "Structural columns cut are heavier (0.70mm) in NBR 6492");
        assert_eq!(styles["door"].cut, Pen(7), "Doors cut are medium (0.35mm)");
    }
}
