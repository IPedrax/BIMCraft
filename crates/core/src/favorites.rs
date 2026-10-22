//! Favorites: materials and families kept for every project, in a file of the user's own, so a
//! texture made once is at hand in the next project. A material is kept with the pictures its
//! texture is made of, a family with the materials it's made of, so each comes into a project whole.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::family::Family;
use crate::model::{MAX_TEXT, Material, Model};
use crate::texture::{Kind, MAX_IMAGES, Picture};

/// Favorites of each kind one file may hold, and how large the file may be, bytes.
pub const MAX_FAVORITES: usize = 500;
const MAX_FILE: u64 = 256 << 20;

/// A favorite material: itself, and the pictures its texture is made of, by id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialEntry {
    pub material: Material,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub images: BTreeMap<String, Picture>,
}

/// A favorite family: itself, and the materials it's made of that the project it came from had.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyEntry {
    pub family: Family,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub materials: BTreeMap<String, MaterialEntry>,
}

/// What a favorites file holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Favorites {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub materials: BTreeMap<String, MaterialEntry>,
    #[serde(default)]
    pub families: BTreeMap<String, FamilyEntry>,
}

/// Where the user's favorites are kept: where `BIMCRAFT_FAVORITES` says, else in the user's own configuration folder.
pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BIMCRAFT_FAVORITES").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let home = || std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let folder = if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA").filter(|p| !p.is_empty())?).join("BIMCraft")
    } else if cfg!(target_os = "macos") {
        home()?.join("Library").join("Application Support").join("BIMCraft")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".config")))?
            .join("bimcraft")
    };
    Some(folder.join("favorites.json"))
}

impl Favorites {
    /// The favorites in the file at `path`; none yet where there's no file.
    pub fn read(path: &Path) -> Result<Favorites, String> {
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Favorites::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if size >= MAX_FILE {
            return Err(format!("{} is too large ({size} bytes)", path.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let favorites: Favorites = serde_json::from_str(&text).map_err(|e| format!("{} isn't a favorites file: {e}", path.display()))?;
        favorites.check().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(favorites)
    }

    /// Whether it's sound: not more than a file may hold, names that are names, textures of pictures that are there.
    pub fn check(&self) -> Result<(), String> {
        if self.materials.len() > MAX_FAVORITES || self.families.len() > MAX_FAVORITES {
            return Err(format!("it holds more than {MAX_FAVORITES} favorites of a kind"));
        }
        let entry = |who: &str, name: &str, e: &MaterialEntry| -> Result<(), String> {
            if name.trim().is_empty() || name.chars().count() > MAX_TEXT {
                return Err(format!("{who} `{name}`: a name is up to {MAX_TEXT} characters, and not empty"));
            }
            if e.images.len() > MAX_IMAGES {
                return Err(format!("{who} `{name}`: it holds more than {MAX_IMAGES} pictures"));
            }
            match &e.material.texture {
                Some(t) => t.check(&e.images).map_err(|er| format!("{who} `{name}`: texture: {er}")),
                None => Ok(()),
            }
        };
        for (name, m) in &self.materials {
            entry("material", name, m)?;
        }
        for (name, f) in &self.families {
            if name.trim().is_empty() || name.chars().count() > MAX_TEXT {
                return Err(format!("family `{name}`: a name is up to {MAX_TEXT} characters, and not empty"));
            }
            f.family.check(&|m| f.materials.contains_key(m) || !m.is_empty()).map_err(|e| format!("family `{name}`: {e}"))?;
            for (n, m) in &f.materials {
                entry("material", n, m)?;
            }
        }
        Ok(())
    }

    /// Written to `path`, whole or not at all: the folder made if need be.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl MaterialEntry {
    /// Material `name` of `m`'s library, with the picture its texture is made of.
    pub fn of(m: &Model, name: &str) -> Option<MaterialEntry> {
        let material = m.materials.get(name)?.clone();
        let images = material
            .texture
            .iter()
            .filter(|t| t.kind == Kind::Image)
            .filter_map(|t| Some((t.image.clone(), m.images.get(&t.image)?.clone())))
            .collect();
        Some(MaterialEntry { material, images })
    }

    /// Put into `m`'s library as `name`: its pictures first (the same one once, by its id), where there's room.
    pub fn bring_into(&self, m: &mut Model, name: &str) -> Result<(), String> {
        for (id, picture) in &self.images {
            if !m.images.contains_key(id) && m.images.len() >= MAX_IMAGES {
                return Err(format!("the project holds {} pictures already; take a texture of one away first", m.images.len()));
            }
            m.images.insert(id.clone(), picture.clone());
        }
        m.materials.insert(name.to_string(), self.material.clone());
        Ok(())
    }
}

impl FamilyEntry {
    /// Family `name` of `m`, with the materials it's made of that `m`'s library has.
    pub fn of(m: &Model, name: &str) -> Option<FamilyEntry> {
        let family = m.families.get(name)?.clone();
        let materials = family.materials().into_iter().filter_map(|n| Some((n.to_string(), MaterialEntry::of(m, n)?))).collect();
        Some(FamilyEntry { family, materials })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("bimcraft-fav-{name}-{}", std::process::id()))
    }

    #[test]
    fn favorites_are_written_whole_and_read_back_and_a_missing_file_has_none() {
        let path = scratch("rw").join("deep").join("favorites.json");
        assert_eq!(Favorites::read(&path), Ok(Favorites::default()));
        let m = Model::new("x");
        let mut f = Favorites { version: 1, ..Favorites::default() };
        f.materials.insert("Mine".into(), MaterialEntry::of(&m, "Brick").unwrap());
        f.families.insert("Table".into(), FamilyEntry::of(&m, "Table").unwrap());
        f.write(&path).unwrap();
        assert!(!path.with_extension("json.part").exists(), "no part of it left behind");
        assert_eq!(Favorites::read(&path).unwrap(), f);
        // A family brings the materials it's made of; a material of a pattern, no pictures.
        assert!(f.families["Table"].materials.contains_key("Timber") && f.materials["Mine"].images.is_empty());
        std::fs::remove_dir_all(scratch("rw")).unwrap();
    }

    #[test]
    fn a_file_that_isnt_one_or_holds_more_than_it_may_is_refused_and_never_crashes_it() {
        let path = scratch("bad");
        std::fs::create_dir_all(&path).unwrap();
        let file = path.join("favorites.json");
        for (text, says) in [
            ("not json", "isn't a favorites file"),
            (r#"{"materials": {"": {"material": {"color": [1, 2, 3]}}}}"#, "name"),
            (
                r#"{"materials": {"X": {"material": {"color": [1, 2, 3], "texture": {"kind": "image", "size": [10, 10], "image": "nope"}}}}}"#,
                "picture",
            ),
            (r#"{"materials": {"X": {"material": {"color": [1, 2, 3], "texture": {"kind": "brick", "size": [0, 10]}}}}}"#, "tile"),
            (r#"{"extra": 1}"#, "unknown field"),
            (r#"{"families": {"F": {"family": {"types": {}}}}}"#, "type"),
        ] {
            std::fs::write(&file, text).unwrap();
            let e = Favorites::read(&file).unwrap_err();
            assert!(e.contains(says), "{text}: {e}");
        }
        // More than a kind may hold.
        let mut many = Favorites::default();
        for i in 0..=MAX_FAVORITES {
            many.materials.insert(format!("M{i}"), MaterialEntry { material: Material::default(), images: BTreeMap::new() });
        }
        assert!(many.check().unwrap_err().contains("more than"));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_favorite_comes_into_another_project_with_its_pictures_once() {
        let mut from = Model::new("a");
        let png = {
            let rgba: Vec<u8> = (0..16).flat_map(|_| [10, 200, 30, 255]).collect();
            tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(4, 4).unwrap()).unwrap().encode_png().unwrap()
        };
        let picture = Picture::import("grass", &png).unwrap();
        let id = picture.id();
        from.images.insert(id.clone(), picture);
        let mut texture = crate::texture::Texture::new(Kind::Image);
        texture.image = id.clone();
        from.materials.insert("Grass".into(), Material { texture: Some(texture), ..Material::default() });
        let entry = MaterialEntry::of(&from, "Grass").unwrap();
        assert_eq!(entry.images.keys().collect::<Vec<_>>(), [&id]);
        let mut into = Model::new("b");
        entry.bring_into(&mut into, "Lawn").unwrap();
        entry.bring_into(&mut into, "Meadow").unwrap();
        assert_eq!((into.images.len(), into.materials["Lawn"] == into.materials["Meadow"]), (1, true));
        into.sync_library();
        assert_eq!(into.check(), Ok(()));
        // No room for another picture, and it says so.
        let mut full = Model::new("c");
        for k in 0..MAX_IMAGES {
            let rgba: Vec<u8> = (0..4).flat_map(|_| [k as u8, 0, 0, 255]).collect();
            let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(2, 2).unwrap()).unwrap().encode_png().unwrap();
            let p = Picture::import("x", &png).unwrap();
            full.images.insert(p.id(), p);
        }
        assert!(entry.bring_into(&mut full, "Lawn").unwrap_err().contains("pictures already"));
    }
}
