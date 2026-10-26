//! Fill patterns read from and written to PAT files, the text format AutoCAD made public and Revit's Fill Patterns
//! dialog imports and exports. A file holds patterns one after another: a `*Name, description` header, then a line for
//! each family of hatch lines, `angle, x, y, shift, offset[, dash, …]` (the offset across, the shift along, a space as a
//! negative dash, 0 a dot). Comments start with `;`, and two of them carry settings: `;%UNITS=MM` or `INCH` (millimetres
//! when the file doesn't say) and, after a header, `;%TYPE=MODEL` or `DRAFTING` (drafting when it doesn't).
//!
//! A file is read kindly: a pattern that can't be one (no lines, a family with its lines on each other, more families
//! or dashes than a pattern takes) is left out and named with why, and the rest come in.

use crate::fills::{FillPattern, HatchLine, MAX_DASHES, MAX_FAMILIES, Target};
use crate::geom::P2;

/// The most patterns a file brings in, and the longest file read, in bytes.
pub const MAX_PATTERNS: usize = 2000;
pub const MAX_TEXT: usize = 16 << 20;

/// What a file held: its patterns by name, in the order they came, each with its description; and those left out, each
/// with why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    pub patterns: Vec<(String, FillPattern)>,
    pub left_out: Vec<(String, String)>,
}

/// The patterns in `text`, a PAT file's contents.
pub fn read(text: &str) -> Result<Read, String> {
    if text.len() > MAX_TEXT {
        return Err(format!("the file is {} bytes, more than {MAX_TEXT}", text.len()));
    }
    let mut scale = 1.0;
    let mut out = Read::default();
    // The pattern being read: its name, its type, its families, and what's wrong with it.
    let mut current: Option<(String, FillPattern, Option<String>)> = None;
    let finish = |cur: Option<(String, FillPattern, Option<String>)>, out: &mut Read| {
        let Some((name, pattern, wrong)) = cur else { return };
        let why = wrong.or_else(|| pattern.lines.is_empty().then(|| "it has no lines".to_string()));
        let why = why.or_else(|| pattern.check(&name).err());
        match why {
            Some(why) => out.left_out.push((name, why)),
            None if out.patterns.iter().any(|(n, _)| *n == name) => out.left_out.push((name, "the file has it twice".into())),
            None if out.patterns.len() >= MAX_PATTERNS => out.left_out.push((name, format!("more than {MAX_PATTERNS} patterns"))),
            None => out.patterns.push((name, pattern)),
        }
    };
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if let Some(setting) = line.strip_prefix(";%") {
            let (key, value) = setting.split_once('=').unwrap_or((setting, ""));
            match (key.trim().to_ascii_uppercase().as_str(), value.trim().to_ascii_uppercase().as_str()) {
                ("UNITS", "INCH" | "INCHES") => scale = 25.4,
                ("UNITS", _) => scale = 1.0,
                ("TYPE", "MODEL") => {
                    if let Some((_, p, _)) = &mut current {
                        p.target = Target::Model;
                    }
                }
                ("TYPE", _) => {
                    if let Some((_, p, _)) = &mut current {
                        p.target = Target::Drafting;
                    }
                }
                _ => {}
            }
            continue;
        }
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('*') {
            finish(current.take(), &mut out);
            let name = header.split(',').next().unwrap_or_default().trim();
            let name: String = name.chars().filter(|c| !c.is_control()).take(crate::model::MAX_TEXT).collect();
            current = Some((if name.is_empty() { "Unnamed".into() } else { name }, FillPattern::default(), None));
            continue;
        }
        // A family's line: numbers, a comment after them dropped.
        let Some((_, pattern, wrong)) = &mut current else { continue };
        if wrong.is_some() {
            continue;
        }
        let body = line.split(';').next().unwrap_or_default();
        let numbers: Result<Vec<f64>, _> = body.split(',').map(|x| x.trim().parse::<f64>()).collect();
        match numbers {
            Ok(n) if n.len() >= 5 && n.iter().all(|x| x.is_finite()) => {
                if pattern.lines.len() >= MAX_FAMILIES {
                    *wrong = Some(format!("more than {MAX_FAMILIES} line families"));
                    continue;
                }
                if n.len() - 5 > MAX_DASHES {
                    *wrong = Some(format!("a family has more than {MAX_DASHES} dashes"));
                    continue;
                }
                // The next line along the family's own way: `shift` along, `offset` across. Across the other way, the
                // same lines come stepping back.
                let (shift, offset) = if n[4] < 0.0 { (-n[3], -n[4]) } else { (n[3], n[4]) };
                if offset == 0.0 {
                    *wrong = Some("a family's lines lie on each other: its offset is 0".into());
                    continue;
                }
                pattern.lines.push(HatchLine {
                    angle: n[0].rem_euclid(360.0),
                    origin: P2(n[1] * scale, n[2] * scale),
                    shift: shift * scale,
                    spacing: offset * scale,
                    dashes: n[5..].iter().map(|d| d * scale).collect(),
                });
            }
            _ => *wrong = Some(format!("`{}` isn't a line of numbers: angle, x, y, shift, offset and dashes", body.trim())),
        }
    }
    finish(current.take(), &mut out);
    Ok(out)
}

/// The patterns in the PAT file at `path`, no larger than [`MAX_TEXT`]. Old PAT files are often Windows-1252: a byte that
/// isn't UTF-8 is read as the letter it is there.
pub fn read_file(path: &std::path::Path) -> Result<Read, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_TEXT as u64 {
        return Err(format!("{} is too large ({size} bytes)", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| char::from(b)).collect());
    read(&text)
}

/// `patterns` as a PAT file in millimetres, solid ones left out (a PAT file has no way to say solid).
pub fn write<'a>(patterns: impl IntoIterator<Item = (&'a String, &'a FillPattern)>) -> String {
    let mut out = String::from(";%UNITS=MM\n;%VERSION=3.0\n;Fill patterns from BIMCraft\n");
    let num = |x: f64| {
        let r = (x * 1e6).round() / 1e6;
        format!("{}", r + 0.0)
    };
    for (name, p) in patterns {
        if p.is_solid() {
            continue;
        }
        // A name is one line, and a comma would end it.
        let name: String = name.chars().map(|c| if c == ',' || c.is_control() { ' ' } else { c }).collect();
        out.push_str(&format!("*{name}\n"));
        out.push_str(match p.target {
            Target::Model => ";%TYPE=MODEL\n",
            Target::Drafting => ";%TYPE=DRAFTING\n",
        });
        for l in &p.lines {
            let mut fields = vec![num(l.angle), num(l.origin.0), num(l.origin.1), num(l.shift), num(l.spacing)];
            fields.extend(l.dashes.iter().map(|d| num(*d)));
            out.push_str(&fields.join(","));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRICK: &str = ";%UNITS=MM
;%VERSION=3.0
*Brick 230x75, running bond
;%TYPE=MODEL
0, 0,0, 0,75
90, 0,0, 75,115, 75,-75
;a comment
*Diagonal, 45 degrees
45, 0,0, 0,3
";

    #[test]
    fn a_pat_file_reads_as_patterns_and_writes_back_the_same() {
        let r = read(BRICK).unwrap_or_default();
        assert!(r.left_out.is_empty(), "{:?}", r.left_out);
        let names: Vec<&str> = r.patterns.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Brick 230x75", "Diagonal"]);
        let brick = &r.patterns[0].1;
        assert_eq!(brick.target, Target::Model);
        assert_eq!(brick.lines[1], HatchLine { angle: 90.0, origin: P2(0.0, 0.0), shift: 75.0, spacing: 115.0, dashes: vec![75.0, -75.0] });
        assert_eq!(r.patterns[1].1.target, Target::Drafting, "drafting when it doesn't say");
        let again = read(&write(r.patterns.iter().map(|(n, p)| (n, p)))).unwrap_or_default();
        assert_eq!(again, r);
    }

    #[test]
    fn inches_are_millimetres_a_negative_offset_steps_the_other_way_and_what_cant_be_is_left_out() {
        let text = ";%UNITS=INCH
*Lines
0, 0,0, 1,-2, 0.5,-0.25
*Flat
0, 0,0, 0,0
*Words
0, zero, 0, 0, 1
*Empty
*Lines
0, 0,0, 0,1
";
        let r = read(text).unwrap_or_default();
        assert_eq!(r.patterns.len(), 1);
        let l = &r.patterns[0].1.lines[0];
        assert_eq!((l.shift, l.spacing), (-25.4, 50.8));
        assert_eq!(l.dashes, [12.7, -6.35]);
        let out: Vec<&str> = r.left_out.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(out, ["Flat", "Words", "Empty", "Lines"]);
        assert!(r.left_out[3].1.contains("twice"));
        assert!(read(&"x".repeat(MAX_TEXT + 1)).is_err());
    }
}
