//! Generated closed equipment for project-owned industrial door types. Authored families keep their own geometry.
//! Pieces are the existing family representation, shared by plans, sections, 3D and IFC; Operation remains in the model.
use crate::family::{Piece, Plane, sweep};
use crate::geom::P2;
use crate::model::Operation;

pub(crate) fn pieces(operation: Operation, width: f64, height: f64, face: f64, material: &str) -> Result<Vec<Piece>, String> {
    let half = width / 2.0;
    let mut boxes = Vec::new();
    match operation {
        Operation::Sectional => {
            // Roughly 600 mm panels; bound the derived mesh even for the largest permitted type.
            let n = (height / 600.0).round().clamp(1.0, 256.0) as usize;
            let each = height / n as f64;
            let gap = (each * 0.02).min(5.0);
            for i in 0..n {
                boxes.push(((-half - 40.0, half + 40.0), (face + 20.0, face + 65.0), (i as f64 * each + gap, (i + 1) as f64 * each - gap)));
            }
            for along in [(-half - 100.0, -half - 40.0), (half + 40.0, half + 100.0)] {
                boxes.push((along, (face, face + 80.0), (0.0, height + 100.0)));
                boxes.push((along, (face + 80.0, face + 80.0 + height), (height + 50.0, height + 100.0)));
            }
        }
        Operation::Roller => {
            boxes.push(((-half - 30.0, half + 30.0), (face + 30.0, face + 45.0), (0.0, height)));
            for along in [(-half - 100.0, -half - 30.0), (half + 30.0, half + 100.0)] {
                boxes.push((along, (face, face + 80.0), (0.0, height)));
            }
            boxes.push(((-half - 100.0, half + 100.0), (face, face + 450.0), (height, height + 450.0)));
        }
        _ => {}
    }
    boxes
        .into_iter()
        .map(|((a, b), (c, d), (bottom, top))| {
            let outline = vec![P2(a, c), P2(b, c), P2(b, d), P2(a, d)];
            let (tris, caps) = sweep(&outline, bottom, top, Plane::Xy)?;
            Ok(Piece { tris, caps, outline, bottom, top, material: material.into(), round: false })
        })
        .collect()
}
