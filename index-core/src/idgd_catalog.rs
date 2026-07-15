use crate::bitmap::{BitmapStore, EffectLine, PerLineBitmapStore};
use crate::card::{IdGdOccurrence, LocaleText};
use crate::idgd_collapse::CollapseEntry;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct IdGdCatalog {
    pub set: String,
    pub entries: Vec<IdGdCatalogEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdGdCatalogEntry {
    pub id_gd: u32,
    pub card_count: u64,
    pub bitmap_bytes: u64,
    pub bitmap_file: String,
    pub element_type: String,
    pub translations: BTreeMap<String, LocaleText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m1: Option<BitmapMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m2: Option<BitmapMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m3: Option<BitmapMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec: Option<BitmapMeta>,
    /// `true` if this idGd appeared on a main effect line (m1/m2/m3) at least once.
    #[serde(default)]
    pub is_main: bool,
    /// `true` if this idGd appeared on an echo effect line (ec) at least once.
    #[serde(default)]
    pub is_echo: bool,
    /// Non-canonical idGd values collapsed into this entry (same element type and text).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicated_id_gd: Vec<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectRegionFlags {
    pub is_main: bool,
    pub is_echo: bool,
}

impl EffectRegionFlags {
    pub fn from_seen(seen_main: bool, seen_echo: bool) -> Self {
        Self {
            is_main: seen_main,
            is_echo: seen_echo,
        }
    }

    /// OR-merge flags from multiple source catalogs (per-set or cross-set).
    pub fn merge<'a>(sources: impl IntoIterator<Item = &'a Self>) -> Self {
        let mut out = Self::default();
        for s in sources {
            out.is_main |= s.is_main;
            out.is_echo |= s.is_echo;
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitmapMeta {
    pub card_count: u64,
    pub bitmap_bytes: u64,
    pub bitmap_file: String,
}

#[derive(Debug, Default)]
pub struct IdGdCatalogBuilder {
    entries: BTreeMap<u32, DraftEntry>,
}

#[derive(Debug, Clone)]
struct DraftEntry {
    element_type: String,
    translations: BTreeMap<String, LocaleText>,
    seen_main: bool,
    seen_echo: bool,
    duplicated_id_gd: Vec<u32>,
}

impl IdGdCatalogBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record metadata from the first time this `idGd` is seen across the build.
    pub fn record_first(&mut self, occurrence: &IdGdOccurrence) {
        self.entries
            .entry(occurrence.id_gd)
            .or_insert_with(|| DraftEntry {
                element_type: occurrence.element_type.clone(),
                translations: occurrence.translations.clone(),
                seen_main: false,
                seen_echo: false,
                duplicated_id_gd: Vec::new(),
            });
    }

    /// Record that `id_gd` appeared on an effect line (`m1`..`m3` = main, `ec` = echo).
    pub fn record_effect_line(&mut self, id_gd: u32, line: EffectLine) {
        let entry = self.entries.entry(id_gd).or_insert_with(|| DraftEntry {
            element_type: "UNKNOWN".to_string(),
            translations: BTreeMap::new(),
            seen_main: false,
            seen_echo: false,
            duplicated_id_gd: Vec::new(),
        });
        match line {
            EffectLine::Ec => entry.seen_echo = true,
            _ => entry.seen_main = true,
        }
    }

    pub fn collapse_entries(&self) -> Vec<CollapseEntry> {
        self.entries
            .iter()
            .map(|(&id_gd, d)| CollapseEntry {
                id_gd,
                element_type: d.element_type.clone(),
                translations: d.translations.clone(),
            })
            .collect()
    }

    pub fn apply_collapse_remap(&mut self, remap: &std::collections::BTreeMap<u32, u32>) {
        if remap.is_empty() {
            return;
        }
        let duplicated = crate::idgd_collapse::duplicated_id_gd_from_remap(remap);

        for alias in remap.keys() {
            self.entries.remove(alias);
        }

        for (canonical, dups) in duplicated {
            if let Some(entry) = self.entries.get_mut(&canonical) {
                entry.duplicated_id_gd = dups;
            }
        }
    }

    pub fn build(
        self,
        set: &str,
        bitmaps: &BitmapStore,
        bitmap_bytes: &BTreeMap<u32, u64>,
        per_line_bitmaps: &PerLineBitmapStore,
        per_line_bitmap_bytes: &BTreeMap<(u32, EffectLine), u64>,
    ) -> IdGdCatalog {
        let mut entries = Vec::with_capacity(bitmaps.len());
        for (&id_gd, bitmap) in bitmaps.iter() {
            let draft = self.entries.get(&id_gd);
            let (element_type, translations, flags, duplicated_id_gd) = match draft {
                Some(d) => (
                    d.element_type.clone(),
                    d.translations.clone(),
                    EffectRegionFlags::from_seen(d.seen_main, d.seen_echo),
                    d.duplicated_id_gd.clone(),
                ),
                None => (
                    "UNKNOWN".to_string(),
                    BTreeMap::new(),
                    EffectRegionFlags::default(),
                    Vec::new(),
                ),
            };

            let line_meta = |line: EffectLine| -> Option<BitmapMeta> {
                let bmp = per_line_bitmaps.get(id_gd, line)?;
                if bmp.is_empty() {
                    return None;
                }
                let bytes = per_line_bitmap_bytes
                    .get(&(id_gd, line))
                    .copied()
                    .unwrap_or(0);
                Some(BitmapMeta {
                    card_count: bmp.len(),
                    bitmap_bytes: bytes,
                    bitmap_file: format!("{id_gd}_{}.roar", line.suffix()),
                })
            };

            entries.push(IdGdCatalogEntry {
                id_gd,
                card_count: bitmap.len(),
                bitmap_bytes: bitmap_bytes.get(&id_gd).copied().unwrap_or(0),
                bitmap_file: format!("{id_gd}.roar"),
                element_type,
                translations,
                m1: line_meta(EffectLine::M1),
                m2: line_meta(EffectLine::M2),
                m3: line_meta(EffectLine::M3),
                ec: line_meta(EffectLine::Ec),
                is_main: flags.is_main,
                is_echo: flags.is_echo,
                duplicated_id_gd,
            });
        }
        IdGdCatalog {
            set: set.to_string(),
            entries,
        }
    }

    pub fn save(catalog: &IdGdCatalog, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(catalog)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_region_flags_from_seen_main_only() {
        let f = EffectRegionFlags::from_seen(true, false);
        assert!(f.is_main);
        assert!(!f.is_echo);
    }

    #[test]
    fn effect_region_flags_from_seen_echo_only() {
        let f = EffectRegionFlags::from_seen(false, true);
        assert!(!f.is_main);
        assert!(f.is_echo);
    }

    #[test]
    fn effect_region_flags_from_seen_both() {
        let f = EffectRegionFlags::from_seen(true, true);
        assert!(f.is_main);
        assert!(f.is_echo);
    }

    #[test]
    fn effect_region_flags_merge_uses_or() {
        let a = EffectRegionFlags {
            is_main: true,
            is_echo: false,
        };
        let b = EffectRegionFlags {
            is_main: false,
            is_echo: true,
        };
        let merged = EffectRegionFlags::merge([&a, &b]);
        assert!(merged.is_main);
        assert!(merged.is_echo);
    }
}
