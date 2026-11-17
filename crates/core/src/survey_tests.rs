//! `ifc_survey`: what a file holds, as counts and the names a format writes, so its owner can run it on a file that must not leave
//! the machine and paste back the reply. Written by hand; no one's project is here.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::engine::Session;

/// A small structural file: two beams (one of one body item, one of two), a column, a storey 300 up whose placement is at 0,
/// a property set with two properties on the beams, a quantity set on one, and an aggregation.
const IFC: &str = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.);
#2=IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.);
#3=IFCSIUNIT(*,.MASSUNIT.,.KILO.,.GRAM.);
#4=IFCUNITASSIGNMENT((#1,#2,#3));
#5=IFCPROJECT('PROJECTGUID',$,'Secret Project',$,$,$,$,$,#4);
#6=IFCCARTESIANPOINT((0.,0.,0.));
#7=IFCAXIS2PLACEMENT3D(#6,$,$);
#8=IFCLOCALPLACEMENT($,#7);
#9=IFCBUILDINGSTOREY('STOREYGUID',$,'Secret Level',$,$,#8,$,$,.ELEMENT.,300.);
#10=IFCCARTESIANPOINT((0.,0.,25.));
#11=IFCAXIS2PLACEMENT3D(#10,$,$);
#12=IFCLOCALPLACEMENT(#8,#11);
#13=IFCCARTESIANPOINT((0.,0.,40.));
#14=IFCAXIS2PLACEMENT3D(#13,$,$);
#15=IFCLOCALPLACEMENT(#8,#14);
#20=IFCEXTRUDEDAREASOLID($,$,$,100.);
#21=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#20));
#22=IFCPRODUCTDEFINITIONSHAPE($,$,(#21));
#23=IFCEXTRUDEDAREASOLID($,$,$,200.);
#24=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#20,#23));
#25=IFCPRODUCTDEFINITIONSHAPE($,$,(#24));
#30=IFCBEAM('BEAMGUIDONE',$,'Secret Beam 1',$,$,#12,#22,$,.BEAM.);
#31=IFCBEAM('BEAMGUIDTWO',$,'Secret Beam 2',$,$,#15,#25,$,.BEAM.);
#32=IFCCOLUMN('COLUMNGUID',$,'Secret Column',$,$,#12,$,$,.COLUMN.);
#40=IFCRELCONTAINEDINSPATIALSTRUCTURE('REL1',$,$,$,(#30,#31,#32),#9);
#41=IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$);
#42=IFCPROPERTYSINGLEVALUE('Remark',$,IFCLABEL('hidden value'),$);
#43=IFCPROPERTYSET('PSETGUID',$,'Pset_BeamCommon',$,(#41,#42));
#44=IFCRELDEFINESBYPROPERTIES('REL2',$,$,$,(#30,#31),#43);
#45=IFCQUANTITYVOLUME('NetVolume',$,$,0.429,$);
#46=IFCELEMENTQUANTITY('QSETGUID',$,'Qto_BeamBaseQuantities',$,$,(#45));
#47=IFCRELDEFINESBYPROPERTIES('REL3',$,$,$,(#30),#46);
#48=IFCRELAGGREGATES('REL4',$,$,$,#30,(#31));
ENDSEC;END-ISO-10303-21;
";

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bimcraft-survey-{}-{name}", std::process::id()))
}

fn survey(bytes: &[u8], name: &str) -> Result<Value, String> {
    let path = temp(name);
    std::fs::write(&path, bytes).unwrap();
    let r = Session::new().execute("ifc_survey", &json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    r
}

/// The row of `list` (an array of objects) whose `key` is `name`.
fn row<'a>(list: &'a Value, key: &str, name: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|r| r[key] == json!(name)).unwrap_or_else(|| panic!("no {key} {name} in {list}"))
}

#[test]
fn ifc_survey_counts_classes_and_names_property_sets_without_names_or_values() {
    let r = survey(IFC.as_bytes(), "counts.ifc").unwrap();
    assert_eq!(r["schema"], json!("IFC4"));
    assert_eq!(r["units"], json!({"LENGTHUNIT": "CENTIMETRE", "VOLUMEUNIT": "CUBIC_METRE", "MASSUNIT": "KILOGRAM"}));
    assert_eq!(r["encoding"], json!({"valid_utf8": true, "non_ascii_bytes": 0}));
    let beam = row(&r["classes"], "class", "IFCBEAM");
    assert_eq!(beam["count"], json!(2));
    assert_eq!(beam["predefined"], json!({"BEAM": 2}));
    // Three body items in all: one beam has one, the other two.
    assert_eq!(beam["bodies"], json!({"IFCEXTRUDEDAREASOLID": 3}));
    assert_eq!(beam["items"], json!({"1": 1, "2": 1, "3+": 0}));
    assert_eq!(
        (beam["contained"].clone(), beam["aggregated"].clone(), beam["nested"].clone(), beam["typed"].clone()),
        (json!(2), json!(1), json!(0), json!(0))
    );
    let column = row(&r["classes"], "class", "IFCCOLUMN");
    assert_eq!(
        (column["count"].clone(), column["contained"].clone(), column["items"].clone()),
        (json!(1), json!(1), json!({"1": 0, "2": 0, "3+": 0}))
    );
    let set = row(&r["property_sets"], "name", "Pset_BeamCommon");
    assert_eq!(set["count"], json!(1));
    assert_eq!(set["on"], json!({"IFCBEAM": 2}));
    assert_eq!(set["properties"], json!({"LoadBearing": {"IFCBOOLEAN": 1}, "Remark": {"IFCLABEL": 1}}));
    let quantities = row(&r["quantity_sets"], "name", "Qto_BeamBaseQuantities");
    assert_eq!(quantities["on"], json!({"IFCBEAM": 1}));
    assert_eq!(quantities["quantities"], json!({"NetVolume": {"IFCQUANTITYVOLUME": 1}}));
    let relations = row(&r["relations"], "class", "IFCRELDEFINESBYPROPERTIES");
    assert_eq!(relations["count"], json!(2));
    // What an owner's file names stays in the file: no product's or storey's name, no id, no value.
    let text = r.to_string();
    for secret in ["Secret", "hidden value", "GUID", "0.429"] {
        assert!(!text.contains(secret), "{secret} leaked into {text}");
    }
}

#[test]
fn ifc_survey_gives_each_storeys_elevation_its_placement_and_its_lowest_product() {
    // The storey says 300 and stands at 0; its lowest product stands 25 above that, and three stand in it.
    let r = survey(IFC.as_bytes(), "storeys.ifc").unwrap();
    assert_eq!(r["storeys"], json!([{"elevation": 300.0, "placement_z": 0.0, "lowest_product_z": 25.0, "products": 3}]));
}

#[test]
fn ifc_survey_says_whether_the_file_is_utf_8() {
    let mut bytes = IFC.replace("hidden value", "Cobrimento").into_bytes();
    // A Latin-1 c cedilla inside a string, as a Windows exporter writes one.
    let at = bytes.windows(10).position(|w| w == b"Cobrimento").unwrap();
    bytes.insert(at + 3, 0xE7);
    let r = survey(&bytes, "latin1.ifc").unwrap();
    assert_eq!(r["encoding"], json!({"valid_utf8": false, "non_ascii_bytes": 1}));
}

#[test]
fn ifc_survey_of_a_file_that_is_no_ifc_is_refused_saying_so() {
    assert!(survey(b"not a step file at all", "garbage.ifc").unwrap_err().contains("not a STEP file"));
    let text = temp("plain.txt");
    std::fs::write(&text, IFC).unwrap();
    let e = Session::new().execute("ifc_survey", &json!({"path": text})).unwrap_err();
    assert!(e.contains(".ifc"), "{e}");
    let _ = std::fs::remove_file(&text);
    let e = Session::new().execute("ifc_survey", &json!({"path": temp("missing.ifc")})).unwrap_err();
    assert!(e.contains("missing.ifc"), "{e}");
    assert!(Session::new().execute("ifc_survey", &json!({})).unwrap_err().contains("path"));
}

#[test]
fn ifc_survey_is_capped_and_says_how_many_rows_it_left_out() {
    // 600 classes of one entity each: 500 rows, and the other 100 counted.
    let mut text = String::from("ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;\n");
    for i in 0..600 {
        text.push_str(&format!("#{}=IFCQZ{i:03}($);\n", i + 1));
    }
    text.push_str("ENDSEC;END-ISO-10303-21;\n");
    let r = survey(text.as_bytes(), "many.ifc").unwrap();
    assert_eq!(r["classes"].as_array().unwrap().len(), 500);
    assert_eq!(r["left_out"]["classes"], json!(100));
    assert!(r.to_string().len() <= crate::survey::MAX_BYTES);
}

/// Payloads for the sweep: files that are not what the command wants, in `dir`.
pub(crate) fn sweep_payloads(dir: &Path) -> Vec<(&'static str, Value)> {
    let _ = std::fs::create_dir_all(dir);
    let (garbage, one, folder) = (dir.join("garbage.ifc"), dir.join("one.ifc"), dir.join("folder.ifc"));
    let _ = std::fs::write(&garbage, (0..4096u32).map(|i| u8::try_from(i * 7919 % 251).unwrap_or(0)).collect::<Vec<u8>>());
    let _ = std::fs::write(&one, b"#");
    let _ = std::fs::create_dir_all(&folder);
    vec![
        ("ifc_survey", json!({"path": garbage})),
        ("ifc_survey", json!({"path": one})),
        ("ifc_survey", json!({"path": folder})),
        ("ifc_survey", json!({"path": dir.join("missing.ifc")})),
        ("ifc_survey", json!({"path": 5})),
        ("ifc_survey", json!({"path": ["a", "b"]})),
        ("ifc_survey", json!({})),
    ]
}
