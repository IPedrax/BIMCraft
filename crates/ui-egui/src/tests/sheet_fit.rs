//! The existing Place View control fits an unscaled 3D view to paper.
use super::*;

#[test]
fn place_view_fits_a_large_3d_view_above_the_sheet_title_block() {
    let mut h = app_at(r#"wall {"points": [[0,0],[60000,0],[60000,40000],[0,40000]], "closed": true}"#, 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    let page = ids(&h, "sheet")[0];
    assert_eq!(h.state().sheet, Sheet::Paper(page));
    press(&mut h, "View");
    press(&mut h, "Place View");
    press(&mut h, "3D View: {3D}");
    let vp = ids(&h, "viewport");
    assert_eq!(vp.len(), 1);
    assert_eq!(field(&h, vp[0], "scale"), 500.0);
    let (frame, (x, y)) = bimcraft_core::sheet::frame(&h.state().session.model, vp[0]).unwrap();
    assert!(x >= 125.0 && x + frame.width as f32 <= 2025.0, "the picture stays within the left and right borders");
    assert!(y >= 75.0 && y + frame.height as f32 + 75.0 <= 1210.0, "the picture and its title stay above the title block");
    assert_eq!(h.state().sheet, Sheet::Paper(page));
    // The placed view disappears from the choices, completing the same journey as other views.
    press(&mut h, "View");
    press(&mut h, "Place View");
    assert!(h.query_by_label("3D View: {3D}").is_none());
}
