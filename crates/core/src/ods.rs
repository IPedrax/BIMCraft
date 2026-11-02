//! Bounded ODS tables used by the project's editable equipment catalogue.

use std::io::{Cursor, Read, Write};

use quick_xml::{Reader, events::Event};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const MAX_ROWS: usize = 20_000;
pub const MAX_CELL: usize = 16_384;
const MAX_COLUMNS: usize = 64;
const MAX_XML: u64 = 32 << 20;
const MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";

/// Geometry of a removed catalogue type remains in the model, but is not a billable product.
pub fn placeholder_element(model: &crate::model::Model, element: &crate::model::Element) -> bool {
    if let Some((family, ty)) = element.family() {
        return model.library.placeholder(&crate::library::Kind::FamilyType, &format!("{family}: {ty}"));
    }
    element.run().is_some_and(|run| model.library.placeholder(&crate::library::Kind::Type(element.category().into()), &run.type_name))
}

/// Imported catalogue types with no model instances. Built-in unused types are not proposed as project equipment.
pub fn pending_products(model: &crate::model::Model) -> Vec<(crate::ids::Id, &crate::library::Entry)> {
    use crate::library::Kind;
    let mut used = std::collections::BTreeSet::new();
    for element in model.elements.values() {
        if let Some((family, ty)) = element.family() {
            used.insert((Kind::FamilyType, format!("{family}: {ty}")));
        }
        if let Some(run) = element.run() {
            used.insert((Kind::Type(element.category().into()), run.type_name.clone()));
        }
    }
    model
        .library
        .entries()
        .filter(|(_, entry)| entry.catalogue_added && !entry.placeholder && !used.contains(&(entry.kind.clone(), entry.name.clone())))
        .collect()
}

/// Synchronize a bound catalogue transactionally. While the model is open, only new rows may enter it.
pub fn sync_bound(
    model: &mut crate::model::Model,
    protect_existing: bool,
    owned: &std::collections::BTreeSet<crate::ids::Guid>,
) -> Result<serde_json::Value, String> {
    let path = model.catalogue.as_ref().ok_or("No ODS catalogue is linked")?.clone();
    let mut bytes = Vec::new();
    std::fs::File::open(&path).map_err(|e| e.to_string())?.take(MAX_XML + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let mut rows: Vec<_> = read(&bytes)?
        .into_iter()
        .filter(|row| !row.first().and_then(|g| crate::ids::Guid::parse(g)).is_some_and(|guid| model.catalogue_retired.contains(&guid)))
        .collect();
    if !protect_existing && !model.catalogue_pending_edits.is_empty() {
        let exported: std::collections::BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(index, row)| row.first().and_then(|guid| crate::ids::Guid::parse(guid)).map(|guid| (guid, index)))
            .collect();
        for current in catalogue_rows(model).into_iter().skip(1) {
            let Some(guid) = crate::ids::Guid::parse(&current[0]) else { continue };
            let Some(previous) = model.catalogue_pending_edits.get(&guid) else { continue };
            if current == *previous {
                continue;
            }
            let index = exported.get(&guid).copied().ok_or_else(|| {
                format!("Locally edited catalogue product {guid} conflicts with an externally deleted row; synchronize to keep the model version")
            })?;
            let equal = |left: &[String], right: &[String]| {
                (0..left.len().max(right.len())).all(|column| {
                    left.get(column).map(String::as_str).unwrap_or_default() == right.get(column).map(String::as_str).unwrap_or_default()
                })
            };
            if !equal(&rows[index], previous) && !equal(&rows[index], &current) {
                return Err(format!(
                    "Locally edited catalogue product {guid} conflicts with an ODS row; synchronize to keep the model version"
                ));
            }
            rows[index] = current;
        }
    }
    if !protect_existing && !model.catalogue_unexported.is_empty() {
        let exported: std::collections::BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(index, row)| row.first().and_then(|guid| crate::ids::Guid::parse(guid)).map(|guid| (guid, index)))
            .collect();
        for row in catalogue_rows(model).into_iter().skip(1) {
            let Some(guid) = crate::ids::Guid::parse(&row[0]).filter(|guid| model.catalogue_unexported.contains(guid)) else {
                continue;
            };
            if let Some(&index) = exported.get(&guid) {
                if (0..rows[index].len().max(row.len())).any(|column| {
                    rows[index].get(column).map(String::as_str).unwrap_or_default()
                        != row.get(column).map(String::as_str).unwrap_or_default()
                }) {
                    return Err(format!(
                        "Unexported catalogue product {} conflicts with an ODS row; resolve the product before synchronizing",
                        row[0]
                    ));
                }
            } else {
                rows.push(row);
            }
        }
    }
    let mut candidate = model.clone();
    let updated = if protect_existing {
        // Validate the schema even when there are no new rows, without applying edited existing rows.
        let header = rows.first().ok_or("ODS catalogue has no header")?.clone();
        apply_catalogue(&mut candidate, std::slice::from_ref(&header))?;
        let existing = catalogue_rows(model);
        let guids: std::collections::BTreeSet<_> = model.library.entries().map(|(_, e)| e.guid.to_string()).collect();
        let names: std::collections::BTreeSet<_> = existing.iter().skip(1).map(|r| (r[1].as_str(), r[2].as_str())).collect();
        let mut new_rows = vec![header];
        for row in rows.iter().skip(1) {
            if row.iter().all(String::is_empty) {
                continue;
            }
            let known = row.first().is_some_and(|g| !g.is_empty() && guids.contains(g.as_str()))
                || row.get(1).zip(row.get(2)).is_some_and(|(category, name)| names.contains(&(category.as_str(), name.as_str())));
            // A row exported before Undo belongs to this document; do not resurrect it as a foreign product.
            let retired = row.first().and_then(|g| crate::ids::Guid::parse(g)).is_some_and(|guid| owned.contains(&guid));
            if !known && !retired {
                new_rows.push(row.clone());
            }
        }
        let new_rows = transfer_rows(&candidate, &new_rows, None)?;
        apply_catalogue(&mut candidate, &new_rows)?
    } else {
        let rows = transfer_rows(&candidate, &rows, None)?;
        let count = apply_catalogue(&mut candidate, &rows)?;
        let retained: std::collections::BTreeSet<_> = rows.iter().skip(1).filter_map(|r| r.first()).map(String::as_str).collect();
        for row in catalogue_rows(model).iter().skip(1) {
            if !retained.contains(row[0].as_str()) {
                let kind =
                    if row[1] == "family-type" { crate::library::Kind::FamilyType } else { crate::library::Kind::Type(row[1].clone()) };
                candidate.library.set_placeholder(&kind, &row[2], true)?;
            }
        }
        count
    };
    candidate.catalogue_retired.clear();
    candidate.catalogue_unexported.clear();
    candidate.catalogue_pending_edits.clear();
    candidate.check()?;
    let output = replace_first_sheet(&bytes, &catalogue_rows(&candidate))?;
    let backup = if output != bytes {
        let parent = std::path::Path::new(&path).parent().ok_or("Catalogue path needs a parent directory")?;
        let mut backup =
            tempfile::Builder::new().prefix("bimcraft-ods-backup-").suffix(".ods").tempfile_in(parent).map_err(|e| e.to_string())?;
        backup.write_all(&bytes).map_err(|e| e.to_string())?;
        backup.as_file().sync_all().map_err(|e| e.to_string())?;
        let (_, backup) = backup.keep().map_err(|e| e.to_string())?;
        crate::autosave::atomic_write_bytes(std::path::Path::new(&path), &output)?;
        Some(backup)
    } else {
        None
    };
    let placeholders: Vec<_> = candidate.library.entries().filter(|(_, e)| e.placeholder).map(|(_, e)| e.guid).collect();
    *model = candidate;
    Ok(serde_json::json!({"path":path,"updated":updated,"backup":backup,"placeholders":placeholders}))
}

// Keep sheet names, other sheets, styles and embedded assets. Only the database rows belong to BIMCraft.
fn replace_first_sheet(bytes: &[u8], rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut content = String::new();
    archive.by_name("content.xml").map_err(|e| e.to_string())?.take(MAX_XML + 1).read_to_string(&mut content).map_err(|e| e.to_string())?;
    let mut reader = Reader::from_str(&content);
    let mut inner_start = None;
    let mut inner_end = None;
    let mut depth = 0usize;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                if inner_start.is_some() {
                    depth += 1;
                } else if e.local_name().as_ref() == b"table" {
                    inner_start = Some(reader.buffer_position() as usize);
                    depth = 1;
                }
            }
            Event::End(_) if inner_start.is_some() => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    inner_end = Some(before);
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let (start, end) = inner_start.zip(inner_end).ok_or("ODS first sheet is missing")?;
    let generated = write(rows)?;
    let mut generated_archive = ZipArchive::new(Cursor::new(generated)).map_err(|e| e.to_string())?;
    let mut generated_xml = String::new();
    generated_archive.by_name("content.xml").map_err(|e| e.to_string())?.read_to_string(&mut generated_xml).map_err(|e| e.to_string())?;
    let table = generated_xml.find("<table:table table:name=").ok_or("ODS generated sheet is missing")?;
    let first = table + generated_xml[table..].find('>').ok_or("ODS generated sheet is malformed")? + 1;
    let last = generated_xml.rfind("</table:table>").ok_or("ODS generated sheet is malformed")?;
    let body = generated_xml[first..last].replace("<table:table-row>", "<table:table-row xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">");
    content.replace_range(start..end, &body);
    if content.len() as u64 > MAX_XML {
        return Err("ODS content is too large".into());
    }
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| e.to_string())?;
        if file.name().map_err(|e| e.to_string())? == "content.xml" {
            output
                .start_file("content.xml", SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))
                .map_err(|e| e.to_string())?;
            output.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            output.raw_copy_file(file).map_err(|e| e.to_string())?;
        }
    }
    let bytes = output.finish().map_err(|e| e.to_string())?.into_inner();
    if bytes.len() as u64 > MAX_XML {
        return Err("ODS file is too large".into());
    }
    Ok(bytes)
}

fn escaped(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

const HEADINGS: [&str; 13] = [
    "GUID",
    "Category",
    "Name",
    "Width_mm",
    "Depth_mm",
    "Round",
    "Material",
    "Description",
    "Brand",
    "Article",
    "Manufacturer",
    "Mass_kg",
    "Height_mm",
];

fn dimension_parameter<'a>(
    family: &'a crate::family::Family,
    language: crate::language::Language,
    name: &str,
) -> Option<&'a crate::family::Parameter> {
    family.parameters.iter().find(|p| {
        !p.instance && p.formula.is_none() && p.kind == crate::family::Kind::Length && (p.name == name || p.name == language.word(name))
    })
}

pub fn catalogue_rows(model: &crate::model::Model) -> Vec<Vec<String>> {
    let mut rows = vec![HEADINGS.iter().map(|s| s.to_string()).collect()];
    for category in ["wire", "cable", "cablecarrier"] {
        if let Some(types) = model.types.runs.get(category) {
            for (name, ty) in types {
                if model.library.placeholder(&crate::library::Kind::Type(category.into()), name) {
                    continue;
                }
                let guid = model.library.guid(&crate::library::Kind::Type(category.into()), name);
                let product =
                    guid.and_then(|g| model.library.by_guid(g)).and_then(|(_, e)| e.product.as_ref()).cloned().unwrap_or_default();
                rows.push(vec![
                    guid.map(|g| g.to_string()).unwrap_or_default(),
                    category.into(),
                    name.clone(),
                    ty.width.to_string(),
                    ty.depth.to_string(),
                    ty.round.to_string(),
                    ty.material.clone(),
                    product.description,
                    product.brand,
                    product.article,
                    product.manufacturer,
                    product.mass_kg.map(|mass| mass.to_string()).unwrap_or_default(),
                    String::new(),
                ]);
            }
        }
    }
    for (family_name, family) in &model.families {
        for type_name in family.types.keys() {
            let name = format!("{family_name}: {type_name}");
            if model.library.placeholder(&crate::library::Kind::FamilyType, &name) {
                continue;
            }
            let guid = model.library.guid(&crate::library::Kind::FamilyType, &name);
            let product = guid.and_then(|g| model.library.by_guid(g)).and_then(|(_, e)| e.product.as_ref()).cloned().unwrap_or_default();
            let values = family.values(type_name, &Default::default()).ok();
            let dimension = |key| {
                dimension_parameter(family, model.language, key)
                    .and_then(|p| values.as_ref()?.get(&p.name))
                    .and_then(|v| match v {
                        crate::family::Val::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default()
            };
            rows.push(vec![
                guid.map(|g| g.to_string()).unwrap_or_default(),
                "family-type".into(),
                name,
                dimension("Width"),
                dimension("Depth"),
                String::new(),
                String::new(),
                product.description,
                product.brand,
                product.article,
                product.manufacturer,
                product.mass_kg.map(|mass| mass.to_string()).unwrap_or_default(),
                dimension("Height"),
            ]);
        }
    }
    rows
}

/// Reconcile copied catalogues without adopting another project's identities.
pub fn transfer_rows(model: &crate::model::Model, rows: &[Vec<String>], conflicts: Option<&str>) -> Result<Vec<Vec<String>>, String> {
    if !matches!(conflicts, None | Some("error" | "update" | "copy")) {
        return Err("ODS conflicts must be error, update or copy".into());
    }
    let current = catalogue_rows(model);
    let identities: std::collections::BTreeMap<_, _> =
        current.iter().skip(1).filter_map(|row| crate::ids::Guid::parse(&row[0]).map(|guid| (guid, row))).collect();
    let products: std::collections::BTreeMap<_, _> = current.iter().skip(1).map(|row| ((row[1].as_str(), row[2].as_str()), row)).collect();
    let mut result = rows.to_vec();
    let mut names: std::collections::BTreeSet<_> =
        current.iter().skip(1).chain(rows.iter().skip(1)).filter(|r| r.len() >= 3).map(|r| (r[1].clone(), r[2].clone())).collect();
    let name_limit = MAX_ROWS + model.library.entries().count() + 2;
    for row in result.iter_mut().skip(1).filter(|r| r.iter().any(|s| !s.is_empty())) {
        if row.len() < 3 || row.len() > HEADINGS.len() {
            return Err("ODS catalogue row has invalid column count".into());
        }
        let guid = if row[0].is_empty() {
            None
        } else {
            Some(serde_json::from_value::<crate::ids::Guid>(serde_json::Value::String(row[0].clone())).map_err(|e| e.to_string())?)
        };
        let by_guid = guid.and_then(|g| identities.get(&g).copied());
        let kind = if row[1] == "family-type" { crate::library::Kind::FamilyType } else { crate::library::Kind::Type(row[1].clone()) };
        if by_guid.is_none()
            && let Some((_, entry)) = guid.and_then(|g| model.library.by_guid(g))
        {
            if entry.placeholder && entry.kind == kind && entry.name == row[2] {
                continue;
            }
            return Err("ODS GUID belongs to a different library entry".into());
        }
        let existing = by_guid.or_else(|| products.get(&(row[1].as_str(), row[2].as_str())).copied());
        let Some(existing) = existing else {
            row[0] = model.library.guid(&kind, &row[2]).map(|g| g.to_string()).unwrap_or_default();
            continue;
        };
        if existing[1] != row[1] {
            return Err("ODS GUID belongs to a different catalogue category".into());
        }
        let identical = row.iter().enumerate().skip(1).all(|(i, cell)| {
            existing.get(i).is_some_and(|old| {
                old == cell
                    || (matches!(i, 3 | 4 | 11 | 12)
                        && old.parse::<f64>().ok().zip(cell.parse::<f64>().ok()).is_some_and(|(a, b)| a.is_finite() && a == b))
            })
        });
        let same_identity = by_guid.is_some() && existing[2] == row[2];
        if identical || conflicts == Some("update") || (same_identity && conflicts.is_none()) {
            row[0].clone_from(&existing[0]);
            row[2].clone_from(&existing[2]);
        } else if conflicts == Some("copy") {
            let base = row[2].clone();
            let mut suffix = 2;
            loop {
                let name = format!("{base} ({suffix})");
                if names.insert((row[1].clone(), name.clone())) {
                    row[2] = name;
                    break;
                }
                suffix += 1;
                if suffix > name_limit {
                    return Err("ODS cannot allocate a separate product name".into());
                }
            }
            row[0].clear();
        } else {
            return Err(format!("ODS product `{}` conflicts with this project; choose conflicts: update or copy", row[2]));
        }
    }
    Ok(result)
}

/// Apply rows to existing library identities; Session validates and rolls back the entire command.
pub fn apply_catalogue(model: &mut crate::model::Model, rows: &[Vec<String>]) -> Result<usize, String> {
    if rows
        .first()
        .is_none_or(|r| ![7, 12, HEADINGS.len()].contains(&r.len()) || r.iter().map(String::as_str).ne(HEADINGS[..r.len()].iter().copied()))
    {
        return Err("ODS catalogue headings do not match the exported schema".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0;
    let mut products = Vec::new();
    let mut applied = Vec::new();
    let has_product_columns = rows.first().is_some_and(|row| row.len() >= 12);
    for source in rows.iter().skip(1).filter(|r| r.iter().any(|s| !s.is_empty())) {
        if source.len() > HEADINGS.len() {
            return Err("ODS catalogue row has too many cells".into());
        }
        let mut row = source.clone();
        row.resize(HEADINGS.len(), String::new());
        if !["wire", "cable", "cablecarrier", "family-type"].contains(&row[1].as_str()) {
            return Err("ODS catalogue row needs a cable, carrier or family-type category".into());
        }
        let (category, name) = (&row[1], &row[2]);
        if !seen.insert((category.clone(), name.clone())) {
            return Err(format!("ODS catalogue repeats `{name}`"));
        }
        let kind = if category == "family-type" { crate::library::Kind::FamilyType } else { crate::library::Kind::Type(category.clone()) };
        if !row[0].is_empty() {
            let guid: crate::ids::Guid = serde_json::from_value(serde_json::Value::String(row[0].clone())).map_err(|e| e.to_string())?;
            let (_, entry) = model.library.by_guid(guid).ok_or("ODS catalogue GUID is not in this project")?;
            if entry.kind != kind || entry.name != *name {
                return Err("ODS catalogue GUID belongs to a different type; renaming requires a model command".into());
            }
        } else if model.library.id(&kind, name).is_some() {
            return Err(format!("ODS new type `{name}` already exists; keep its GUID"));
        }
        let number = |value: &str| -> Result<f64, String> {
            let value: f64 = value.parse().map_err(|_| "ODS dimension must be a number")?;
            if !value.is_finite() || value <= 0.0 || value > crate::model::MAX_COORD {
                return Err("ODS dimension must be finite, positive and within the coordinate limit".into());
            }
            Ok(value)
        };
        if category == "family-type" {
            if !row[5].is_empty() || !row[6].is_empty() {
                return Err("ODS family-type Round and Material cells must be empty".into());
            }
            let (family_name, type_name) = model
                .families
                .keys()
                .filter_map(|family| name.strip_prefix(&format!("{family}: ")).map(|ty| (family.clone(), ty.to_string())))
                .max_by_key(|(family, _)| family.len())
                .ok_or("ODS device family is not in this project")?;
            let family = model.families.get(&family_name).ok_or("ODS device family is missing")?;
            let mut changes = Vec::new();
            for (column, key) in [(3, "Width"), (4, "Depth"), (12, "Height")] {
                if !row[column].is_empty() {
                    let parameter = dimension_parameter(family, model.language, key)
                        .ok_or_else(|| format!("ODS {family_name}: {key} is not an editable type length parameter"))?;
                    changes.push((parameter.name.clone(), serde_json::json!(number(&row[column])?)));
                }
            }
            let family = model.families.get_mut(&family_name).ok_or("ODS device family is missing")?;
            let values = family.types.entry(type_name).or_default();
            for (parameter, value) in changes {
                values.insert(parameter, value);
            }
        } else {
            let round = row[5].parse::<bool>().map_err(|_| "ODS Round must be true or false")?;
            let width = number(&row[3])?;
            let depth = number(&row[4])?;
            if round && width != depth {
                return Err("ODS round type must have equal width and depth".into());
            }
            model
                .types
                .runs
                .entry(category.clone())
                .or_default()
                .insert(name.clone(), crate::model::RunType { width, depth, round, material: row[6].clone() });
        }
        if has_product_columns {
            let product = crate::library::Product {
                description: row[7].clone(),
                brand: row[8].clone(),
                article: row[9].clone(),
                manufacturer: row[10].clone(),
                mass_kg: if row[11].is_empty() { None } else { Some(row[11].parse().map_err(|_| "ODS mass must be a number")?) },
            };
            let product = (product != crate::library::Product::default()).then_some(product);
            products.push((kind.clone(), name.clone(), product));
        }
        applied.push((kind, name.clone(), row[0].is_empty()));
        count += 1;
    }
    model.sync_library();
    for (kind, name, added) in applied {
        model.library.set_placeholder(&kind, &name, false)?;
        if added {
            model.library.mark_catalogue_added(&kind, &name)?;
        }
    }
    for (kind, name, product) in products {
        model.library.set_product(&kind, &name, product)?;
    }
    Ok(count)
}

/// Export literal strings, including strings beginning with spreadsheet formula characters.
pub fn write(rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    if rows.len() > MAX_ROWS {
        return Err("ODS catalogue has too many rows".into());
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" office:version=\"1.3\"><office:body><office:spreadsheet><table:table table:name=\"Каталог\">",
    );
    for row in rows {
        if row.len() > MAX_COLUMNS {
            return Err("ODS catalogue has too many columns".into());
        }
        xml.push_str("<table:table-row>");
        for cell in row {
            if cell.len() > MAX_CELL || cell.chars().any(|c| c < ' ' && !matches!(c, '\n' | '\r' | '\t')) {
                return Err("ODS cell is too long or contains control characters".into());
            }
            xml.push_str("<table:table-cell office:value-type=\"string\"><text:p>");
            xml.push_str(&escaped(cell));
            xml.push_str("</text:p></table:table-cell>");
        }
        xml.push_str("</table:table-row>");
        if xml.len() as u64 > MAX_XML {
            return Err("ODS catalogue exceeds the size limit".into());
        }
    }
    xml.push_str("</table:table></office:spreadsheet></office:body></office:document-content>");
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let manifest = format!(
        "<?xml version=\"1.0\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{MIME}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    for (name, data) in [("mimetype", MIME), ("content.xml", xml.as_str()), ("META-INF/manifest.xml", manifest.as_str())] {
        archive.start_file(name, options).map_err(|e| e.to_string())?;
        archive.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(archive.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Read the first sheet, omitting empty rows and trailing cells without expanding office-editor padding.
pub fn read(bytes: &[u8]) -> Result<Vec<Vec<String>>, String> {
    if bytes.len() as u64 > MAX_XML {
        return Err("ODS file exceeds the size limit".into());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > 256 {
        return Err("ODS archive has too many entries".into());
    }
    let entry = archive.by_name("content.xml").map_err(|e| e.to_string())?;
    let mut xml = String::new();
    entry.take(MAX_XML + 1).read_to_string(&mut xml).map_err(|e| e.to_string())?;
    if xml.len() as u64 > MAX_XML {
        return Err("ODS content exceeds the size limit".into());
    }
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().expand_empty_elements = true;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut scalar = None;
    let (mut sheet, mut in_cell) = (false, false);
    let (mut row_repeat, mut cell_repeat) = (1, 1);
    let mut expanded_bytes = 0_u64;
    let mut empty_columns = 0;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"table" => sheet = true,
            Event::Start(e) if sheet && e.local_name().as_ref() == b"table-row" => {
                row.clear();
                empty_columns = 0;
                row_repeat = repeated(&e, b"number-rows-repeated", 1_048_576)?;
            }
            Event::Start(e) if sheet && e.local_name().as_ref() == b"table-cell" => {
                cell.clear();
                scalar = cell_scalar(&e)?;
                in_cell = true;
                cell_repeat = repeated(&e, b"number-columns-repeated", 1_048_576)?;
            }
            Event::Start(e) | Event::Empty(e) if in_cell && e.local_name().as_ref() == b"s" => {
                let count = repeated(&e, b"c", MAX_CELL)?;
                cell.extend(std::iter::repeat_n(' ', count));
            }
            Event::Start(e) | Event::Empty(e) if in_cell && e.local_name().as_ref() == b"tab" => cell.push('\t'),
            Event::Start(e) | Event::Empty(e) if in_cell && e.local_name().as_ref() == b"line-break" => cell.push('\n'),
            Event::CData(e) if in_cell => cell.push_str(&e.decode().map_err(|e| e.to_string())?),
            Event::Text(e) if in_cell => {
                let text = e.xml_content(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
                cell.push_str(&quick_xml::escape::unescape(&text).map_err(|e| e.to_string())?);
                if cell.len() > MAX_CELL {
                    return Err("ODS cell is too long".into());
                }
            }
            Event::GeneralRef(e) if in_cell => {
                let name = e.decode().map_err(|e| e.to_string())?;
                cell.push_str(&quick_xml::escape::unescape(&format!("&{name};")).map_err(|e| e.to_string())?);
                if cell.len() > MAX_CELL {
                    return Err("ODS cell is too long".into());
                }
            }
            Event::End(e) if sheet && e.local_name().as_ref() == b"table-cell" => {
                in_cell = false;
                if cell.is_empty() && scalar.is_none() {
                    empty_columns = (empty_columns + cell_repeat).min(MAX_COLUMNS + 1);
                } else {
                    if row.len() + empty_columns + cell_repeat > MAX_COLUMNS {
                        return Err("ODS row has too many cells".into());
                    }
                    row.resize(row.len() + empty_columns, String::new());
                    empty_columns = 0;
                    for _ in 0..cell_repeat {
                        let heading = rows.first().and_then(|header: &Vec<String>| header.get(row.len())).map(String::as_str);
                        let value = match (heading, scalar.as_ref()) {
                            (Some("Width_mm" | "Depth_mm" | "Height_mm" | "Mass_kg"), Some(("float", value)))
                            | (Some("Round"), Some(("boolean", value))) => value,
                            (_, Some((_, value))) if cell.is_empty() => value,
                            _ => &cell,
                        };
                        row.push(value.clone());
                    }
                }
            }
            Event::End(e) if sheet && e.local_name().as_ref() == b"table-row" => {
                if row.is_empty() {
                    continue;
                }
                if rows.len() + row_repeat > MAX_ROWS {
                    return Err("ODS catalogue has too many rows".into());
                }
                let row_bytes = std::mem::size_of::<Vec<String>>() as u64
                    + row.iter().map(|cell| cell.len() as u64 + std::mem::size_of::<String>() as u64).sum::<u64>();
                expanded_bytes += row_bytes * row_repeat as u64;
                if expanded_bytes > MAX_XML {
                    return Err("ODS expanded cells exceed the size limit".into());
                }
                rows.extend(std::iter::repeat_n(row.clone(), row_repeat));
            }
            Event::End(e) if sheet && e.local_name().as_ref() == b"table" => return Ok(rows),
            Event::DocType(_) => return Err("ODS document types are not supported".into()),
            Event::Eof => return Err("ODS first sheet is missing or incomplete".into()),
            _ => {}
        }
        if cell.len() > MAX_CELL {
            return Err("ODS cell is too long".into());
        }
    }
}

/// ODF 1.3, 19.388–19.389: stored scalars are independent of localized display text.
fn cell_scalar(e: &quick_xml::events::BytesStart<'_>) -> Result<Option<(&'static str, String)>, String> {
    let (mut kind, mut number, mut boolean) = (None, None, None);
    for attr in e.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        let name = attr.key.local_name();
        let target = match name.as_ref() {
            b"value-type" => &mut kind,
            b"value" => &mut number,
            b"boolean-value" => &mut boolean,
            _ => continue,
        };
        let value = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
        if value.len() > MAX_CELL {
            return Err("ODS cell is too long".into());
        }
        *target = Some(value.trim().to_owned());
    }
    match kind.as_deref() {
        Some("float") => Ok(number.map(|value| ("float", value))),
        Some("boolean") => boolean
            .map(|value| match value.as_str() {
                "true" | "1" => Ok(("boolean", "true".into())),
                "false" | "0" => Ok(("boolean", "false".into())),
                _ => Err("ODS boolean value must be true, false, 1 or 0".into()),
            })
            .transpose(),
        _ => Ok(None),
    }
}

fn repeated(e: &quick_xml::events::BytesStart<'_>, name: &[u8], max: usize) -> Result<usize, String> {
    for attr in e.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        if attr.key.local_name().as_ref() == name {
            let count = std::str::from_utf8(&attr.value).map_err(|e| e.to_string())?.parse::<usize>().map_err(|e| e.to_string())?;
            if count == 0 || count > max {
                return Err("ODS repetition exceeds the limit".into());
            }
            return Ok(count);
        }
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn product_details_survive_catalogue_import_and_project_reopening() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalogue.ods");
        let project = temp.path().join("project.bimcraft");
        let mut s = crate::engine::Session::new();
        s.execute("type", &json!({"category":"wire", "name":"Product", "width":6, "round":true})).unwrap();
        let mut rows = super::catalogue_rows(&s.model);
        assert_eq!(rows[0].len(), 13);
        let row = rows.iter_mut().skip(1).find(|r| r[2] == "Product").unwrap();
        row[7..12].clone_from_slice(&["Кабель огнестойкий".into(), "КПС".into(), "001-23".into(), "Изготовитель".into(), "0.08".into()]);
        let expected = row.clone();
        std::fs::write(&path, super::write(&rows).unwrap()).unwrap();
        s.execute("catalogue_import", &json!({"path":path})).unwrap();
        s.execute("save", &json!({"path":project})).unwrap();
        s.execute("open", &json!({"path":project})).unwrap();
        let rows = super::catalogue_rows(&s.model);
        assert_eq!(rows.iter().skip(1).find(|r| r[2] == "Product").unwrap(), &expected);
        let mut invalid = rows.clone();
        invalid.iter_mut().skip(1).find(|r| r[2] == "Product").unwrap()[11] = "-1".into();
        std::fs::write(&path, super::write(&invalid).unwrap()).unwrap();
        assert!(s.execute("catalogue_import", &json!({"path":path})).is_err());
        assert_eq!(super::catalogue_rows(&s.model), rows);
        let legacy: Vec<_> = rows.iter().map(|row| row[..7].to_vec()).collect();
        std::fs::write(&path, super::write(&legacy).unwrap()).unwrap();
        s.execute("catalogue_import", &json!({"path":path})).unwrap();
        assert_eq!(super::catalogue_rows(&s.model), rows);
    }

    fn xml_archive(xml: &str) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn office_editors_can_repeat_unused_rows_and_columns() {
        let bytes = xml_archive(
            "<table><table-row><table-cell><p>GUID</p></table-cell><table-cell number-columns-repeated=\"16383\"/></table-row><table-row number-rows-repeated=\"1048575\"><table-cell number-columns-repeated=\"16384\"/></table-row></table>",
        );
        let rows = super::read(&bytes).unwrap();
        assert_eq!(rows, vec![vec!["GUID"]]);
    }

    #[test]
    fn office_text_spacing_and_cdata_are_preserved() {
        let bytes = xml_archive(
            "<table><table-row><table-cell><p>Прибор<s c=\"2\"/><span>А</span><tab/>Б<line-break/>В<![CDATA[ <&>]]></p></table-cell></table-row></table>",
        );
        assert_eq!(super::read(&bytes).unwrap(), vec![vec!["Прибор  А\tБ\nВ <&>"]]);
    }

    #[test]
    fn office_numeric_cells_use_stored_values_but_product_codes_keep_their_display() {
        let mut s = crate::engine::Session::new();
        let header =
            super::catalogue_rows(&s.model)[0].iter().map(|name| format!("<table-cell><p>{name}</p></table-cell>")).collect::<String>();
        let data = r#"
            <table-cell/><table-cell><p>wire</p></table-cell><table-cell><p>Кабель</p></table-cell>
            <table-cell office:value-type="float" office:value="6.5"><p>6,50</p></table-cell>
            <table-cell office:value="6&#46;5" office:value-type="float"/>
            <table-cell office:value-type="boolean" office:boolean-value="true"><p>ИСТИНА</p></table-cell>
            <table-cell number-columns-repeated="3"/>
            <table-cell office:value-type="float" office:value="123"><p>000123</p></table-cell>
            <table-cell><p>Изготовитель</p></table-cell>
            <table-cell office:value-type="float" office:value="0.075"><p>0,075 кг</p></table-cell>
        "#;
        let rows =
            super::read(&xml_archive(&format!("<table><table-row>{header}</table-row><table-row>{data}</table-row></table>"))).unwrap();
        assert_eq!(&rows[1][3..6], &["6.5", "6.5", "true"]);
        assert_eq!(rows[1][9], "000123");
        assert_eq!(rows[1][11], "0.075");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalogue.ods");
        std::fs::write(&path, xml_archive(&format!("<table><table-row>{header}</table-row><table-row>{data}</table-row></table>")))
            .unwrap();
        s.execute("catalogue_import", &json!({"path":path})).unwrap();
        s.model.check().unwrap();
        assert_eq!(s.model.types.runs["wire"]["Кабель"].width, 6.5);
        let exported = super::catalogue_rows(&s.model);
        let cable = exported.iter().find(|row| row[2] == "Кабель").unwrap();
        assert_eq!(cable[9], "000123");
        assert_eq!(cable[11], "0.075");
        let before = s.model.clone();
        let invalid = data.replace("office:value=\"6.5\"", "office:value=\"NaN\"");
        std::fs::write(&path, xml_archive(&format!("<table><table-row>{header}</table-row><table-row>{invalid}</table-row></table>")))
            .unwrap();
        assert!(s.execute("catalogue_import", &json!({"path":path})).is_err());
        assert_eq!(s.model, before);
    }

    #[test]
    fn office_repeated_numeric_cells_use_each_columns_meaning() {
        let bytes = xml_archive(
            r#"<table><table-row><table-cell><p>Name</p></table-cell><table-cell><p>Width_mm</p></table-cell><table-cell><p>Round</p></table-cell></table-row>
            <table-row><table-cell office:value-type="float" office:value="1.5" number-columns-repeated="2"><p>1,50</p></table-cell>
            <table-cell office:value-type="boolean" office:boolean-value="false"/></table-row></table>"#,
        );
        assert_eq!(super::read(&bytes).unwrap()[1], vec!["1,50", "1.5", "false"]);
    }

    #[test]
    fn office_stored_cell_values_obey_the_same_size_limit_as_text() {
        let bytes = xml_archive(&format!(
            "<table><table-row><table-cell office:value-type=\"float\" office:value=\"{}\"/></table-row></table>",
            "1".repeat(super::MAX_CELL + 1)
        ));
        assert!(super::read(&bytes).is_err());
    }

    #[test]
    fn office_boolean_values_accept_xml_schema_spelling_and_reject_invalid_values() {
        for (stored, expected) in [("true", "true"), ("false", "false"), ("1", "true"), ("0", "false")] {
            let bytes = xml_archive(&format!(
                "<table><table-row><table-cell office:value-type=\"boolean\" office:boolean-value=\"{stored}\"/></table-row></table>"
            ));
            assert_eq!(super::read(&bytes).unwrap(), vec![vec![expected]]);
        }
        let bytes = xml_archive(
            "<table><table-row><table-cell office:value-type=\"boolean\" office:boolean-value=\"maybe\"><p>true</p></table-cell></table-row></table>",
        );
        assert!(super::read(&bytes).is_err());
    }

    #[test]
    fn an_invalid_catalogue_rolls_back_earlier_rows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalogue.ods");
        let mut s = crate::engine::Session::new();
        s.execute("type", &json!({"category":"wire", "name":"A", "width":6, "round":true})).unwrap();
        let before = s.model.clone();
        let mut rows = super::catalogue_rows(&s.model);
        let row = rows.iter_mut().skip(1).find(|r| r[2] == "A").unwrap();
        row[3] = "9".into();
        row[4] = "9".into();
        rows.push(vec![String::new(), "wire".into(), "Bad".into(), "NaN".into(), "6".into(), "true".into(), String::new()]);
        std::fs::write(&path, super::write(&rows).unwrap()).unwrap();
        assert!(s.execute("catalogue_import", &json!({"path":path})).is_err());
        assert_eq!(*s.model, *before);
    }

    #[test]
    fn a_new_catalogue_row_gets_a_library_guid_and_undo_removes_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalogue.ods");
        let mut s = crate::engine::Session::new();
        let mut rows = super::catalogue_rows(&s.model);
        rows.push(vec![String::new(), "wire".into(), "New cable".into(), "6".into(), "6".into(), "true".into(), String::new()]);
        std::fs::write(&path, super::write(&rows).unwrap()).unwrap();
        s.execute("catalogue_import", &json!({"path":path})).unwrap();
        let kind = crate::library::Kind::Type("wire".into());
        assert!(s.model.library.guid(&kind, "New cable").is_some());
        s.execute("undo", &json!({})).unwrap();
        assert!(s.model.library.guid(&kind, "New cable").is_none());
    }

    #[test]
    fn catalogue_command_updates_all_instances_without_changing_guids() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalogue.ods");
        let mut s = crate::engine::Session::new();
        s.execute("type", &json!({"category":"wire", "name":"Test cable", "width":6, "round":true})).unwrap();
        s.execute("wire", &json!({"points":[[0,0],[1000,0]], "type":"Test cable"})).unwrap();
        s.execute("wire", &json!({"points":[[0,100],[1000,100]], "type":"Test cable"})).unwrap();
        let before: Vec<_> = s.model.elements.iter().map(|(id, _)| (*id, s.model.guid(*id))).collect();
        let guid = s.model.library.guid(&crate::library::Kind::Type("wire".into()), "Test cable");
        s.execute("catalogue_export", &json!({"path":path})).unwrap();
        let mut rows = super::read(&std::fs::read(&path).unwrap()).unwrap();
        let row = rows.iter_mut().skip(1).find(|r| r[2] == "Test cable").unwrap();
        row[3] = "9".into();
        row[4] = "9".into();
        std::fs::write(&path, super::write(&rows).unwrap()).unwrap();
        s.execute("catalogue_import", &json!({"path":path})).unwrap();
        let project = temp.path().join("project.bimcraft");
        s.execute("save", &json!({"path":project})).unwrap();
        s.execute("open", &json!({"path":project})).unwrap();
        assert_eq!(s.model.types.runs["wire"]["Test cable"].width, 9.0);
        assert_eq!(guid, s.model.library.guid(&crate::library::Kind::Type("wire".into()), "Test cable"));
        assert_eq!(before, s.model.elements.iter().map(|(id, _)| (*id, s.model.guid(*id))).collect::<Vec<_>>());
    }

    #[test]
    fn catalogue_cells_round_trip_without_becoming_formulas() {
        let rows = vec![vec!["GUID".into(), "Наименование".into()], vec!["=1+1".into(), "Кабель <А> & \"Б\"".into()]];
        let bytes = super::write(&rows).unwrap();
        assert_eq!(super::read(&bytes).unwrap(), rows);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name().unwrap(), "mimetype");
    }

    #[test]
    fn malformed_and_oversized_catalogues_are_errors() {
        assert!(super::read(b"not an ods file").is_err());
        assert!(super::write(&vec![vec!["x".into()]; super::MAX_ROWS + 1]).is_err());
        assert!(super::write(&[vec!["x".repeat(super::MAX_CELL + 1)]]).is_err());
    }

    #[test]
    fn repeated_cells_cannot_expand_a_small_archive_into_unbounded_memory() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
        let xml = format!(
            "<table><table-row number-rows-repeated=\"20000\"><table-cell><p>{}</p></table-cell></table-row></table>",
            "x".repeat(4096)
        );
        zip.write_all(xml.as_bytes()).unwrap();
        assert!(super::read(&zip.finish().unwrap().into_inner()).is_err());
    }
}
