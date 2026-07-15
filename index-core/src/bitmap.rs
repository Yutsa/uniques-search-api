use anyhow::Context;
use anyhow::Result;
use roaring::RoaringBitmap;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub struct BitmapStore {
    map: BTreeMap<u32, RoaringBitmap>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectLine {
    M1,
    M2,
    M3,
    Ec,
}

impl EffectLine {
    pub const ALL: [EffectLine; 4] = [EffectLine::M1, EffectLine::M2, EffectLine::M3, EffectLine::Ec];

    pub fn suffix(self) -> &'static str {
        match self {
            EffectLine::M1 => "m1",
            EffectLine::M2 => "m2",
            EffectLine::M3 => "m3",
            EffectLine::Ec => "ec",
        }
    }
}

pub struct PerLineBitmapStore {
    map: BTreeMap<(u32, EffectLine), RoaringBitmap>,
}

impl BitmapStore {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, id_gd: u32, card_index: u32) {
        self.map
            .entry(id_gd)
            .or_default()
            .insert(card_index);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &RoaringBitmap)> {
        self.map.iter()
    }

    /// OR-merge bitmaps for aliased ids into their canonical id; drop alias keys.
    pub fn remap_ids(&mut self, remap: &std::collections::BTreeMap<u32, u32>) {
        if remap.is_empty() {
            return;
        }
        let old = std::mem::take(&mut self.map);
        for (id, bitmap) in old {
            let canonical = remap.get(&id).copied().unwrap_or(id);
            *self.map.entry(canonical).or_default() |= bitmap;
        }
    }

    /// Write bitmap files; returns serialized byte size per `idGd`.
    pub fn write_dir(&self, dir: &Path) -> Result<BTreeMap<u32, u64>> {
        fs::create_dir_all(dir)?;
        let mut sizes = BTreeMap::new();
        for (id_gd, bitmap) in &self.map {
            let path = dir.join(format!("{id_gd}.roar"));
            let mut bytes = Vec::new();
            bitmap
                .serialize_into(&mut bytes)
                .with_context(|| format!("serialize idGd {id_gd}"))?;
            let size = bytes.len() as u64;
            fs::write(&path, bytes)?;
            sizes.insert(*id_gd, size);
        }
        Ok(sizes)
    }

    pub fn load(id_gd: u32, path: &Path) -> Result<RoaringBitmap> {
        let bytes = fs::read(path)?;
        RoaringBitmap::deserialize_from(&bytes[..])
            .with_context(|| format!("deserialize idGd {id_gd} from {}", path.display()))
    }
}

impl Default for BitmapStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PerLineBitmapStore {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, id_gd: u32, line: EffectLine, card_index: u32) {
        self.map
            .entry((id_gd, line))
            .or_default()
            .insert(card_index);
    }

    pub fn get(&self, id_gd: u32, line: EffectLine) -> Option<&RoaringBitmap> {
        self.map.get(&(id_gd, line))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&(u32, EffectLine), &RoaringBitmap)> {
        self.map.iter()
    }

    /// OR-merge per-line bitmaps for aliased ids into their canonical id; drop alias keys.
    pub fn remap_ids(&mut self, remap: &std::collections::BTreeMap<u32, u32>) {
        if remap.is_empty() {
            return;
        }
        let old = std::mem::take(&mut self.map);
        for ((id, line), bitmap) in old {
            let canonical = remap.get(&id).copied().unwrap_or(id);
            *self.map.entry((canonical, line)).or_default() |= bitmap;
        }
    }

    /// Write bitmap files; returns serialized byte size per `(idGd, EffectLine)`.
    ///
    /// Empty bitmaps are omitted from disk and from the returned sizes map.
    pub fn write_dir(&self, dir: &Path) -> Result<BTreeMap<(u32, EffectLine), u64>> {
        fs::create_dir_all(dir)?;
        let mut sizes = BTreeMap::new();
        for ((id_gd, line), bitmap) in &self.map {
            if bitmap.is_empty() {
                continue;
            }
            let path = dir.join(format!("{id_gd}_{}.roar", line.suffix()));
            let mut bytes = Vec::new();
            bitmap
                .serialize_into(&mut bytes)
                .with_context(|| format!("serialize idGd {id_gd} line {}", line.suffix()))?;
            let size = bytes.len() as u64;
            fs::write(&path, bytes)?;
            sizes.insert((*id_gd, *line), size);
        }
        Ok(sizes)
    }
}

impl Default for PerLineBitmapStore {
    fn default() -> Self {
        Self::new()
    }
}
