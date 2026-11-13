//! A synthetic project for tests: a container with `BasicFileInfo`, the schema, the element table and one or more partitions,
//! holding the elements and levels a test names, written in the two record forms the reader knows (up to 2023 and from 2024).
//! Autodesk's files are never committed; the bytes here follow the layouts in `inventory.rs`, which real files were checked
//! against.

use crate::fixture::{self, Basic, Class};

/// The tags the fixture's schema gives its classes (12 is the first tag a schema has).
pub const OUTLINE: u16 = 12;
pub const PARENTS: u16 = 13;
pub const HEADER: u16 = 14;
pub const PLANE: u16 = 15;
pub const CELL_LIST: u16 = 16;
pub const REGIONS: u16 = 17;
pub const MATERIAL: u16 = 18;
pub const PHYSICAL: u16 = 19;
pub const PATTERN: u16 = 20;
pub const PROFILE_ROOF: u16 = 21;
/// `Roof`, the class of a roof by extrusion.
pub const EXTRUSION_ROOF: u16 = 22;
/// `ElementGroup`, a model group's class; `Toposolid` and `SiteSurface`, terrain's.
pub const GROUP: u16 = 23;
pub const TOPOSOLID: u16 = 24;
pub const SITE_SURFACE: u16 = 25;
/// The tag a name is framed by in the fixture's type data (any tag but `ffff` will do).
pub const NAME_TAG: u16 = 0x1002;

const KIND_PLACED: u32 = 0xffff_ef7f;
const KIND_SYMBOL: u32 = 0xffff_8000;
pub const KIND_TYPE: u32 = 0xffff_8080;
pub const KIND_MATERIAL: u32 = 0xffff_8020;

/// A record: an element of `category`, a placed instance or not (a type's symbol).
#[derive(Clone, Debug)]
pub struct Rec {
    pub id: u32,
    pub category: i64,
    pub placed: bool,
    pub class_tag: u16,
    /// A record that sits inside the body of another record, which names it (the second prologue). From 2024 only.
    pub inside: Option<u32>,
    /// The id the record's own header holds when it has one of its own: set 0 to leave it undeclared in the element table.
    pub declared: bool,
    /// The model box in feet.
    pub bbox: [f64; 6],
    /// The two reference lists after the box.
    pub refs: Vec<u64>,
    pub refs2: Vec<u64>,
    /// The items of the list after the header tag, which only a workshared file's record holds (up to 2023).
    pub items: usize,
}

impl Rec {
    pub fn new(id: u32, category: i64) -> Self {
        Self {
            id,
            category,
            placed: true,
            class_tag: 100,
            inside: None,
            declared: true,
            bbox: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            refs: Vec::new(),
            refs2: Vec::new(),
            items: 0,
        }
    }

    /// With this box and these references.
    pub fn at(mut self, bbox: [f64; 6], refs: &[u64]) -> Self {
        self.bbox = bbox;
        self.refs = refs.to_vec();
        self
    }
}

/// A record with no box: a type (kind `0xffff8080`) or a material (`0xffff8020`).
#[derive(Clone, Debug)]
pub struct TypeRec {
    pub id: u32,
    pub category: i64,
    pub kind: u32,
    /// From 2024: the type's frame has no id of its own and sits in the body of a chain record of its id (the second prologue).
    pub inside: bool,
    /// The items of the list after the header tag, which only a workshared file's record holds (up to 2023).
    pub items: usize,
}

impl TypeRec {
    pub fn new(id: u32, category: i64) -> Self {
        Self { id, category, kind: KIND_TYPE, inside: false, items: 0 }
    }
}

/// An element's serialised data: the header, its id, and these bytes.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: u32,
    pub payload: Vec<u8>,
}

/// A family or family type's name entry, with the category it is of.
#[derive(Clone, Debug)]
pub struct NameEntry {
    pub id: u32,
    pub name: String,
    pub category: i64,
}

/// A material object, which names itself.
#[derive(Clone, Debug)]
pub struct Mat {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Lev {
    pub id: u32,
    pub name: String,
    pub feet: f64,
    pub placed: bool,
    /// False leaves out its name block.
    pub named: bool,
    /// False leaves out the second copy of the elevation that confirms the first.
    pub confirmed: bool,
    /// The Building Story setting the block holds, or none for a block with the frame left out.
    pub story: Option<bool>,
    /// A level that sits inside another element, which its block names (from 2024 only).
    pub owner: Option<u32>,
    /// The items of the list after the header tag, which only a workshared file's record holds (up to 2023).
    pub items: usize,
}

impl Lev {
    pub fn new(id: u32, name: &str, feet: f64) -> Self {
        Self { id, name: name.to_string(), feet, placed: true, named: true, confirmed: true, story: None, owner: None, items: 0 }
    }
}

/// Where a material's object keeps its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NameStyle {
    /// After the class tag: two zero words, a count and the text.
    #[default]
    Own,
    /// Framed by `ff ff ff ff` and the tag of `PatternHelper` (2024 and later).
    PatternFramed,
    /// After the built-in parameter `-1001203` and four zero bytes.
    ParamEntry,
    /// Closed by eight zero bytes, an ElementId and `ff` times eight (2024 and later).
    Closed,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub release: u32,
    pub recs: Vec<Rec>,
    pub levels: Vec<Lev>,
    pub types: Vec<TypeRec>,
    pub data: Vec<Block>,
    pub mats: Vec<Mat>,
    pub names: Vec<NameEntry>,
    /// A room's object: its id, number and name.
    pub room_names: Vec<(u32, String, String)>,
    /// Geometry nodes of columns: the id and the two boxes (feet) at 96 and 48 bytes before it.
    pub nodes: Vec<(u32, [f64; 6], [f64; 6])>,
    /// Ids the element table declares beyond the ones above.
    pub extra_ids: Vec<u32>,
    /// How many gzip members the partition's bytes are cut into (a record can run across two).
    pub members: usize,
    /// The name the file records for its last save (`Username`).
    pub saved_by: String,
    /// Where the materials' objects keep their names.
    pub name_style: NameStyle,
    /// What the object styles say a category's material is (the category and the material's id): written once for each.
    pub category_materials: Vec<(i64, u32)>,
}

impl Project {
    pub fn new(release: u32) -> Self {
        Self {
            release,
            recs: Vec::new(),
            levels: Vec::new(),
            types: Vec::new(),
            data: Vec::new(),
            mats: Vec::new(),
            names: Vec::new(),
            nodes: Vec::new(),
            room_names: Vec::new(),
            extra_ids: Vec::new(),
            members: 1,
            saved_by: String::new(),
            name_style: NameStyle::default(),
            category_materials: Vec::new(),
        }
    }

    fn wide(&self) -> bool {
        self.release >= 2024
    }

    /// The inflated bytes of the partition.
    pub fn partition_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut blocks = Vec::new();
        if self.wide() {
            let hosts: Vec<u32> = self.recs.iter().filter_map(|r| r.inside).collect();
            for r in self.recs.iter().filter(|r| r.inside.is_none()) {
                out.extend_from_slice(&record64(r.id.into(), r, &[]));
            }
            for host in hosts {
                let inside: Vec<&Rec> = self.recs.iter().filter(|r| r.inside == Some(host)).collect();
                let mut body = Vec::new();
                for r in inside {
                    body.extend_from_slice(&[0x33; 10]);
                    body.extend_from_slice(&frame64(0, r));
                }
                let host_rec = Rec { placed: true, ..Rec::new(host, -2_000_011) };
                out.extend_from_slice(&record64(host.into(), &host_rec, &body));
            }
            for l in &self.levels {
                out.extend_from_slice(&level64(l));
            }
            for t in &self.types {
                out.extend_from_slice(&type64(t));
            }
        } else {
            for r in &self.recs {
                out.extend_from_slice(&record32(r));
                out.extend_from_slice(&[0x44; 5]);
            }
            for l in &self.levels {
                out.extend_from_slice(&level32(l));
                out.extend_from_slice(&[0x44; 3]);
            }
            for t in &self.types {
                out.extend_from_slice(&type32(t));
                out.extend_from_slice(&[0x44; 3]);
            }
        }
        for b in &self.data {
            blocks.extend_from_slice(&data_block(b, self.wide()));
        }
        for m in &self.mats {
            blocks.extend_from_slice(&material_object(m, self.wide(), self.name_style));
        }
        for &(category, material) in &self.category_materials {
            blocks.extend_from_slice(&category_material(category, material, self.release));
        }
        for n in &self.names {
            blocks.extend_from_slice(&name_entry(n, self.wide()));
        }
        for (id, number, name) in &self.room_names {
            blocks.extend_from_slice(&room_object(*id, number, name, self.wide()));
        }
        for (id, first, second) in &self.nodes {
            blocks.extend_from_slice(&[0x13; 5]);
            blocks.extend_from_slice(&bbox(first));
            blocks.extend_from_slice(&bbox(second));
            blocks.extend_from_slice(&u64::from(*id).to_le_bytes());
            blocks.extend_from_slice(&3u64.to_le_bytes());
            blocks.extend_from_slice(&u64::MAX.to_le_bytes());
        }
        for l in self.levels.iter().filter(|l| l.named) {
            blocks.extend_from_slice(&name_block(l, self.wide()));
        }
        out.extend_from_slice(&blocks);
        out
    }

    /// The element table.
    pub fn element_table(&self) -> Vec<u8> {
        let mut ids: Vec<u32> = self.recs.iter().filter(|r| r.declared).map(|r| r.id).collect();
        ids.extend(self.recs.iter().filter_map(|r| r.inside));
        ids.extend(self.levels.iter().map(|l| l.id));
        ids.extend(self.types.iter().map(|t| t.id));
        ids.extend(self.mats.iter().map(|m| m.id));
        ids.extend(self.names.iter().map(|n| n.id));
        ids.extend(self.room_names.iter().map(|r| r.0));
        ids.extend(&self.extra_ids);
        // A table of one record fits both record sizes; a real one holds thousands of ids.
        ids.extend(900_001..900_006);
        ids.sort_unstable();
        ids.dedup();
        let mut out = vec![0xaa, 0x05];
        out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
        for id in &ids {
            if self.wide() {
                out.extend_from_slice(&u64::from(*id).to_le_bytes());
                out.extend_from_slice(&[0; 12]);
                out.extend_from_slice(&u64::from(*id).to_le_bytes());
                out.extend_from_slice(&u64::MAX.to_le_bytes());
                out.extend_from_slice(&[0; 4]);
            } else {
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&[0; 16]);
                out.extend_from_slice(&u32::MAX.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0; 19]);
        out
    }

    pub fn schema_bytes() -> Vec<u8> {
        fixture::schema(&[
            Class { name: "Outline", fields: &["m_min"], base: None },
            Class { name: "ElementParents", fields: &["m_ids"], base: None },
            Class { name: "ElementHeader", fields: &[], base: None },
            Class { name: "Plane", fields: &["m_origin"], base: None },
            Class { name: "CellList", fields: &["m_cells"], base: None },
            Class { name: "VerticalRegionsStructure", fields: &["m_layers"], base: None },
            Class { name: "Material", fields: &["m_name"], base: None },
            Class { name: "PhysicalParamSet", fields: &["m_set"], base: None },
            Class { name: "PatternHelper", fields: &["m_pattern"], base: None },
            Class { name: "ProfileRoof", fields: &["m_sketch"], base: None },
            Class { name: "Roof", fields: &["m_extrusion"], base: None },
            Class { name: "ElementGroup", fields: &["m_members"], base: None },
            Class { name: "Toposolid", fields: &["m_surface"], base: None },
            Class { name: "SiteSurface", fields: &["m_points"], base: None },
        ])
    }

    /// The container as a Revit file.
    pub fn file(&self) -> Vec<u8> {
        self.file_from(&self.partition_bytes())
    }

    /// The container with these bytes as its partition (cut into the project's number of gzip members).
    pub fn file_from(&self, all: &[u8]) -> Vec<u8> {
        let cut = |k: usize| all.len() * k / self.members.max(1);
        let chunks: Vec<&[u8]> = (0..self.members.max(1)).map(|k| &all[cut(k)..cut(k + 1)]).collect();
        let basic = fixture::basic_file_info(&Basic { release: self.release, username: &self.saved_by, ..Basic::default() });
        fixture::container(&[
            ("BasicFileInfo", &basic),
            ("Formats/Latest", &fixture::gzip(&Self::schema_bytes())),
            ("Global/ElemTable", &fixture::global(&self.element_table())),
            ("Partitions/21", &fixture::partition(&chunks)),
        ])
    }
}

fn marker() -> [u8; 8] {
    let mut m = [0xff; 8];
    m[..2].copy_from_slice(&OUTLINE.to_le_bytes());
    m[6..].copy_from_slice(&PARENTS.to_le_bytes());
    m
}

/// The bounding box, as six `f64`.
fn bbox(b: &[f64; 6]) -> Vec<u8> {
    b.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The two reference lists: a count and the slots (`u64`, or `u32` up to 2023); the second only follows a first.
fn lists(r: &Rec, wide: bool) -> Vec<u8> {
    let mut f = Vec::new();
    let mut list = |l: &[u64]| {
        f.extend_from_slice(&(l.len() as u32).to_le_bytes());
        for v in l {
            if wide {
                f.extend_from_slice(&v.to_le_bytes());
            } else {
                f.extend_from_slice(&(*v as u32).to_le_bytes());
            }
        }
    };
    list(&r.refs);
    if !r.refs.is_empty() || !wide {
        list(&r.refs2);
    }
    f
}

/// The fields of a 2024 and later frame from `+0x10` to the end of the box and an empty reference list.
fn frame64(raw_id: u64, r: &Rec) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&raw_id.to_le_bytes());
    f.extend_from_slice(&0x8cu32.to_le_bytes()); // the record's size
    let constant = PARENTS.wrapping_sub(12);
    f.extend_from_slice(&constant.to_le_bytes());
    f.extend_from_slice(&0u16.to_le_bytes());
    f.extend_from_slice(&0u16.to_le_bytes()); // +0x10
    f.extend_from_slice(&r.category.to_le_bytes()); // +0x12
    f.extend_from_slice(&[0xff; 16]); // +0x1a
    f.extend_from_slice(&[0xff; 8]); // +0x2a design option
    f.extend_from_slice(&u64::MAX.to_le_bytes()); // +0x32 container
    f.extend_from_slice(&[0xff; 8]); // +0x3a
    f.extend_from_slice(&(if r.placed { KIND_PLACED } else { KIND_SYMBOL }).to_le_bytes()); // +0x42
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&r.class_tag.to_le_bytes()); // +0x4a
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&marker()); // +0x50
    f.extend_from_slice(&bbox(&r.bbox)); // +0x58
    f.extend_from_slice(&lists(r, true)); // +0x88 reference lists
    f
}

/// A chain record: a frame (or, with a `body`, a frame and then more), and the trailer that repeats the size.
fn record64(id: u64, r: &Rec, body: &[u8]) -> Vec<u8> {
    let mut f = frame64(id, r);
    f.extend_from_slice(body);
    let size = f.len() as u32;
    f[8..12].copy_from_slice(&size.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&size.to_le_bytes());
    f
}

fn level64(l: &Lev) -> Vec<u8> {
    let r = Rec { placed: l.placed, ..Rec::new(l.id, -2_000_240) };
    let mut f = frame64(l.id.into(), &r);
    f.truncate(0x50);
    f.extend_from_slice(&marker()[2..]); // the six bytes of the marker, without the Outline tag
    f.extend_from_slice(&2u16.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    let size = f.len() as u32;
    f[8..12].copy_from_slice(&size.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&size.to_le_bytes());
    f
}

/// The count and the items of the list after a header tag: a workshared file's record holds some, another file's none.
fn list_items(n: usize) -> Vec<u8> {
    let mut b = (n as u32).to_le_bytes().to_vec();
    for _ in 0..n {
        b.extend_from_slice(&[0xff; 8]);
        b.extend_from_slice(&[1, 0, 0x9a, 0x34, 0, 0]);
    }
    b
}

/// An element record of up to 2023, laid out back from its marker.
fn record32(r: &Rec) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&r.id.to_le_bytes()); // m-52
    f.extend_from_slice(&112u32.to_le_bytes());
    f.extend_from_slice(&HEADER.to_le_bytes()); // m-44 with no items
    f.extend_from_slice(&list_items(r.items));
    f.extend_from_slice(&r.category.to_le_bytes()); // m-38
    f.extend_from_slice(&[0xff; 8]);
    f.extend_from_slice(&u32::MAX.to_le_bytes()); // m-22 container
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&(if r.placed { KIND_PLACED } else { KIND_SYMBOL }).to_le_bytes()); // m-14
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&r.class_tag.to_le_bytes()); // m-6
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&marker()); // m
    f.extend_from_slice(&bbox(&r.bbox)); // m+8
    f.extend_from_slice(&lists(r, false));
    f
}

fn level32(l: &Lev) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&l.id.to_le_bytes()); // id_at = category - 14, and 14 more for each item
    f.extend_from_slice(&66u32.to_le_bytes());
    f.extend_from_slice(&HEADER.to_le_bytes());
    f.extend_from_slice(&list_items(l.items));
    f.extend_from_slice(&(-2_000_240i64).to_le_bytes()); // the category
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&u32::MAX.to_le_bytes()); // +16 container
    f.extend_from_slice(&[0xff; 4]);
    f.extend_from_slice(&(if l.placed { KIND_PLACED } else { KIND_SYMBOL }).to_le_bytes()); // +24
    f.extend_from_slice(&[0; 10]);
    f.extend_from_slice(&marker()[2..]); // +38
    f.extend_from_slice(&[0; 8]);
    f
}

/// The block that names a level and gives its elevation (twice, the second time with the 24 bytes before it).
fn name_block(l: &Lev, wide: bool) -> Vec<u8> {
    let mut b = vec![0x11; 7];
    if let (true, Some(other)) = (wide, l.owner) {
        // The block of a level inside another element: its id, then eight `ff` and the owner's id where a project level has
        // an unbroken run of 56.
        b.extend_from_slice(&[1, 0, 0, 0]);
        b.extend_from_slice(&u64::from(l.id).to_le_bytes());
        b.extend_from_slice(&[0xff; 8]);
        b.extend_from_slice(&u64::from(other).to_le_bytes());
        b.extend_from_slice(&[0xff; 40]);
    } else if wide {
        b.extend_from_slice(&u64::from(l.id).to_le_bytes());
        b.extend_from_slice(&[0xff; 56]);
    } else {
        b.extend_from_slice(&l.id.to_le_bytes());
        b.extend_from_slice(&[0xff; 28]);
    }
    b.extend_from_slice(&[0, 0, 0]);
    b.extend_from_slice(&(l.name.encode_utf16().count() as u32).to_le_bytes());
    b.extend(l.name.encode_utf16().flat_map(u16::to_le_bytes));
    if let Some(story) = l.story {
        // An unset id, `00 01`, a number, the level's id and the setting.
        b.extend_from_slice(&if wide { vec![0xff; 8] } else { vec![0xff; 4] });
        b.extend_from_slice(&[0, 1]);
        b.extend_from_slice(&0.0f64.to_le_bytes());
        if wide {
            b.extend_from_slice(&u64::from(l.id).to_le_bytes());
        } else {
            b.extend_from_slice(&l.id.to_le_bytes());
        }
        b.push(u8::from(story));
    }
    b.extend_from_slice(&[0x22; 10]);
    b.extend_from_slice(&[5, 0, 0, 0]);
    b.extend_from_slice(&PLANE.to_le_bytes());
    let filler: Vec<u8> = (0..49u8).map(|i| 0x30 + i).collect();
    b.extend_from_slice(&filler);
    let feet = l.feet.to_le_bytes();
    b.extend_from_slice(&feet);
    b.extend_from_slice(&[0x55; 100]);
    if l.confirmed {
        b.extend_from_slice(&filler[25..]);
        b.extend_from_slice(&feet);
    }
    // Far enough that the next block's setting is not within reach of this one's search.
    b.extend_from_slice(&[0x66; 0x300]);
    b
}

/// A box-less record of 2024 and later: a level's frame, with the type's kind and a slot list holding its own id.
fn type64(t: &TypeRec) -> Vec<u8> {
    if t.inside {
        // A chain record of the type's id, with the type's frame, without an id of its own, in its body.
        let host = Rec { placed: false, ..Rec::new(t.id, t.category) };
        let mut body = vec![0x33; 10];
        body.extend_from_slice(&type_frame64(0, t));
        return record64(t.id.into(), &host, &body);
    }
    let mut f = type_frame64(t.id.into(), t);
    let size = f.len() as u32;
    f[8..12].copy_from_slice(&size.to_le_bytes());
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&size.to_le_bytes());
    f
}

/// The frame of a box-less record of 2024 and later, as far as its slot list (which names the type itself).
fn type_frame64(raw_id: u64, t: &TypeRec) -> Vec<u8> {
    let r = Rec { placed: true, ..Rec::new(t.id, t.category) };
    let mut f = frame64(raw_id, &r);
    f.truncate(0x50);
    f[0x42..0x46].copy_from_slice(&t.kind.to_le_bytes());
    f.extend_from_slice(&marker()[2..]); // +0x50
    f.extend_from_slice(&2u32.to_le_bytes()); // +0x56 the slot list: another element's id and its own
    f.extend_from_slice(&7u64.to_le_bytes());
    f.extend_from_slice(&u64::from(t.id).to_le_bytes());
    f
}

/// A box-less record up to 2023: a level's frame.
fn type32(t: &TypeRec) -> Vec<u8> {
    let mut f = level32(&Lev { placed: true, items: t.items, ..Lev::new(t.id, "", 0.0) });
    let at = 14 + 14 * t.items; // the category follows the id, size, header tag, the count and the items
    f[at..at + 8].copy_from_slice(&t.category.to_le_bytes());
    f[at + 24..at + 28].copy_from_slice(&t.kind.to_le_bytes());
    f
}

/// An element's data: `ff ff ff ff`, the tag of `CellList`, `01 00 00 00`, the id (a `u32` and four more bytes up to 2023), and
/// the payload.
fn data_block(b: &Block, wide: bool) -> Vec<u8> {
    let mut f = vec![0x77; 5];
    f.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    f.extend_from_slice(&CELL_LIST.to_le_bytes());
    f.extend_from_slice(&[1, 0, 0, 0]);
    if wide {
        f.extend_from_slice(&u64::from(b.id).to_le_bytes());
    } else {
        f.extend_from_slice(&b.id.to_le_bytes());
        f.extend_from_slice(&[0x11; 4]);
    }
    f.extend_from_slice(&b.payload);
    f
}

/// What the object styles hold for a category's material: from 2024 (and only in 2024) the category's id, an unset id, `01 00 00 00`,
/// another unset id and the material; before, the `i32` id, `ff` times four, a word of 1, a 1, the frame of `PatternHelper`, eight
/// bytes, `i32 -3000010` and the material.
fn category_material(category: i64, material: u32, release: u32) -> Vec<u8> {
    let mut f = vec![0x14; 3];
    if release >= 2024 {
        f.extend_from_slice(&category.to_le_bytes());
        f.extend_from_slice(&[0xff; 8]);
        f.extend_from_slice(&[1, 0, 0, 0]);
        f.extend_from_slice(&[0xff; 8]);
        f.extend_from_slice(&u64::from(material).to_le_bytes());
    } else {
        f.extend_from_slice(&(category as i32).to_le_bytes());
        f.extend_from_slice(&[0xff; 4]);
        f.extend_from_slice(&1u32.to_le_bytes());
        f.extend_from_slice(&1u32.to_le_bytes());
        f.extend_from_slice(&[0xff; 4]);
        f.extend_from_slice(&PATTERN.to_le_bytes());
        f.extend_from_slice(&[0x15; 8]);
        f.extend_from_slice(&(-3_000_010i32).to_le_bytes());
        f.extend_from_slice(&material.to_le_bytes());
    }
    f.extend_from_slice(&[0x16; 3]);
    f
}

/// A material object: `01 00 00 00`, its id, the class tag at the fixed distance and its name, where `style` puts it.
fn material_object(m: &Mat, wide: bool, style: NameStyle) -> Vec<u8> {
    let mut f = vec![0x55; 3];
    f.extend_from_slice(&[1, 0, 0, 0]);
    let units: Vec<u8> = m.name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let count = (m.name.encode_utf16().count() as u32).to_le_bytes();
    let name = match style {
        NameStyle::Own => [&[0u8; 8][..], &count, &units, &[0x66; 40]].concat(),
        NameStyle::PatternFramed => [&[0x66u8; 16][..], &[0xff; 4], &PATTERN.to_le_bytes(), &count, &units, &[0x66; 40]].concat(),
        NameStyle::ParamEntry => {
            let parameter = if wide { (-1_001_203i64).to_le_bytes().to_vec() } else { (-1_001_203i32).to_le_bytes().to_vec() };
            [&[0x66u8; 16][..], &parameter, &[0; 4], &count, &units, &[0x66; 40]].concat()
        }
        NameStyle::Closed => [&[0x66u8; 16][..], &count, &units, &[0; 8], &u64::from(m.id).to_le_bytes(), &[0xff; 8], &[0x66; 40]].concat(),
    };
    if wide {
        f.extend_from_slice(&u64::from(m.id).to_le_bytes());
        f.extend_from_slice(&[0x31; 0x47 - 8]);
    } else {
        f.extend_from_slice(&m.id.to_le_bytes());
        f.extend_from_slice(&[0x31; 0x27 - 4 - 7]);
        f.extend_from_slice(&[0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    }
    f.extend_from_slice(&MATERIAL.to_le_bytes());
    f.extend_from_slice(&name);
    f
}

/// A bounded line record, as a wall's data holds its location line: the start and end parameter, the origin and the direction.
pub fn line_bytes(start: f64, end: f64, origin: [f64; 3], direction: [f64; 3]) -> Vec<u8> {
    let mut f = vec![0u8; 8]; // a line's marker word is 0
    f.extend_from_slice(&[0x04, 0x00, 0x08, 0x01]);
    for v in [start, end, origin[0], origin[1], origin[2], direction[0], direction[1], direction[2]] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f
}

/// An arc record: a marker word 1, then the angles, the axes, the radius and the centre.
pub fn arc_bytes(angles: [f64; 2], x: [f64; 3], y: [f64; 3], radius: f64, centre: [f64; 3]) -> Vec<u8> {
    let mut f = 1u64.to_le_bytes().to_vec();
    f.extend_from_slice(&[0x04, 0x00, 0x08, 0x01]);
    for v in angles.into_iter().chain(x).chain(y).chain([radius]).chain(centre) {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f
}

/// A wall's orientation: the anchor, the location line, the word and the flip with its second flag.
pub fn orientation_bytes(location_line: u32, word: u32, flip: bool, wide: bool) -> Vec<u8> {
    let mut f = vec![0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0];
    f.extend_from_slice(&location_line.to_le_bytes());
    if wide {
        f.extend_from_slice(&word.to_le_bytes());
    }
    f.extend_from_slice(&[u8::from(flip), 0, 0, 0]);
    f
}

/// A framed name: `ff ff ff ff`, a tag, a counted UTF-16 string.
pub fn name_bytes(name: &str) -> Vec<u8> {
    let mut f = vec![0xff, 0xff, 0xff, 0xff];
    f.extend_from_slice(&NAME_TAG.to_le_bytes());
    f.extend_from_slice(&(name.encode_utf16().count() as u32).to_le_bytes());
    f.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
    f
}

/// A type's layers, framed by `ff ff ff ff` and the tag of `VerticalRegionsStructure`: (width in feet, material id or none,
/// function). The release gives the record's layout.
pub fn layers_bytes(layers: &[(f64, Option<u32>, u32)], release: u32) -> Vec<u8> {
    let mut f = vec![0xff, 0xff, 0xff, 0xff];
    f.extend_from_slice(&REGIONS.to_le_bytes());
    f.extend_from_slice(&(layers.len() as u32).to_le_bytes());
    for &(width, material, function) in layers {
        let mut r = Vec::new();
        r.extend_from_slice(&width.to_le_bytes());
        match release {
            2024..=2027 => {
                r.extend_from_slice(&material.map_or(u64::MAX, u64::from).to_le_bytes());
                r.extend_from_slice(&u64::MAX.to_le_bytes());
                r.extend_from_slice(&function.to_le_bytes());
                r.extend_from_slice(&[0; 9]);
                if release >= 2026 {
                    r.extend_from_slice(&[0; 4]);
                }
            }
            _ => {
                r.extend_from_slice(&function.to_le_bytes());
                r.extend_from_slice(&[0; 4]);
                r.extend_from_slice(&material.unwrap_or(u32::MAX).to_le_bytes());
                r.extend_from_slice(&u32::MAX.to_le_bytes());
                r.extend_from_slice(&[0; 5]);
            }
        }
        f.extend_from_slice(&r);
    }
    f
}

/// A name entry: from 2024 the id, a counted name and the category; before, `01 00 00 00`, the id and a counted name.
fn name_entry(n: &NameEntry, wide: bool) -> Vec<u8> {
    let mut f = vec![0x42; 3];
    let units: Vec<u8> = n.name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let count = (n.name.encode_utf16().count() as u32).to_le_bytes();
    if wide {
        f.extend_from_slice(&u64::from(n.id).to_le_bytes());
        f.extend_from_slice(&count);
        f.extend_from_slice(&units);
        f.extend_from_slice(&n.category.to_le_bytes());
    } else {
        f.extend_from_slice(&[1, 0, 0, 0]);
        f.extend_from_slice(&n.id.to_le_bytes());
        f.extend_from_slice(&count);
        f.extend_from_slice(&units);
    }
    f.extend_from_slice(&[0x24; 3]);
    f
}

/// A family instance's placement: the rotation about up by `angle` radians, row by row, and the origin (feet).
pub fn transform_bytes(angle: f64, origin: [f64; 3]) -> Vec<u8> {
    let (sin, cos) = angle.sin_cos();
    [cos, -sin, 0.0, sin, cos, 0.0, 0.0, 0.0, 1.0, origin[0], origin[1], origin[2]].iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A room's object: `01 00 00 00`, its id, some bytes, then the number and the name as parameter entries.
fn room_object(id: u32, number: &str, name: &str, wide: bool) -> Vec<u8> {
    let mut f = vec![0x62; 3];
    f.extend_from_slice(&[1, 0, 0, 0]);
    let (number_tag, name_tag) = if wide {
        f.extend_from_slice(&u64::from(id).to_le_bytes());
        ((-1_006_901i64).to_le_bytes().to_vec(), (-1_006_900i64).to_le_bytes().to_vec())
    } else {
        f.extend_from_slice(&id.to_le_bytes());
        ((-1_006_901i32).to_le_bytes().to_vec(), (-1_006_900i32).to_le_bytes().to_vec())
    };
    f.extend_from_slice(&[0x71; 40]);
    let text = |s: &str| {
        [&(s.encode_utf16().count() as u32).to_le_bytes()[..], &s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>()].concat()
    };
    f.extend_from_slice(&number_tag);
    f.extend_from_slice(&text(number));
    f.extend_from_slice(&name_tag);
    f.extend_from_slice(&text(name));
    f.extend_from_slice(&[0x72; 20]);
    f
}
