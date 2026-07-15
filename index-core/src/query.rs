use crate::bitmap::{BitmapStore, EffectLine};
use crate::catalog::Catalog;
use crate::compact::CompactCardView;
use crate::idgd_catalog::IdGdCatalog;
use crate::idgd_collapse::IdGdAliasMap;
use crate::path::parse_card_reference;
use anyhow::{bail, Context, Result};
use roaring::RoaringBitmap;
use std::collections::BTreeMap;
use std::path::Path;

pub struct QueryRow {
    pub card_index: u32,
    pub reference: String,
    pub hand: u8,
    pub reserve: u8,
    pub m: u8,
    pub o: u8,
    pub f: u8,
    pub main_effect: String,
    pub echo_effect: String,
}

pub struct QueryResult {
    pub cardinality: u64,
    pub rows: Vec<QueryRow>,
}

pub struct EffectCard {
    pub reference: String,
    pub hand: u8,
    pub reserve: u8,
    pub m: u8,
    pub o: u8,
    pub f: u8,
    pub effect_lines: Vec<String>,
}

pub struct EffectQueryResult {
    pub cardinality: u64,
    pub recap_lines: Vec<String>,
    pub cards: Vec<EffectCard>,
}

/// Buckets of idGd values grouped by element type (TRIGGER / CONDITION / OUTPUT).
#[derive(Debug, Clone, Default)]
pub struct IdGdQueryBuckets {
    pub triggers: Vec<u32>,
    pub conditions: Vec<u32>,
    pub outputs: Vec<u32>,
}

pub fn query_id_gd(
    index_dir: &Path,
    set: &str,
    id_gd: u32,
    list_limit: Option<usize>,
    whole_card: bool,
) -> Result<QueryResult> {
    query_id_gds(index_dir, set, &[id_gd], list_limit, whole_card)
}

pub fn query_id_gd_effect_text(
    index_dir: &Path,
    set: &str,
    id_gd: u32,
    list_limit: Option<usize>,
    locale: &str,
    whole_card: bool,
) -> Result<EffectQueryResult> {
    query_id_gds_effect_text(index_dir, set, &[id_gd], list_limit, locale, whole_card)
}

/// Look up a single card by reference and return translated effect text.
pub fn query_refid_effect_text(
    index_dir: &Path,
    set: &str,
    refid: &str,
    locale: &str,
) -> Result<EffectCard> {
    let parsed = parse_card_reference(refid)?;
    let reference = parsed.reference();

    let set_dir = index_dir.join(set);
    let catalog = Catalog::load(&set_dir.join("catalog.json"))?;
    let bit = catalog.lookup_bit(&parsed)?;

    let cards_bin_path = set_dir.join("cards.bin");
    let cards_data = std::fs::read(&cards_bin_path)
        .with_context(|| format!("read {}", cards_bin_path.display()))?;

    let view = CompactCardView::from_data(&cards_data, bit)
        .with_context(|| format!("cards.bin has no record at bit {bit}"))?;

    if view.faction_code() == 0 && record_tail_nonzero(view.as_bytes()) {
        bail!("corrupt zero-faction record at {reference}");
    }
    if view.faction_code() == 0 {
        bail!("card not indexed at {reference}");
    }

    let idgd_catalog_path = set_dir.join("idgd_catalog.json");
    let idgd_catalog_text = std::fs::read_to_string(&idgd_catalog_path)
        .with_context(|| format!("read {}", idgd_catalog_path.display()))?;
    let idgd_catalog: IdGdCatalog = serde_json::from_str(&idgd_catalog_text)
        .with_context(|| format!("parse {}", idgd_catalog_path.display()))?;
    let text_by_id = load_translation_map(&idgd_catalog, locale);

    let decoded = catalog.decode_bit(bit)?;
    effect_card_from_view(&decoded.reference, &view, &text_by_id)
}

pub fn query_id_gds(
    index_dir: &Path,
    set: &str,
    id_gds: &[u32],
    list_limit: Option<usize>,
    whole_card: bool,
) -> Result<QueryResult> {
    let (bitmap, _recap, _texts) =
        build_multi_idgd_query(index_dir, set, id_gds, None, whole_card)?;

    let cardinality = bitmap.len();
    let mut rows = Vec::new();

    let Some(limit) = list_limit else {
        return Ok(QueryResult { cardinality, rows });
    };
    if limit == 0 {
        return Ok(QueryResult { cardinality, rows });
    }

    let set_dir = index_dir.join(set);
    let catalog = Catalog::load(&set_dir.join("catalog.json"))?;
    let cards_bin_path = set_dir.join("cards.bin");
    let cards_data = std::fs::read(&cards_bin_path)
        .with_context(|| format!("read {}", cards_bin_path.display()))?;

    for bit in bitmap.iter().take(limit) {
        let decoded = catalog.decode_bit(bit)?;
        let view = match CompactCardView::from_data(&cards_data, bit) {
            Some(v) => v,
            None => continue,
        };

        let hand = view.main_cost();
        let reserve = view.recall_cost();
        let m = view.mountain_power();
        let o = view.ocean_power();
        let f = view.forest_power();

        let mut main_parts = Vec::new();
        for g in 0..3 {
            let [t, c, e] = view.main_effect_group(g);
            if t == 0 && c == 0 && e == 0 {
                continue;
            }
            main_parts.push(format!("{t}_{c}_{e}"));
        }
        let main_effect = main_parts.join(", ");

        let [et, ec, ee] = view.echo_effect();
        let echo_effect = if et == 0 && ec == 0 && ee == 0 {
            String::new()
        } else {
            format!("{et}_{ec}_{ee}")
        };

        rows.push(QueryRow {
            card_index: bit,
            reference: decoded.reference,
            hand,
            reserve,
            m,
            o,
            f,
            main_effect,
            echo_effect,
        });
    }

    Ok(QueryResult { cardinality, rows })
}

pub fn query_id_gds_effect_text(
    index_dir: &Path,
    set: &str,
    id_gds: &[u32],
    list_limit: Option<usize>,
    locale: &str,
    whole_card: bool,
) -> Result<EffectQueryResult> {
    let (bitmap, recap_lines, text_by_id) =
        build_multi_idgd_query(index_dir, set, id_gds, Some(locale), whole_card)?;

    let cardinality = bitmap.len();
    let mut cards = Vec::new();

    let Some(limit) = list_limit else {
        return Ok(EffectQueryResult {
            cardinality,
            recap_lines,
            cards,
        });
    };
    if limit == 0 {
        return Ok(EffectQueryResult {
            cardinality,
            recap_lines,
            cards,
        });
    }

    let set_dir = index_dir.join(set);
    let catalog = Catalog::load(&set_dir.join("catalog.json"))?;
    let cards_bin_path = set_dir.join("cards.bin");
    let cards_data = std::fs::read(&cards_bin_path)
        .with_context(|| format!("read {}", cards_bin_path.display()))?;

    for bit in bitmap.iter().take(limit) {
        let decoded = catalog.decode_bit(bit)?;
        let view = match CompactCardView::from_data(&cards_data, bit) {
            Some(v) => v,
            None => continue,
        };
        cards.push(effect_card_from_view(&decoded.reference, &view, &text_by_id)?);
    }

    Ok(EffectQueryResult {
        cardinality,
        recap_lines,
        cards,
    })
}

/// Execute a multi-idGd query against on-disk bitmaps.
pub fn execute_idgd_query(
    id_gd_dir: &Path,
    buckets: &IdGdQueryBuckets,
    whole_card: bool,
) -> Result<RoaringBitmap> {
    if whole_card {
        execute_whole_card_query(id_gd_dir, buckets)
    } else {
        execute_per_line_query(id_gd_dir, buckets)
    }
}

/// Execute using preloaded per-line bitmaps (for bench-query).
pub fn execute_idgd_query_preloaded(
    per_line: &BTreeMap<(u32, EffectLine), RoaringBitmap>,
    whole_card: &BTreeMap<u32, RoaringBitmap>,
    buckets: &IdGdQueryBuckets,
    whole_card_mode: bool,
) -> RoaringBitmap {
    if whole_card_mode {
        execute_whole_card_query_preloaded(whole_card, buckets)
    } else {
        execute_per_line_query_preloaded(per_line, buckets)
    }
}

fn build_multi_idgd_query(
    index_dir: &Path,
    set: &str,
    id_gds: &[u32],
    locale: Option<&str>,
    whole_card: bool,
) -> Result<(RoaringBitmap, Vec<String>, BTreeMap<u32, String>)> {
    anyhow::ensure!(!id_gds.is_empty(), "at least one --id-gd must be provided");

    let set_dir = index_dir.join(set);
    let idgd_catalog_path = set_dir.join("idgd_catalog.json");
    let idgd_catalog_text = std::fs::read_to_string(&idgd_catalog_path)
        .with_context(|| format!("read {}", idgd_catalog_path.display()))?;
    let idgd_catalog: IdGdCatalog = serde_json::from_str(&idgd_catalog_text)
        .with_context(|| format!("parse {}", idgd_catalog_path.display()))?;

    let (buckets, text_by_id) =
        bucket_id_gds(id_gds, &idgd_catalog, &idgd_catalog_path, locale)?;

    let id_gd_dir = set_dir.join("id_gd");
    let bitmap = execute_idgd_query(&id_gd_dir, &buckets, whole_card)?;

    if !whole_card && bitmap.is_empty() {
        warn_if_missing_per_line_bitmaps(&id_gd_dir, &buckets);
    }

    let recap_lines = build_recap_lines(locale, &buckets, &text_by_id);

    Ok((bitmap, recap_lines, text_by_id))
}

fn bucket_id_gds(
    id_gds: &[u32],
    idgd_catalog: &IdGdCatalog,
    idgd_catalog_path: &Path,
    locale: Option<&str>,
) -> Result<(IdGdQueryBuckets, BTreeMap<u32, String>)> {
    #[derive(Clone)]
    struct MetaEntry {
        element_type: String,
        translations: BTreeMap<String, crate::card::LocaleText>,
    }

    let mut meta_by_id: BTreeMap<u32, MetaEntry> = BTreeMap::new();
    for e in &idgd_catalog.entries {
        meta_by_id.insert(
            e.id_gd,
            MetaEntry {
                element_type: e.element_type.clone(),
                translations: e.translations.clone(),
            },
        );
    }

    let alias_map = IdGdAliasMap::from_catalog(idgd_catalog);

    let mut buckets = IdGdQueryBuckets::default();

    for &id in id_gds {
        let resolved = alias_map.resolve(id);
        let meta = meta_by_id
            .get(&resolved)
            .with_context(|| format!("idGd {id} not found in {}", idgd_catalog_path.display()))?;
        match meta.element_type.as_str() {
            "TRIGGER" => {
                if !buckets.triggers.contains(&resolved) {
                    buckets.triggers.push(resolved);
                }
            }
            "CONDITION" => {
                if !buckets.conditions.contains(&resolved) {
                    buckets.conditions.push(resolved);
                }
            }
            "OUTPUT" => {
                if !buckets.outputs.contains(&resolved) {
                    buckets.outputs.push(resolved);
                }
            }
            other => {
                anyhow::bail!("idGd {id} has unsupported element_type={other}");
            }
        }
    }

    anyhow::ensure!(
        !buckets.triggers.is_empty() || !buckets.conditions.is_empty() || !buckets.outputs.is_empty(),
        "no valid idGd values were provided"
    );

    let mut text_by_id: BTreeMap<u32, String> = BTreeMap::new();
    if let Some(locale) = locale {
        for (&id, meta) in &meta_by_id {
            let t = pick_translation(&meta.translations, locale);
            if !t.is_empty() {
                text_by_id.insert(id, t);
            }
        }
    }

    Ok((buckets, text_by_id))
}

fn build_recap_lines(
    locale: Option<&str>,
    buckets: &IdGdQueryBuckets,
    text_by_id: &BTreeMap<u32, String>,
) -> Vec<String> {
    let mut recap_lines = Vec::new();
    if locale.is_some() {
        recap_lines.push("Searching for cards matching:".to_string());
        if !buckets.triggers.is_empty() {
            recap_lines.push("Triggers is one of :".to_string());
            for id in &buckets.triggers {
                let t = text_by_id.get(id).cloned().unwrap_or_default();
                recap_lines.push(format!("- \"{t}\""));
            }
        }
        if !buckets.conditions.is_empty() {
            recap_lines.push("Condition is one of :".to_string());
            for id in &buckets.conditions {
                let t = text_by_id.get(id).cloned().unwrap_or_default();
                recap_lines.push(format!("- \"{t}\""));
            }
        }
        if !buckets.outputs.is_empty() {
            recap_lines.push("Output is one of :".to_string());
            for id in &buckets.outputs {
                let t = text_by_id.get(id).cloned().unwrap_or_default();
                recap_lines.push(format!("- \"{t}\""));
            }
        }
    }
    recap_lines
}

fn execute_whole_card_query(id_gd_dir: &Path, buckets: &IdGdQueryBuckets) -> Result<RoaringBitmap> {
    let mut groups: Vec<RoaringBitmap> = Vec::new();
    if !buckets.triggers.is_empty() {
        groups.push(union_whole_card_bitmaps(id_gd_dir, &buckets.triggers)?);
    }
    if !buckets.conditions.is_empty() {
        groups.push(union_whole_card_bitmaps(id_gd_dir, &buckets.conditions)?);
    }
    if !buckets.outputs.is_empty() {
        groups.push(union_whole_card_bitmaps(id_gd_dir, &buckets.outputs)?);
    }
    intersect_groups(groups)
}

fn execute_per_line_query(id_gd_dir: &Path, buckets: &IdGdQueryBuckets) -> Result<RoaringBitmap> {
    let mut result = RoaringBitmap::new();
    for line in EffectLine::ALL {
        if let Some(line_match) = intersect_buckets_on_line(id_gd_dir, buckets, line)? {
            result |= line_match;
        }
    }
    Ok(result)
}

fn execute_whole_card_query_preloaded(
    whole_card: &BTreeMap<u32, RoaringBitmap>,
    buckets: &IdGdQueryBuckets,
) -> RoaringBitmap {
    let mut groups: Vec<RoaringBitmap> = Vec::new();
    if !buckets.triggers.is_empty() {
        groups.push(union_whole_card_bitmaps_preloaded(whole_card, &buckets.triggers));
    }
    if !buckets.conditions.is_empty() {
        groups.push(union_whole_card_bitmaps_preloaded(
            whole_card,
            &buckets.conditions,
        ));
    }
    if !buckets.outputs.is_empty() {
        groups.push(union_whole_card_bitmaps_preloaded(whole_card, &buckets.outputs));
    }
    intersect_groups(groups).unwrap_or_default()
}

fn execute_per_line_query_preloaded(
    per_line: &BTreeMap<(u32, EffectLine), RoaringBitmap>,
    buckets: &IdGdQueryBuckets,
) -> RoaringBitmap {
    let mut result = RoaringBitmap::new();
    for line in EffectLine::ALL {
        if let Some(line_match) = intersect_buckets_on_line_preloaded(per_line, buckets, line) {
            result |= line_match;
        }
    }
    result
}

/// Within one effect line: (union triggers) ∩ (union conditions) ∩ (union outputs),
/// skipping empty query buckets.
fn intersect_buckets_on_line(
    id_gd_dir: &Path,
    buckets: &IdGdQueryBuckets,
    line: EffectLine,
) -> Result<Option<RoaringBitmap>> {
    let mut groups: Vec<RoaringBitmap> = Vec::new();
    if !buckets.triggers.is_empty() {
        groups.push(union_per_line_bitmaps(id_gd_dir, &buckets.triggers, line)?);
    }
    if !buckets.conditions.is_empty() {
        groups.push(union_per_line_bitmaps(id_gd_dir, &buckets.conditions, line)?);
    }
    if !buckets.outputs.is_empty() {
        groups.push(union_per_line_bitmaps(id_gd_dir, &buckets.outputs, line)?);
    }
    intersect_groups(groups).map(Some)
}

fn intersect_buckets_on_line_preloaded(
    per_line: &BTreeMap<(u32, EffectLine), RoaringBitmap>,
    buckets: &IdGdQueryBuckets,
    line: EffectLine,
) -> Option<RoaringBitmap> {
    let mut groups: Vec<RoaringBitmap> = Vec::new();
    if !buckets.triggers.is_empty() {
        groups.push(union_per_line_bitmaps_preloaded(
            per_line,
            &buckets.triggers,
            line,
        ));
    }
    if !buckets.conditions.is_empty() {
        groups.push(union_per_line_bitmaps_preloaded(
            per_line,
            &buckets.conditions,
            line,
        ));
    }
    if !buckets.outputs.is_empty() {
        groups.push(union_per_line_bitmaps_preloaded(
            per_line,
            &buckets.outputs,
            line,
        ));
    }
    intersect_groups(groups).ok()
}

fn intersect_groups(groups: Vec<RoaringBitmap>) -> Result<RoaringBitmap> {
    let mut it = groups.into_iter();
    let mut bitmap = match it.next() {
        Some(b) => b,
        None => return Ok(RoaringBitmap::new()),
    };
    for g in it {
        bitmap &= g;
    }
    Ok(bitmap)
}

fn union_whole_card_bitmaps(id_gd_dir: &Path, ids: &[u32]) -> Result<RoaringBitmap> {
    let mut merged = RoaringBitmap::new();
    for &id in ids {
        let bitmap_path = id_gd_dir.join(format!("{id}.roar"));
        let bitmap = BitmapStore::load(id, &bitmap_path)
            .with_context(|| format!("idGd {id} not found at {}", bitmap_path.display()))?;
        merged |= bitmap;
    }
    Ok(merged)
}

fn union_whole_card_bitmaps_preloaded(
    whole_card: &BTreeMap<u32, RoaringBitmap>,
    ids: &[u32],
) -> RoaringBitmap {
    let mut merged = RoaringBitmap::new();
    for &id in ids {
        if let Some(bmp) = whole_card.get(&id) {
            merged |= bmp;
        }
    }
    merged
}

fn union_per_line_bitmaps(id_gd_dir: &Path, ids: &[u32], line: EffectLine) -> Result<RoaringBitmap> {
    let mut merged = RoaringBitmap::new();
    for &id in ids {
        let bitmap_path = id_gd_dir.join(format!("{id}_{}.roar", line.suffix()));
        if let Some(bitmap) = try_load_bitmap(&bitmap_path)? {
            merged |= bitmap;
        }
    }
    Ok(merged)
}

fn union_per_line_bitmaps_preloaded(
    per_line: &BTreeMap<(u32, EffectLine), RoaringBitmap>,
    ids: &[u32],
    line: EffectLine,
) -> RoaringBitmap {
    let mut merged = RoaringBitmap::new();
    for &id in ids {
        if let Some(bmp) = per_line.get(&(id, line)) {
            merged |= bmp;
        }
    }
    merged
}

fn try_load_bitmap(path: &Path) -> Result<Option<RoaringBitmap>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    let bmp = RoaringBitmap::deserialize_from(&bytes[..])
        .with_context(|| format!("deserialize bitmap from {}", path.display()))?;
    if bmp.is_empty() {
        return Ok(None);
    }
    Ok(Some(bmp))
}

/// If no `{id}_m*.roar` / `{id}_ec.roar` exist for the queried ids, the index likely predates
/// per-line bitmaps or needs a rebuild.
fn warn_if_missing_per_line_bitmaps(id_gd_dir: &Path, buckets: &IdGdQueryBuckets) {
    let ids: Vec<u32> = buckets
        .triggers
        .iter()
        .chain(buckets.conditions.iter())
        .chain(buckets.outputs.iter())
        .copied()
        .collect();

    let any_file = ids.iter().any(|&id| {
        EffectLine::ALL.iter().any(|line| {
            id_gd_dir
                .join(format!("{id}_{}.roar", line.suffix()))
                .is_file()
        })
    });

    if !any_file {
        eprintln!(
            "warning: no per-line idGd bitmaps found under {} for the queried ids.",
            id_gd_dir.display()
        );
        eprintln!(
            "         Rebuild the index (cli-indexer build) or pass --whole-card to use combined {{id}}.roar files."
        );
    }
}

fn load_translation_map(idgd_catalog: &IdGdCatalog, locale: &str) -> BTreeMap<u32, String> {
    let mut text_by_id = BTreeMap::new();
    for entry in &idgd_catalog.entries {
        let t = pick_translation(&entry.translations, locale);
        if !t.is_empty() {
            text_by_id.insert(entry.id_gd, t);
        }
    }
    text_by_id
}

fn effect_card_from_view(
    reference: &str,
    view: &CompactCardView<'_>,
    text_by_id: &BTreeMap<u32, String>,
) -> Result<EffectCard> {
    let mut effect_lines = Vec::new();

    for g in 0..3 {
        let [t, c, e] = view.main_effect_group(g);
        if let Some(line) = build_effect_line(t, c, e, text_by_id) {
            effect_lines.push(line);
        }
    }

    let [t, c, e] = view.echo_effect();
    if let Some(line) = build_effect_line(t, c, e, text_by_id) {
        effect_lines.push(line);
    }

    Ok(EffectCard {
        reference: reference.to_string(),
        hand: view.main_cost(),
        reserve: view.recall_cost(),
        m: view.mountain_power(),
        o: view.ocean_power(),
        f: view.forest_power(),
        effect_lines,
    })
}

fn record_tail_nonzero(record: &[u8; crate::compact::RECORD_SIZE]) -> bool {
    record[1..].iter().any(|b| *b != 0)
}

fn pick_translation(map: &BTreeMap<String, crate::card::LocaleText>, locale: &str) -> String {
    crate::card::translation_text(map, locale)
}

fn build_effect_line(t: u16, c: u16, e: u16, text_by_id: &BTreeMap<u32, String>) -> Option<String> {
    if t == 0 && c == 0 && e == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if t != 0 {
        parts.push(text_by_id.get(&(t as u32)).cloned().unwrap_or_default());
    }
    if c != 0 {
        parts.push(text_by_id.get(&(c as u32)).cloned().unwrap_or_default());
    }
    if e != 0 {
        parts.push(text_by_id.get(&(e as u32)).cloned().unwrap_or_default());
    }
    let line = parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::LocaleText;
    use crate::idgd_catalog::{IdGdCatalog, IdGdCatalogEntry};
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    fn write_bitmap(path: &Path, values: &[u32]) -> Result<()> {
        let mut bmp = RoaringBitmap::new();
        for &v in values {
            bmp.insert(v);
        }
        let mut bytes = Vec::new();
        bmp.serialize_into(&mut bytes)?;
        fs::write(path, bytes)?;
        Ok(())
    }

    fn lt(locale: &str, text: &str) -> LocaleText {
        LocaleText {
            locale: locale.to_string(),
            text: text.to_string(),
        }
    }

    fn write_idgd_catalog(set_dir: &Path, entries: Vec<(u32, &str, &str)>) -> Result<()> {
        let mut cat_entries = Vec::new();
        for (id, element_type, text) in entries {
            let mut translations: BTreeMap<String, LocaleText> = BTreeMap::new();
            translations.insert("en_US".to_string(), lt("en_US", text));
            cat_entries.push(IdGdCatalogEntry {
                id_gd: id,
                card_count: 0,
                bitmap_bytes: 0,
                bitmap_file: format!("{id}.roar"),
                element_type: element_type.to_string(),
                translations,
                m1: None,
                m2: None,
                m3: None,
                ec: None,
                is_main: true,
                is_echo: false,
                duplicated_id_gd: Vec::new(),
            });
        }
        let cat = IdGdCatalog {
            set: "TEST".to_string(),
            entries: cat_entries,
        };
        let text = serde_json::to_string_pretty(&cat)?;
        fs::write(set_dir.join("idgd_catalog.json"), text)?;
        Ok(())
    }

    fn setup_index() -> Result<(TempDir, std::path::PathBuf)> {
        let td = TempDir::new()?;
        let set_dir = td.path().join("SET");
        fs::create_dir_all(set_dir.join("id_gd"))?;
        Ok((td, set_dir))
    }

    fn id_gd_dir(set_dir: &Path) -> std::path::PathBuf {
        set_dir.join("id_gd")
    }

    #[test]
    fn resolves_duplicated_id_gd_alias() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        let catalog = IdGdCatalog {
            set: "TEST".to_string(),
            entries: vec![IdGdCatalogEntry {
                id_gd: 100,
                card_count: 1,
                bitmap_bytes: 0,
                bitmap_file: "100.roar".to_string(),
                element_type: "TRIGGER".to_string(),
                translations: {
                    let mut m = BTreeMap::new();
                    m.insert("en_US".to_string(), lt("en_US", "When played"));
                    m
                },
                m1: None,
                m2: None,
                m3: None,
                ec: None,
                is_main: true,
                is_echo: false,
                duplicated_id_gd: vec![200],
            }],
        };
        let text = serde_json::to_string_pretty(&catalog)?;
        fs::write(set_dir.join("idgd_catalog.json"), text)?;

        write_bitmap(&set_dir.join("id_gd/100.roar"), &[42])?;

        let (bmp, _recap, _texts) =
            build_multi_idgd_query(set_dir.parent().unwrap(), "SET", &[200], None, true)?;
        assert_eq!(bmp.len(), 1);
        assert!(bmp.contains(42));
        Ok(())
    }

    #[test]
    fn unions_within_bucket_whole_card() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(
            &set_dir,
            vec![(1, "TRIGGER", "{J}"), (2, "TRIGGER", "When I go...")],
        )?;
        write_bitmap(&set_dir.join("id_gd/1.roar"), &[1, 2])?;
        write_bitmap(&set_dir.join("id_gd/2.roar"), &[2, 3])?;

        let (bmp, _recap, _texts) =
            build_multi_idgd_query(set_dir.parent().unwrap(), "SET", &[1, 2], None, true)?;
        assert_eq!(bmp.len(), 3);
        assert!(bmp.contains(1));
        assert!(bmp.contains(2));
        assert!(bmp.contains(3));
        Ok(())
    }

    #[test]
    fn intersects_across_buckets_whole_card() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(
            &set_dir,
            vec![
                (1, "TRIGGER", "T1"),
                (2, "TRIGGER", "T2"),
                (3, "CONDITION", "C3"),
            ],
        )?;
        write_bitmap(&set_dir.join("id_gd/1.roar"), &[1, 2])?;
        write_bitmap(&set_dir.join("id_gd/2.roar"), &[2, 3])?;
        write_bitmap(&set_dir.join("id_gd/3.roar"), &[2, 3])?;

        let (bmp, _recap, _texts) =
            build_multi_idgd_query(set_dir.parent().unwrap(), "SET", &[1, 2, 3], None, true)?;
        assert_eq!(bmp.len(), 2);
        assert!(bmp.contains(2));
        assert!(bmp.contains(3));
        assert!(!bmp.contains(1));
        Ok(())
    }

    #[test]
    fn intersects_across_buckets_same_line_only_by_default() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(
            &set_dir,
            vec![(24, "TRIGGER", "T24"), (191, "CONDITION", "C191")],
        )?;
        // Whole-card matches if id appears anywhere on the card (cross-line OK).
        write_bitmap(&set_dir.join("id_gd/24.roar"), &[10, 20])?;
        write_bitmap(&set_dir.join("id_gd/191.roar"), &[10, 20])?;
        write_bitmap(&set_dir.join("id_gd/24_m1.roar"), &[10, 20])?;
        write_bitmap(&set_dir.join("id_gd/191_m2.roar"), &[10])?;
        write_bitmap(&set_dir.join("id_gd/191_m1.roar"), &[20])?;

        let dir = id_gd_dir(&set_dir);
        let buckets = IdGdQueryBuckets {
            triggers: vec![24],
            conditions: vec![191],
            outputs: vec![],
        };

        let per_line = execute_idgd_query(&dir, &buckets, false)?;
        assert_eq!(per_line.len(), 1);
        assert!(per_line.contains(20));
        assert!(!per_line.contains(10));

        let whole = execute_idgd_query(&dir, &buckets, true)?;
        assert!(whole.contains(10));
        assert!(whole.contains(20));
        Ok(())
    }

    #[test]
    fn per_line_unions_within_bucket_on_one_line() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(
            &set_dir,
            vec![(1, "TRIGGER", "T1"), (2, "TRIGGER", "T2")],
        )?;
        write_bitmap(&set_dir.join("id_gd/1_m1.roar"), &[1, 2])?;
        write_bitmap(&set_dir.join("id_gd/2_m1.roar"), &[2, 3])?;

        let buckets = IdGdQueryBuckets {
            triggers: vec![1, 2],
            conditions: vec![],
            outputs: vec![],
        };
        let bmp = execute_idgd_query(&id_gd_dir(&set_dir), &buckets, false)?;
        assert_eq!(bmp.len(), 3);
        Ok(())
    }

    #[test]
    fn ignores_missing_bucket() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(&set_dir, vec![(10, "OUTPUT", "Draw a card")])?;
        write_bitmap(&set_dir.join("id_gd/10_m1.roar"), &[7, 8, 9])?;

        let (bmp, _recap, _texts) =
            build_multi_idgd_query(set_dir.parent().unwrap(), "SET", &[10], None, false)?;
        assert_eq!(bmp.len(), 3);
        Ok(())
    }

    #[test]
    fn errors_on_unknown_element_type() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(&set_dir, vec![(99, "UNKNOWN", "???")])?;
        write_bitmap(&set_dir.join("id_gd/99_m1.roar"), &[1])?;

        let err = build_multi_idgd_query(set_dir.parent().unwrap(), "SET", &[99], None, false)
            .expect_err("should fail");
        assert!(format!("{err:#}").contains("unsupported element_type"));
        Ok(())
    }

    #[test]
    fn recap_groups_by_bucket_in_input_order() -> Result<()> {
        let (_td, set_dir) = setup_index()?;

        write_idgd_catalog(
            &set_dir,
            vec![
                (1, "TRIGGER", "{J}"),
                (2, "TRIGGER", "When I go to reserve from the Expedition"),
                (3, "CONDITION", "You may pay {1} if you do"),
                (4, "OUTPUT", "Draw a card"),
                (5, "OUTPUT", "You may discard target Permanent"),
            ],
        )?;
        write_bitmap(&set_dir.join("id_gd/1_m1.roar"), &[1])?;
        write_bitmap(&set_dir.join("id_gd/2_m1.roar"), &[1])?;
        write_bitmap(&set_dir.join("id_gd/3_m1.roar"), &[1])?;
        write_bitmap(&set_dir.join("id_gd/4_m1.roar"), &[1])?;
        write_bitmap(&set_dir.join("id_gd/5_m1.roar"), &[1])?;

        let (_bmp, recap, _texts) = build_multi_idgd_query(
            set_dir.parent().unwrap(),
            "SET",
            &[1, 2, 3, 4, 5],
            Some("en_US"),
            false,
        )?;

        let joined = recap.join("\n");
        assert!(joined.contains("Searching for cards matching:"));
        assert!(joined.contains("Triggers is one of :"));
        assert!(joined.contains("- \"{J}\""));
        Ok(())
    }
}
