//! Local envelopes of supported forms, without triangulation or boolean construction.
use super::*;
use crate::bounds::Bounds;

fn include(out: &mut Option<[P3; 2]>, p: P3) -> Option<()> {
    if !p.iter().all(|v| v.is_finite()) {
        return None;
    }
    *out = Some(match *out {
        None => [p, p],
        Some([lo, hi]) => [std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i]))],
    });
    Some(())
}

impl Family {
    /// Conservative local bounds for visible swept profiles and imported meshes.
    /// No triangles or boolean results are constructed. None means unknown/empty;
    /// callers must keep the instance. Visible voids and revolved forms currently
    /// return unknown, and so does an adaptive family, whose geometry its instances' points decide. These envelopes must
    /// not replace exact framing extrema.
    pub fn conservative_bounds<'a>(&'a self, values: &BTreeMap<String, Val>) -> Option<[P3; 2]> {
        if self.adaptive.is_some() {
            return None;
        }
        let mut read = HashMap::new();
        let mut num = |e: &'a Expr| number(e, values, &mut read).ok();
        let mut out = None;
        for form in &self.forms {
            if let Some(visible) = &form.visible
                && num(visible)? == 0.0
            {
                continue;
            }
            if form.void {
                return None;
            }
            if !form.mesh.is_empty() {
                if form.mesh.len() > MAX_BUILT {
                    return None;
                }
                for p in form.mesh.iter().flatten() {
                    include(&mut out, *p)?;
                }
                continue;
            }
            if form.axis.is_some() {
                return None;
            }
            let profile: Vec<P2> = match &form.circle {
                Some(circle) => {
                    let (x, y, r) = (num(&circle.centre[0])?, num(&circle.centre[1])?, num(&circle.radius)?);
                    if r <= 0.0 {
                        return None;
                    }
                    // Enclose every circular profile angle, including every facet.
                    vec![P2(x - r, y - r), P2(x + r, y - r), P2(x + r, y + r), P2(x - r, y + r)]
                }
                None => {
                    if form.profile.len() < 3 || form.profile.len() > MAX_PROFILE {
                        return None;
                    }
                    form.profile.iter().map(|[x, y]| Some(P2(num(x)?, num(y)?))).collect::<Option<_>>()?
                }
            };
            let (from, to) = (num(&form.from)?, num(&form.to)?);
            if to <= from {
                return None;
            }
            let frame = self.frame(&form.host).ok()?;
            for p in profile {
                include(&mut out, frame.point(p, from))?;
                include(&mut out, frame.point(p, to))?;
            }
        }
        match crate::bounds::padded(out?.into_iter()) {
            Bounds::Box(bounds) => Some(bounds),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encloses(f: &Family, values: &BTreeMap<String, Val>) {
        let [lo, hi] = f.conservative_bounds(values).unwrap();
        let pieces = f.pieces(values).unwrap();
        assert!(!pieces.is_empty());
        for p in pieces.iter().flat_map(|p| p.tris.iter().flatten()) {
            assert!((0..3).all(|i| lo[i] <= p[i] && p[i] <= hi[i]), "{p:?} outside {lo:?}..{hi:?}");
        }
    }

    #[test]
    fn swept_and_imported_family_envelopes_enclose_full_geometry() {
        let n = Expr::Number;
        let ring = vec![[n(-100.0), n(-50.0)], [n(700.0), n(-30.0)], [n(800.0), n(400.0)], [n(-80.0), n(300.0)]];
        for plane in [Plane::Xy, Plane::Xz, Plane::Yz] {
            for circle in [None, Some(Circle { centre: [n(-20.0), n(17.0)], radius: n(80.0) })] {
                let f = Family {
                    forms: vec![Form {
                        host: plane.host(),
                        profile: ring.clone(),
                        circle,
                        from: n(-25.0),
                        to: n(900.0),
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                encloses(&f, &BTreeMap::new());
            }
        }
        let source: Family = serde_json::from_value(serde_json::json!({"forms":[{
            "profile":[[0,0],[500,0],[500,300],[0,300]],"from":-20,"to":100}]}))
        .unwrap();
        let mesh = source.pieces(&BTreeMap::new()).unwrap().into_iter().flat_map(|p| p.tris).collect();
        encloses(&Family { forms: vec![Form { mesh: Arc::new(mesh), ..Default::default() }], ..Default::default() }, &BTreeMap::new());
    }

    #[test]
    fn default_family_envelopes_cover_their_supported_types() {
        let mut supported = 0;
        for family in library(crate::language::Language::English).values() {
            for name in family.types.keys() {
                let values = family.values(name, &BTreeMap::new()).unwrap();
                if family.conservative_bounds(&values).is_some() {
                    encloses(family, &values);
                    supported += 1;
                }
            }
        }
        assert!(supported >= 20, "the library must exercise supported geometry, not only unknown bounds");
    }

    #[test]
    fn unsupported_and_nonfinite_forms_keep_unknown_bounds() {
        let f: Family = serde_json::from_value(serde_json::json!({"forms":[{
            "profile":[[0,0],[400,0],[400,300],[0,300]],"to":900}]}))
        .unwrap();
        let mut unsupported = f.clone();
        unsupported.forms[0].void = true;
        assert!(unsupported.conservative_bounds(&BTreeMap::new()).is_none());
        unsupported.forms[0].visible = Some(Expr::Number(0.0));
        unsupported.forms.extend(f.forms.clone());
        encloses(&unsupported, &BTreeMap::new());
        unsupported = f.clone();
        unsupported.forms[0].axis = Some([[Expr::Number(0.0), Expr::Number(0.0)], [Expr::Number(0.0), Expr::Number(1.0)]]);
        assert!(unsupported.conservative_bounds(&BTreeMap::new()).is_none());
        unsupported = f;
        unsupported.forms[0].to = Expr::Number(f64::NAN);
        assert!(unsupported.conservative_bounds(&BTreeMap::new()).is_none());
    }
}
