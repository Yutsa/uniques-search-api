use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use roaring::RoaringBitmap;

use crate::catalog::Catalog;
use crate::path::parse_card_reference;

/// Union bitmap of catalog bits for the given card references (deduped).
pub fn build_bitmap_from_ref_strs(catalog: &Catalog, refs: &[&str]) -> Result<RoaringBitmap> {
    let mut bits = BTreeSet::new();
    for reference in refs {
        let parsed = parse_card_reference(reference)
            .with_context(|| format!("invalid reference {reference:?}"))?;
        let bit = catalog
            .lookup_bit(&parsed)
            .with_context(|| format!("reference not in catalog: {reference}"))?;
        bits.insert(bit);
    }
    let mut bitmap = RoaringBitmap::new();
    for bit in bits {
        bitmap.insert(bit);
    }
    Ok(bitmap)
}

/// Read a refs file (one reference per line; blank lines and `#` comments ignored).
pub fn build_bitmap_from_refs_file(refs_file: &Path, catalog: &Catalog) -> Result<(usize, RoaringBitmap)> {
    let file = std::fs::File::open(refs_file)
        .with_context(|| format!("open refs file {}", refs_file.display()))?;
    let reader = BufReader::new(file);

    let mut refs_read = 0usize;
    let mut refs: Vec<String> = Vec::new();

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("read line {} of {}", line_no + 1, refs_file.display())
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        refs_read += 1;
        refs.push(trimmed.to_string());
    }

    let ref_slices: Vec<&str> = refs.iter().map(String::as_str).collect();
    let bitmap = build_bitmap_from_ref_strs(catalog, &ref_slices).with_context(|| {
        format!("resolve refs in {}", refs_file.display())
    })?;
    Ok((refs_read, bitmap))
}

/// Union bitmap of catalog bits for the given card references, silently
/// skipping any reference that fails to parse or is not present in the
/// catalog (unlike [`build_bitmap_from_ref_strs`], never errors).
///
/// Intended for user-supplied ad-hoc reference lists (e.g. the `ref` query
/// filter) where an unknown reference should simply not match, not reject
/// the whole request.
pub fn build_bitmap_from_ref_strs_lenient(catalog: &Catalog, refs: &[&str]) -> RoaringBitmap {
    let mut bits = BTreeSet::new();
    for reference in refs {
        let Ok(parsed) = parse_card_reference(reference) else {
            continue;
        };
        let Ok(bit) = catalog.lookup_bit(&parsed) else {
            continue;
        };
        bits.insert(bit);
    }
    let mut bitmap = RoaringBitmap::new();
    for bit in bits {
        bitmap.insert(bit);
    }
    bitmap
}

pub fn validate_bitmap_span(bitmap: &RoaringBitmap, total_bit_span: u32) -> Result<()> {
    if let Some(max_bit) = bitmap.iter().max() {
        if max_bit >= total_bit_span {
            anyhow::bail!(
                "bitmap contains card_index {max_bit} outside manifest total_bit_span {total_bit_span}"
            );
        }
    }
    Ok(())
}
