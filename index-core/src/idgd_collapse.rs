use crate::bitmap::{BitmapStore, EffectLine, PerLineBitmapStore};
use crate::card::{translation_text, LocaleText};
use crate::compact::{remap_id_gd_fields, CompactCardFields, RECORD_SIZE};
use crate::idgd_catalog::{IdGdCatalog, IdGdCatalogBuilder};
use anyhow::{Context, Result};
use roaring::RoaringBitmap;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const COLLAPSE_LOCALE: &str = "en_US";

#[derive(Debug, Clone)]
pub struct CollapseEntry {
    pub id_gd: u32,
    pub element_type: String,
    pub translations: BTreeMap<String, LocaleText>,
}

#[derive(Debug, Default, Clone)]
pub struct IdGdAliasMap {
    alias_to_canonical: BTreeMap<u32, u32>,
}

impl IdGdAliasMap {
    pub fn from_catalog(catalog: &IdGdCatalog) -> Self {
        let mut alias_to_canonical = BTreeMap::new();
        for entry in &catalog.entries {
            for &dup in &entry.duplicated_id_gd {
                alias_to_canonical.insert(dup, entry.id_gd);
            }
        }
        Self { alias_to_canonical }
    }

    pub fn resolve(&self, id: u32) -> u32 {
        self.alias_to_canonical.get(&id).copied().unwrap_or(id)
    }

    pub fn resolve_ids(&self, ids: &[u32]) -> Vec<u32> {
        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            let resolved = self.resolve(id);
            if !out.contains(&resolved) {
                out.push(resolved);
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.alias_to_canonical.is_empty()
    }
}

/// Map alias idGd -> canonical idGd for entries sharing element type and text.
pub fn build_collapse_remap(entries: impl IntoIterator<Item = CollapseEntry>) -> BTreeMap<u32, u32> {
    let mut groups: BTreeMap<(String, String), Vec<u32>> = BTreeMap::new();

    for entry in entries {
        let text = translation_text(&entry.translations, COLLAPSE_LOCALE);
        if text.is_empty() {
            continue;
        }
        groups
            .entry((entry.element_type, text))
            .or_default()
            .push(entry.id_gd);
    }

    let mut remap = BTreeMap::new();
    for ids in groups.values() {
        if ids.len() < 2 {
            continue;
        }
        let canonical = *ids.iter().min().expect("non-empty group");
        for &id in ids {
            if id != canonical {
                remap.insert(id, canonical);
            }
        }
    }
    remap
}

/// Invert alias -> canonical into canonical -> sorted duplicate ids.
pub fn duplicated_id_gd_from_remap(remap: &BTreeMap<u32, u32>) -> BTreeMap<u32, Vec<u32>> {
    let mut out: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (&alias, &canonical) in remap {
        if alias != canonical {
            out.entry(canonical).or_default().insert(alias);
        }
    }
    out.into_iter()
        .map(|(canonical, set)| {
            let mut v: Vec<u32> = set.into_iter().collect();
            v.sort_unstable();
            (canonical, v)
        })
        .collect()
}

pub fn apply_build_collapse(
    bitmaps: &mut BitmapStore,
    per_line_bitmaps: &mut PerLineBitmapStore,
    idgd_catalog_builder: &mut IdGdCatalogBuilder,
    compact_cards: &mut [(u32, CompactCardFields)],
) {
    let entries = idgd_catalog_builder.collapse_entries();
    let remap = build_collapse_remap(entries);
    if remap.is_empty() {
        return;
    }

    bitmaps.remap_ids(&remap);
    per_line_bitmaps.remap_ids(&remap);
    idgd_catalog_builder.apply_collapse_remap(&remap);

    for (_, fields) in compact_cards.iter_mut() {
        remap_id_gd_fields(fields, &remap);
    }
}

pub fn remap_cards_bin_file(path: &Path, remap: &BTreeMap<u32, u32>) -> Result<()> {
    if remap.is_empty() {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;

    let len = file
        .metadata()
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    if len % RECORD_SIZE as u64 != 0 {
        return Err(anyhow::anyhow!(
            "cards.bin length {} is not a multiple of {}",
            len,
            RECORD_SIZE
        ));
    }

    let record_count = (len / RECORD_SIZE as u64) as usize;
    let mut buf = [0u8; RECORD_SIZE];

    for idx in 0..record_count {
        let offset = (idx as u64) * RECORD_SIZE as u64;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut buf)?;

        let mut fields = crate::compact::decode_record(&buf);
        remap_id_gd_fields(&mut fields, remap);
        buf = crate::compact::encode_record(&fields);

        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&buf)?;
    }

    Ok(())
}

fn load_bitmap(path: &Path) -> Result<RoaringBitmap> {
    let bytes = fs::read(path)?;
    Ok(RoaringBitmap::deserialize_from(&bytes[..])?)
}

fn write_bitmap(path: &Path, bitmap: &RoaringBitmap) -> Result<u64> {
    let mut bytes = Vec::new();
    bitmap.serialize_into(&mut bytes)?;
    let len = bytes.len() as u64;
    fs::write(path, bytes)?;
    Ok(len)
}

/// Collapse duplicate-text idGd bitmaps on disk after merge; returns merge-time remap.
pub fn collapse_merged_id_gd_on_disk(
    out: &Path,
    meta: &mut BTreeMap<
        u32,
        (
            String,
            BTreeMap<String, LocaleText>,
            crate::idgd_catalog::EffectRegionFlags,
        ),
    >,
    sizes: &mut BTreeMap<u32, u64>,
) -> Result<BTreeMap<u32, u32>> {
    let entries: Vec<CollapseEntry> = meta
        .iter()
        .map(|(&id_gd, (element_type, translations, _))| CollapseEntry {
            id_gd,
            element_type: element_type.clone(),
            translations: translations.clone(),
        })
        .collect();

    let remap = build_collapse_remap(entries);
    if remap.is_empty() {
        return Ok(remap);
    }

    let id_gd_dir = out.join("id_gd");

    for (&alias, &canonical) in &remap {
        if alias == canonical {
            continue;
        }

        let alias_whole = id_gd_dir.join(format!("{alias}.roar"));
        if alias_whole.exists() {
            let alias_bmp = load_bitmap(&alias_whole)?;
            let canonical_whole = id_gd_dir.join(format!("{canonical}.roar"));
            let mut canonical_bmp = if canonical_whole.exists() {
                load_bitmap(&canonical_whole)?
            } else {
                RoaringBitmap::new()
            };
            canonical_bmp |= alias_bmp;
            let bytes = write_bitmap(&canonical_whole, &canonical_bmp)?;
            sizes.insert(canonical, bytes);
            let _ = fs::remove_file(&alias_whole);
        }
        sizes.remove(&alias);

        for line in EffectLine::ALL {
            let alias_path = id_gd_dir.join(format!("{alias}_{}.roar", line.suffix()));
            if !alias_path.exists() {
                continue;
            }
            let alias_bmp = load_bitmap(&alias_path)?;
            let canonical_path = id_gd_dir.join(format!("{canonical}_{}.roar", line.suffix()));
            let mut canonical_bmp = if canonical_path.exists() {
                load_bitmap(&canonical_path)?
            } else {
                RoaringBitmap::new()
            };
            canonical_bmp |= alias_bmp;
            write_bitmap(&canonical_path, &canonical_bmp)?;
            let _ = fs::remove_file(&alias_path);
        }

        if let Some(alias_meta) = meta.remove(&alias) {
            let entry = meta.entry(canonical).or_insert_with(|| {
                (
                    alias_meta.0.clone(),
                    alias_meta.1.clone(),
                    alias_meta.2,
                )
            });
            entry.2 = crate::idgd_catalog::EffectRegionFlags::merge([&entry.2, &alias_meta.2]);
        }
    }

    remap_cards_bin_file(&out.join("cards.bin"), &remap)?;

    Ok(remap)
}

/// Union `duplicated_id_gd` from source catalogs and apply merge-time collapse remap.
pub fn build_merged_duplicated_id_gd(
    source_dirs: &[PathBuf],
    merge_remap: &BTreeMap<u32, u32>,
) -> Result<BTreeMap<u32, Vec<u32>>> {
    fn resolve(id: u32, remap: &BTreeMap<u32, u32>) -> u32 {
        remap.get(&id).copied().unwrap_or(id)
    }

    let mut by_canonical: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();

    for src_dir in source_dirs {
        let path = src_dir.join("idgd_catalog.json");
        if !path.exists() {
            continue;
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let catalog: IdGdCatalog = serde_json::from_str(&text)
            .with_context(|| format!("parse {}", path.display()))?;

        for entry in catalog.entries {
            let canon = resolve(entry.id_gd, merge_remap);
            for dup in entry.duplicated_id_gd {
                let resolved_dup = resolve(dup, merge_remap);
                if resolved_dup != canon {
                    by_canonical.entry(canon).or_default().insert(resolved_dup);
                }
            }
            if entry.id_gd != canon {
                by_canonical.entry(canon).or_default().insert(entry.id_gd);
            }
        }
    }

    for (&alias, &canonical) in merge_remap {
        if alias != canonical {
            by_canonical.entry(canonical).or_default().insert(alias);
        }
    }

    for (canonical, set) in &mut by_canonical {
        set.remove(canonical);
    }
    by_canonical.retain(|_, set| !set.is_empty());

    Ok(by_canonical
        .into_iter()
        .map(|(canonical, set)| {
            let mut v: Vec<u32> = set.into_iter().collect();
            v.sort_unstable();
            (canonical, v)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::LocaleText;
    use crate::idgd_catalog::IdGdCatalogEntry;

    fn locale_text(text: &str) -> BTreeMap<String, LocaleText> {
        BTreeMap::from([(
            "en_US".to_string(),
            LocaleText {
                locale: "en_US".to_string(),
                text: text.to_string(),
            },
        )])
    }

    fn entry(id: u32, element_type: &str, text: &str) -> CollapseEntry {
        CollapseEntry {
            id_gd: id,
            element_type: element_type.to_string(),
            translations: locale_text(text),
        }
    }

    #[test]
    fn build_collapse_remap_groups_same_type_and_text() {
        let remap = build_collapse_remap([
            entry(200, "TRIGGER", "When played"),
            entry(100, "TRIGGER", "When played"),
            entry(300, "TRIGGER", "Other"),
        ]);
        assert_eq!(remap.get(&200), Some(&100));
        assert!(!remap.contains_key(&100));
        assert!(!remap.contains_key(&300));
    }

    #[test]
    fn build_collapse_remap_skips_different_element_type() {
        let remap = build_collapse_remap([
            entry(100, "TRIGGER", "Same text"),
            entry(200, "OUTPUT", "Same text"),
        ]);
        assert!(remap.is_empty());
    }

    #[test]
    fn build_collapse_remap_skips_empty_text() {
        let remap = build_collapse_remap([
            entry(100, "TRIGGER", ""),
            entry(200, "TRIGGER", ""),
        ]);
        assert!(remap.is_empty());
    }

    #[test]
    fn duplicated_id_gd_from_remap_inverts() {
        let mut remap = BTreeMap::new();
        remap.insert(200, 100);
        remap.insert(300, 100);
        let dups = duplicated_id_gd_from_remap(&remap);
        assert_eq!(dups.get(&100).map(Vec::as_slice), Some(&[200, 300][..]));
    }

    #[test]
    fn alias_map_resolves_duplicates() {
        let catalog = IdGdCatalog {
            set: "T".to_string(),
            entries: vec![IdGdCatalogEntry {
                id_gd: 100,
                card_count: 1,
                bitmap_bytes: 1,
                bitmap_file: "100.roar".to_string(),
                element_type: "TRIGGER".to_string(),
                translations: locale_text("x"),
                duplicated_id_gd: vec![200, 300],
                m1: None,
                m2: None,
                m3: None,
                ec: None,
                is_main: true,
                is_echo: false,
            }],
        };
        let map = IdGdAliasMap::from_catalog(&catalog);
        assert_eq!(map.resolve(200), 100);
        assert_eq!(map.resolve(100), 100);
        assert_eq!(map.resolve_ids(&[200, 300, 100]), vec![100]);
    }
}
