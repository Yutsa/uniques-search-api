use roaring::RoaringBitmap;

use index_core::bitmap::EffectLine;

use crate::http::api::error::{bad_request, ApiResult};
use crate::index::UniquesIndex;

use super::models::{Part, Region};

pub(crate) const MAIN_LINES: [EffectLine; 3] = [EffectLine::M1, EffectLine::M2, EffectLine::M3];
pub(crate) const SUPPORT_LINES: [EffectLine; 1] = [EffectLine::Ec];

pub(crate) fn parse_editing(editing: &str) -> ApiResult<(Part, Region)> {
    let (part_str, slot_str) = editing.split_once(':').ok_or_else(|| {
        bad_request(format!(
            "invalid editing '{editing}': expected '<part>:<slot>' (e.g. trigger:0)"
        ))
    })?;

    let part = match part_str.trim() {
        "trigger" => Part::Trigger,
        "condition" => Part::Condition,
        "output" => Part::Output,
        other => {
            return Err(bad_request(format!(
                "invalid editing part '{other}': expected 'trigger', 'condition', or 'output'"
            )));
        }
    };

    let region = match slot_str.trim() {
        "support" => Region::Support,
        s => {
            let n = s.parse::<u32>().map_err(|_| {
                bad_request(format!(
                    "invalid editing slot '{s}': expected a main-effect slot index or 'support'"
                ))
            })?;
            Region::Main(n)
        }
    };

    Ok((part, region))
}

/// Returns clones of the two buckets that are *not* the edited part.
pub(crate) fn other_two_buckets(part: Part, t: &[u32], c: &[u32], o: &[u32]) -> (Vec<u32>, Vec<u32>) {
    match part {
        Part::Trigger => (c.to_vec(), o.to_vec()),
        Part::Condition => (t.to_vec(), o.to_vec()),
        Part::Output => (t.to_vec(), c.to_vec()),
    }
}

pub(crate) fn union_on_line(state: &UniquesIndex, line: EffectLine, ids: &[u32]) -> RoaringBitmap {
    let mut out = RoaringBitmap::new();
    for &id in ids {
        let resolved = state.resolve_id_gd(id);
        if let Some(bm) = state.id_gd_per_line().get(&(resolved, line)) {
            out |= bm;
        }
    }
    out
}
