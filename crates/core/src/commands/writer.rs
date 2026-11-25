use super::{BTreeMap, Path, R, Session, Value, bad, json, only, string};

pub(super) fn odt_template(_s: &mut Session, p: &Value) -> R {
    let c = "odt_template";
    if !p.is_object() {
        return Err(bad(c, "parameters must be an object"));
    }
    only(c, p, &["template", "path", "fields"])?;
    let template = string(c, p, "template")?.ok_or_else(|| bad(c, "template is required"))?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "path is required"))?;
    let fields = odt_fields(c, p)?;
    let template = crate::odt::template_path(Path::new(&template)).map_err(|e| bad(c, e))?;
    let path = crate::odt::template_path(Path::new(&path)).map_err(|e| bad(c, e))?;
    if crate::odt::same_file(&template, &path) {
        return Err(bad(c, "path must differ from the template"));
    }
    let input = crate::odt::read_template(&template).map_err(|e| bad(c, e))?;
    let output = crate::odt::fill(&input, &fields).map_err(|e| bad(c, e))?;
    crate::autosave::atomic_write_bytes(&path, &output).map_err(|e| bad(c, e))?;
    Ok(json!({"template":template,"path":path,"fields":fields.len()}))
}

fn odt_fields(c: &str, p: &Value) -> Result<BTreeMap<String, String>, String> {
    let values = p.get("fields").and_then(Value::as_object).ok_or_else(|| bad(c, "fields must be an object of text values"))?;
    let mut fields = BTreeMap::new();
    for (name, value) in values {
        let value = value.as_str().ok_or_else(|| bad(c, format!("field `{name}` must be text")))?;
        fields.insert(name.clone(), value.to_owned());
    }
    Ok(fields)
}

pub(super) fn odt_set(_: &mut Session, p: &Value) -> R {
    const C: &str = "odt_set";
    only(C, p, &["templates", "path", "fields"])?;
    let list = p["templates"]
        .as_array()
        .filter(|a| (1..=crate::odt::MAX_SET_FILES).contains(&a.len()))
        .ok_or_else(|| bad(C, format!("give 1 to {} template paths", crate::odt::MAX_SET_FILES)))?;
    let dir = string(C, p, "path")?.ok_or_else(|| bad(C, "path is required"))?;
    let dir = std::path::absolute(Path::new(&dir)).map_err(|e| bad(C, e.to_string()))?;
    let parent = dir.parent().ok_or_else(|| bad(C, "path must name a new directory"))?;
    let refuse_existing = || match std::fs::symlink_metadata(&dir) {
        Ok(_) => Err(bad(C, "output directory already exists")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(bad(C, e.to_string())),
    };
    refuse_existing()?;
    let fields = odt_fields(C, p)?;
    let mut sources = Vec::new();
    let mut input_bytes = 0;
    for source in list {
        let source = source.as_str().ok_or_else(|| bad(C, "each template must be a path string"))?;
        let source = crate::odt::document_path(Path::new(source)).map_err(|e| bad(C, e))?;
        let bytes = crate::odt::read_template(&source).map_err(|e| bad(C, e))?;
        input_bytes += bytes.len() as u64;
        if input_bytes > crate::odt::MAX_SET_BYTES {
            return Err(bad(C, "ODT set input exceeds 256 MiB"));
        }
        sources.push((source, bytes));
    }
    let output = crate::odt::fill_set(&sources, &fields).map_err(|e| bad(C, e))?;
    let staging = tempfile::tempdir_in(parent).map_err(|e| bad(C, e.to_string()))?;
    let mut written = Vec::new();
    for (name, bytes) in &output {
        crate::autosave::atomic_write_bytes(&staging.path().join(name), bytes).map_err(|e| bad(C, e))?;
        written.push(dir.join(name));
    }
    refuse_existing()?;
    crate::odt::publish_set(staging.path(), &dir).map_err(|e| bad(C, e.to_string()))?;
    Ok(json!({"path":dir,"files":written,"fields":fields.len()}))
}
