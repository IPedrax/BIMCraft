use super::{Path, R, Session, Value, bad, json, only, string};

pub(super) fn catalogue_export(s: &mut Session, p: &Value) -> R {
    const C: &str = "catalogue_export";
    only(C, p, &["path"])?;
    if let Some(how) = s.read_only {
        return Err(bad(C, format!("cannot write a catalogue from a read-only session: {how}")));
    }
    let path = string(C, p, "path")?.ok_or_else(|| bad(C, "path is required"))?;
    let path = crate::ods::workbook_path(Path::new(&path)).map_err(|e| bad(C, e))?;
    crate::ods::workbook_exists(&path).map_err(|e| bad(C, e))?;
    if s.path.as_ref().is_some_and(|project| crate::engine::same_file(project, &path)) {
        return Err(bad(C, "path must differ from the project file"));
    }
    let bytes = crate::ods::write(&crate::ods::catalogue_rows(&s.model)?).map_err(|e| bad(C, e))?;
    crate::autosave::atomic_write_bytes(&path, &bytes).map_err(|e| bad(C, e))?;
    Ok(json!({"path":path}))
}

pub(super) fn catalogue_import(s: &mut Session, p: &Value) -> R {
    const C: &str = "catalogue_import";
    only(C, p, &["path"])?;
    if let Some(how) = s.read_only {
        return Err(bad(C, format!("cannot import a catalogue into a read-only project: {how}")));
    }
    let path = string(C, p, "path")?.ok_or_else(|| bad(C, "path is required"))?;
    let path = crate::ods::workbook_path(Path::new(&path)).map_err(|e| bad(C, e))?;
    let bytes = crate::ods::read_workbook(&path).map_err(|e| bad(C, e))?;
    let rows = crate::ods::read(&bytes).map_err(|e| bad(C, e))?;
    let count = crate::ods::apply_catalogue(s.model_mut(), &rows).map_err(|e| bad(C, e))?;
    Ok(json!({"rows":count}))
}
