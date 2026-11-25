use std::io::{Cursor, Read, Write};

use serde_json::json;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::engine::Session;

const CONTENT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><office:body><office:text><text:p>Ручной абзац <text:a xlink:href=\"../Info.odt#MasterShtamp\">ссылка</text:a></text:p><text:p><text:variable-set text:name=\"NameOb\" office:value-type=\"string\">Старое имя</text:variable-set></text:p><text:p><text:variable-set text:name=\"Shifr\" office:value-type=\"string\">OLD-1</text:variable-set></text:p></office:text></office:body></office:document-content>";

fn archive(content: &str, extras: usize) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/vnd.oasis.opendocument.text").unwrap();
    zip.start_file("content.xml", deflated).unwrap();
    zip.write_all(content.as_bytes()).unwrap();
    zip.start_file("styles.xml", deflated).unwrap();
    zip.write_all(b"untouched styles").unwrap();
    zip.start_file("Pictures/logo.bin", stored).unwrap();
    zip.write_all(&[0, 1, 2, 255]).unwrap();
    for index in 0..extras {
        zip.start_file(format!("extra-{index}"), stored).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result = Vec::new();
    archive.by_name(name).unwrap().read_to_end(&mut result).unwrap();
    result
}

#[test]
fn odt_template_replaces_existing_variables_and_preserves_the_document() {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("template.odt");
    let output = temp.path().join("copy.odt");
    std::fs::write(&template, archive(CONTENT, 0)).unwrap();
    let mut session = Session::new();
    let before = session.model.clone();

    let result = session
        .execute(
            "odt_template",
            &json!({
                "template": template,
                "path": output,
                "fields": {
                    "NameOb": "Новый <объект> & сеть",
                    "Shifr": "LV-42"
                }
            }),
        )
        .unwrap();

    assert_eq!(result["fields"], 2);
    let bytes = std::fs::read(&output).unwrap();
    let content = String::from_utf8(entry(&bytes, "content.xml")).unwrap();
    assert_eq!(content, CONTENT.replace("Старое имя", "Новый &lt;объект&gt; &amp; сеть").replace("OLD-1", "LV-42"));
    assert_eq!(entry(&bytes, "styles.xml"), b"untouched styles");
    assert_eq!(entry(&bytes, "Pictures/logo.bin"), [0, 1, 2, 255]);
    assert_eq!(session.model, before);
}

#[test]
fn odt_template_refuses_unknown_or_duplicate_variables_before_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("template.odt");
    let duplicate = temp.path().join("duplicate.odt");
    let output = temp.path().join("copy.odt");
    std::fs::write(&template, archive(CONTENT, 0)).unwrap();
    std::fs::write(
        &duplicate,
        archive(
            &CONTENT.replace(
                "</office:text>",
                "<text:variable-set text:name=\"NameOb\" office:value-type=\"string\">again</text:variable-set></office:text>",
            ),
            0,
        ),
    )
    .unwrap();
    std::fs::write(&output, b"keep me").unwrap();
    let mut session = Session::new();

    assert!(
        session
            .execute("odt_template", &json!({"template":template,"path":output,"fields":{"Missing":"value"}}))
            .unwrap_err()
            .contains("Missing")
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
    assert!(
        session
            .execute("odt_template", &json!({"template":duplicate,"path":output,"fields":{"NameOb":"value"}}))
            .unwrap_err()
            .contains("more than once")
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
    let original = std::fs::read(&template).unwrap();
    assert!(session.execute("odt_template", &json!({"template":template,"path":template,"fields":{"NameOb":"value"}})).is_err());
    assert_eq!(std::fs::read(&template).unwrap(), original);
}

#[test]
fn odt_template_bounds_archive_xml_and_field_values() {
    let temp = tempfile::tempdir().unwrap();
    let crowded = temp.path().join("crowded.odt");
    let doctype = temp.path().join("doctype.odt");
    let template = temp.path().join("template.odt");
    let output = temp.path().join("copy.odt");
    std::fs::write(&crowded, archive(CONTENT, 253)).unwrap();
    std::fs::write(&doctype, archive(&CONTENT.replace("<office:document-content", "<!DOCTYPE x><office:document-content"), 0)).unwrap();
    std::fs::write(&template, archive(CONTENT, 0)).unwrap();
    let mut session = Session::new();

    for params in [
        json!({"template":crowded,"path":output,"fields":{"NameOb":"value"}}),
        json!({"template":doctype,"path":output,"fields":{"NameOb":"value"}}),
        json!({"template":template,"path":output,"fields":{"NameOb":"x".repeat(16_385)}}),
        json!({"template":template,"path":output,"fields":{"NameOb":1}}),
    ] {
        assert!(session.execute("odt_template", &params).is_err(), "param {params}");
        assert!(!output.exists());
    }
}

const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"";

fn document(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS}><office:body><office:text>{body}</office:text></office:body></office:document-content>"
    )
}

#[test]
fn odt_template_preserves_carriage_returns_and_skips_formula_fields() {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("template.odt");
    let output = temp.path().join("copy.odt");
    let body = "<text:p><text:variable-set text:name=\"NameOb\" office:value-type=\"string\">old</text:variable-set></text:p><text:variable-set text:name=\"Calc\" text:formula=\"ooow:1\" office:value-type=\"string\" office:string-value=\"1\"></text:variable-set>";
    std::fs::write(&template, archive(&document(body), 0)).unwrap();
    let mut session = Session::new();

    let result = session.execute("odt_template", &json!({"template":template,"path":output,"fields":{"NameOb":"a\r\nb"}})).unwrap();
    assert_eq!(result["fields"], 1);
    let content = String::from_utf8(entry(&std::fs::read(&output).unwrap(), "content.xml")).unwrap();
    assert!(content.contains("a&#13;\nb"));
    assert!(content.contains("text:formula=\"ooow:1\""));

    let err = session.execute("odt_template", &json!({"template":template,"path":output,"fields":{"Calc":"y"}})).unwrap_err();
    assert!(err.contains("Calc"));
}

#[test]
fn odt_template_ignores_foreign_namespace_variable_set() {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("template.odt");
    let output = temp.path().join("copy.odt");
    let body = "<text:p><text:variable-set text:name=\"NameOb\" office:value-type=\"string\">real</text:variable-set></text:p><fake:variable-set xmlns:fake=\"urn:fake\" text:name=\"NameOb\">foreign</fake:variable-set>";
    std::fs::write(&template, archive(&document(body), 0)).unwrap();
    let mut session = Session::new();

    session.execute("odt_template", &json!({"template":template,"path":output,"fields":{"NameOb":"new"}})).unwrap();
    let content = String::from_utf8(entry(&std::fs::read(&output).unwrap(), "content.xml")).unwrap();
    assert!(content.contains(">new</text:variable-set>"));
    assert!(content.contains("<fake:variable-set"));
    assert!(content.contains(">foreign</fake:variable-set>"));
}

#[test]
fn odt_template_refuses_nested_empty_variables_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("template.odt");
    let output = temp.path().join("copy.odt");
    let body = "<text:variable-set text:name=\"Outer\"><text:variable-set text:name=\"Inner\"/></text:variable-set>";
    std::fs::write(&template, archive(&document(body), 0)).unwrap();

    let err = Session::new()
        .execute("odt_template", &json!({"template":template,"path":output,"fields":{"Outer":"new","Inner":"nested"}}))
        .unwrap_err();
    assert!(err.contains("cannot be nested"), "{err}");
    assert!(!output.exists());
}

#[test]
fn odt_template_respects_guest_and_dry_run_file_restrictions() {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("template.odt");
    let output = temp.path().join("copy.odt");
    std::fs::write(&template, archive(CONTENT, 0)).unwrap();
    let params = json!({"template":template,"path":output,"fields":{"NameOb":"new"}});
    let mut session = Session::new();
    session.no_files = true;
    let error = session.execute("odt_template", &params).unwrap_err();
    assert!(error.contains("Co-Work guest"), "{error}");
    session.no_files = false;
    let error = session.script_dry_run(&format!("odt_template {params}")).unwrap_err();
    assert!(error.contains("dry run"), "{error}");
    assert!(!output.exists());
}

const INFO_BODY: &str = "<text:section text:name=\"Переменные\"><text:variable-set text:name=\"NameOb\" office:value-type=\"string\">старое</text:variable-set><text:variable-set text:name=\"Shifr\" office:value-type=\"string\">old</text:variable-set></text:section>";
const TITLE_BODY: &str = "<text:section text:name=\"Linked\"><text:section-source xlink:href=\"../info.odt\" text:filter-name=\"writer8\" text:section-name=\"Переменные\"/></text:section><text:p><text:variable-set text:name=\"NameOb\" office:value-type=\"string\">кэш</text:variable-set></text:p><text:p><text:variable-set text:name=\"TitleFamilie1\" office:value-type=\"string\">Петров</text:variable-set></text:p>";
const MASTER_BODY: &str = "<text:section text:name=\"title.odt\" text:protected=\"true\"><text:section-source xlink:href=\"../title.odt\" text:filter-name=\"writer8\"/></text:section>";

fn write_set(dir: &std::path::Path, title_body: &str) -> Vec<std::path::PathBuf> {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let info = dir.join("info.odt");
    let title = src.join("title.odt");
    let master = src.join("master.odm");
    std::fs::write(&info, archive(&document(INFO_BODY), 0)).unwrap();
    std::fs::write(&title, archive(&document(title_body), 0)).unwrap();
    std::fs::write(&master, archive(&document(MASTER_BODY), 0)).unwrap();
    vec![info, title, master]
}

#[test]
fn odt_set_remaps_links_and_fills_present_fields() {
    let temp = tempfile::tempdir().unwrap();
    let sources = write_set(temp.path(), TITLE_BODY);
    let out = temp.path().join("out");
    let mut session = Session::new();
    let before = session.model.clone();

    let result = session
        .execute(
            "odt_set",
            &json!({
                "templates": sources,
                "path": out,
                "fields": {"NameOb":"Новый объект","Shifr":"LV-1","TitleFamilie1":"Иванов"}
            }),
        )
        .unwrap();
    assert_eq!(result["fields"], 3);

    let title = String::from_utf8(entry(&std::fs::read(out.join("title.odt")).unwrap(), "content.xml")).unwrap();
    assert!(title.contains("xlink:href=\"info.odt\""));
    assert!(title.contains(">Новый объект</text:variable-set>"));
    assert!(title.contains(">Иванов</text:variable-set>"));
    let info = String::from_utf8(entry(&std::fs::read(out.join("info.odt")).unwrap(), "content.xml")).unwrap();
    assert!(info.contains(">LV-1</text:variable-set>"));
    let master = String::from_utf8(entry(&std::fs::read(out.join("master.odm")).unwrap(), "content.xml")).unwrap();
    assert!(master.contains("xlink:href=\"../title.odt\""));
    assert_eq!(std::fs::read(&sources[0]).unwrap(), archive(&document(INFO_BODY), 0));
    assert_eq!(session.model, before);
}

#[test]
fn odt_set_refuses_broken_links_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = Session::new();

    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("elsewhere.odt"), archive(&document(INFO_BODY), 0)).unwrap();
    let sources = write_set(&outside, &TITLE_BODY.replace("../info.odt", "../elsewhere.odt"));
    let out = temp.path().join("out1");
    let err = session.execute("odt_set", &json!({"templates":sources,"path":out,"fields":{}})).unwrap_err();
    assert!(err.contains("leaves the document set"));
    assert!(!out.exists());

    let missing = temp.path().join("missing");
    std::fs::create_dir_all(&missing).unwrap();
    let sources = write_set(&missing, &TITLE_BODY.replace("text:section-name=\"Переменные\"", "text:section-name=\"NoSuch\""));
    let out = temp.path().join("out2");
    let err = session.execute("odt_set", &json!({"templates":sources,"path":out,"fields":{}})).unwrap_err();
    assert!(err.contains("missing section"));
    assert!(!out.exists());
}

#[test]
fn odt_set_refuses_existing_output_and_external_uri_without_touching_sources() {
    let temp = tempfile::tempdir().unwrap();
    let sources = write_set(temp.path(), TITLE_BODY);
    let existing = temp.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    let sentinel = existing.join("info.odt");
    std::fs::write(&sentinel, b"keep").unwrap();
    let original = std::fs::read(&sources[0]).unwrap();
    let mut s = Session::new();
    let error = s.execute("odt_set", &json!({"templates":sources,"path":existing,"fields":{}})).unwrap_err();
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
    assert_eq!(std::fs::read(&sources[0]).unwrap(), original);
    let remote = TITLE_BODY.replace("../info.odt", "https://example.test/info.odt");
    let sources = write_set(temp.path(), &remote);
    let output = temp.path().join("output");
    let error = s.execute("odt_set", &json!({"templates":sources,"path":output,"fields":{}})).unwrap_err();
    assert!(error.contains("relative file"), "{error}");
    assert!(!output.exists());
}
