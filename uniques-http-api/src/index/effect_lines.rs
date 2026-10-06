//! Effect texts of every distinct ability line, computed once at index load.
//!
//! A card stores each ability line as a (trigger, condition, output) idGd triplet. The ~13M lines
//! of the full index use only ~24k distinct triplets, so their raw text and their formatted
//! segments (already serialized to JSON) are computed once per triplet and locale. Building a
//! card's effect fields then copies precomputed text instead of formatting it on every request.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use index_core::card::LocaleText;
use index_core::compact::RECORD_SIZE;
use index_core::idgd_catalog::{IdGdCatalog, IdGdCatalogEntry};
use index_core::keyword_catalog::KeywordCatalog;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use crate::http::api::effect_text::{CardTextParts, join_card_parts};

/// Locale always present in effect texts, used when a part lacks the requested locale.
const FALLBACK_LOCALE: &str = "en_US";

/// One formatted ability line (a list of text segments), serialized once at index load.
#[derive(Debug, Clone)]
pub struct FormattedLineJson(Arc<RawValue>);

impl FormattedLineJson {
    pub fn json(&self) -> &str {
        self.0.get()
    }
}

impl Serialize for FormattedLineJson {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

/// Raw and formatted texts of one ability line in every locale of the index.
#[derive(Debug, Default, Clone)]
struct LineTexts {
    /// Bit `i` set: one of the line's idGds has a translation in `locales[i]`.
    locale_mask: u64,
    /// Per `locales[i]`: raw text and formatted segments, `None` when the line has no text.
    texts: Vec<(Option<String>, Option<FormattedLineJson>)>,
}

/// Raw (`mainEffect`) and formatted (`mainEffectFormatted`) effect fields of one card.
pub struct CardEffectTexts {
    pub raw: BTreeMap<String, String>,
    pub formatted: BTreeMap<String, Vec<FormattedLineJson>>,
}

#[derive(Debug, Default)]
pub struct EffectLineTexts {
    /// Every locale found in the idGd catalog, plus `en_US`, sorted.
    locales: Vec<String>,
    parts: CardTextParts,
    by_line: HashMap<[u16; 3], LineTexts>,
}

impl EffectLineTexts {
    /// Precompute the texts of every ability line found in `cards` (`cards.bin` records).
    pub fn build(catalog: &IdGdCatalog, keywords: &KeywordCatalog, cards: &[u8]) -> Self {
        let mut locales: BTreeSet<String> = catalog
            .entries
            .iter()
            .flat_map(|e| e.translations.keys().cloned())
            .collect();
        locales.insert(FALLBACK_LOCALE.to_string());
        let mut out = Self {
            locales: locales.into_iter().collect(),
            parts: CardTextParts::build(catalog, keywords),
            by_line: HashMap::new(),
        };
        assert!(
            out.locales.len() <= 64,
            "more than 64 locales in the idGd catalog"
        );

        let idgd_by_id = idgd_by_id(catalog);
        // Indexed slices rather than `as_chunks` (Rust 1.88); the Dockerfile builds with 1.86.
        let records =
            (0..cards.len() / RECORD_SIZE).map(|i| &cards[i * RECORD_SIZE..][..RECORD_SIZE]);
        for record in records {
            if record[0] == 0 {
                continue; // padding slot
            }
            for line in 0..4 {
                let ids = std::array::from_fn(|slot| {
                    let at = 6 + (line * 3 + slot) * 2;
                    u16::from_le_bytes([record[at], record[at + 1]])
                });
                if ids != [0; 3] && !out.by_line.contains_key(&ids) {
                    let texts = out.compute(&idgd_by_id, ids);
                    out.by_line.insert(ids, texts);
                }
            }
        }
        out
    }

    /// Number of distinct ability lines precomputed.
    pub fn len(&self) -> usize {
        self.by_line.len()
    }

    /// Effect fields of a card whose ability lines are `lines`, e.g. the three main-effect groups.
    ///
    /// Same output as building each line from the idGd catalog: the locales are `en_US` plus
    /// those of the lines' idGds, lines are joined with two spaces in the raw text, and a locale
    /// is present only when one of its lines has text.
    pub fn card_effect(
        &self,
        idgd_by_id: &BTreeMap<u32, &IdGdCatalogEntry>,
        lines: &[[u16; 3]],
    ) -> CardEffectTexts {
        let lines: Vec<Cow<'_, LineTexts>> = lines
            .iter()
            .filter(|ids| **ids != [0; 3])
            .map(|ids| match self.by_line.get(ids) {
                Some(texts) => Cow::Borrowed(texts),
                // Not in `cards.bin` at load (only test indexes build cards this way).
                None => Cow::Owned(self.compute(idgd_by_id, *ids)),
            })
            .collect();

        let fallback_bit = self
            .locales
            .iter()
            .position(|l| l == FALLBACK_LOCALE)
            .map_or(0, |i| 1u64 << i);
        let mask = lines.iter().fold(fallback_bit, |m, l| m | l.locale_mask);

        let mut raw = BTreeMap::new();
        let mut formatted = BTreeMap::new();
        for (i, locale) in self.locales.iter().enumerate() {
            if mask & (1 << i) == 0 {
                continue;
            }
            let texts: Vec<&str> = lines
                .iter()
                .filter_map(|l| l.texts.get(i)?.0.as_deref())
                .collect();
            if texts.is_empty() {
                continue;
            }
            raw.insert(locale.clone(), texts.join("  "));
            let segments: Vec<FormattedLineJson> = lines
                .iter()
                .filter_map(|l| l.texts.get(i)?.1.clone())
                .collect();
            if !segments.is_empty() {
                formatted.insert(locale.clone(), segments);
            }
        }
        CardEffectTexts { raw, formatted }
    }

    fn compute(&self, idgd_by_id: &BTreeMap<u32, &IdGdCatalogEntry>, ids: [u16; 3]) -> LineTexts {
        let entries: Vec<&IdGdCatalogEntry> = ids
            .iter()
            .filter(|&&id| id != 0)
            .filter_map(|&id| idgd_by_id.get(&(id as u32)).copied())
            .collect();
        let locale_mask = self
            .locales
            .iter()
            .enumerate()
            .filter(|(_, l)| entries.iter().any(|e| e.translations.contains_key(*l)))
            .fold(0u64, |m, (i, _)| m | (1 << i));
        let texts = self
            .locales
            .iter()
            .map(|locale| {
                let raw = raw_line(idgd_by_id, ids, locale);
                let parts = ids
                    .iter()
                    .filter(|&&id| id != 0)
                    .filter_map(|&id| self.parts.get(id as u32, locale));
                let segments = join_card_parts(parts);
                let formatted = (!segments.is_empty()).then(|| {
                    let json = serde_json::value::to_raw_value(&segments)
                        .expect("text segments serialize to JSON");
                    FormattedLineJson(Arc::from(json))
                });
                (raw, formatted)
            })
            .collect();
        LineTexts { locale_mask, texts }
    }
}

pub fn idgd_by_id(catalog: &IdGdCatalog) -> BTreeMap<u32, &IdGdCatalogEntry> {
    catalog.entries.iter().map(|e| (e.id_gd, e)).collect()
}

/// Raw text of one ability line: its parts' texts joined by a space, `None` when all are empty.
fn raw_line(
    idgd_by_id: &BTreeMap<u32, &IdGdCatalogEntry>,
    ids: [u16; 3],
    locale: &str,
) -> Option<String> {
    let parts: Vec<String> = ids
        .iter()
        .filter(|&&id| id != 0)
        .map(|&id| {
            idgd_by_id
                .get(&(id as u32))
                .map(|entry| pick_translation(&entry.translations, locale))
                .unwrap_or_default()
        })
        .filter(|text| !text.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn pick_translation(map: &BTreeMap<String, LocaleText>, locale: &str) -> String {
    if let Some(t) = map.get(locale) {
        return t.text.clone();
    }
    if let Some(t) = map.get(FALLBACK_LOCALE) {
        return t.text.clone();
    }
    map.values()
        .next()
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use index_core::compact::{CompactCardFields, encode_record};

    fn entry(id_gd: u32, texts: &[(&str, &str)]) -> IdGdCatalogEntry {
        IdGdCatalogEntry {
            id_gd,
            card_count: 1,
            bitmap_bytes: 1,
            bitmap_file: format!("{id_gd}.roar"),
            element_type: String::new(),
            translations: texts
                .iter()
                .map(|(locale, text)| {
                    let t = LocaleText {
                        locale: locale.to_string(),
                        text: text.to_string(),
                    };
                    (locale.to_string(), t)
                })
                .collect(),
            m1: None,
            m2: None,
            m3: None,
            ec: None,
            is_main: true,
            is_echo: false,
            duplicated_id_gd: Vec::new(),
        }
    }

    fn catalog() -> IdGdCatalog {
        IdGdCatalog {
            set: "TEST".to_string(),
            entries: vec![
                entry(24, &[("en_US", "{J}"), ("fr_FR", "{J}")]),
                entry(191, &[("en_US", "[]"), ("fr_FR", "[]")]),
                entry(
                    70,
                    &[
                        ("en_US", "It gains [FLEETING]."),
                        ("fr_FR", "Il gagne [FLEETING]."),
                    ],
                ),
                // English only: French falls back to it.
                entry(76, &[("en_US", "Draw a card.")]),
            ],
        }
    }

    fn keywords() -> KeywordCatalog {
        let mut k = KeywordCatalog::default();
        k.insert("FLEETING", "en_US", "Fleeting");
        k.insert("FLEETING", "fr_FR", "Fugace");
        k
    }

    fn cards_bin(main_effect: [[u16; 3]; 3], echo_effect: [u16; 3]) -> Vec<u8> {
        let mut cards = vec![0u8; 2 * RECORD_SIZE]; // slot 0 is padding
        let record = encode_record(&CompactCardFields {
            faction_code: 1,
            main_cost: 1,
            recall_cost: 1,
            mountain_power: 0,
            ocean_power: 0,
            forest_power: 0,
            main_effect,
            echo_effect,
        });
        cards[RECORD_SIZE..].copy_from_slice(&record);
        cards
    }

    fn json_lines(formatted: &BTreeMap<String, Vec<FormattedLineJson>>, locale: &str) -> String {
        let lines: Vec<&str> = formatted[locale]
            .iter()
            .map(FormattedLineJson::json)
            .collect();
        format!("[{}]", lines.join(","))
    }

    #[test]
    fn build_collects_the_distinct_lines_of_cards_bin() {
        let main = [[24, 191, 70], [0, 191, 76], [0, 0, 0]];
        let lines = EffectLineTexts::build(&catalog(), &keywords(), &cards_bin(main, [24, 0, 76]));
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn card_effect_matches_raw_and_formatted_texts() {
        let main = [[24, 191, 70], [0, 191, 76], [0, 0, 0]];
        let catalog = catalog();
        let lines = EffectLineTexts::build(&catalog, &keywords(), &cards_bin(main, [0; 3]));
        let card = lines.card_effect(&idgd_by_id(&catalog), &main);

        assert_eq!(
            card.raw,
            BTreeMap::from([
                (
                    "en_US".to_string(),
                    "{J} [] It gains [FLEETING].  [] Draw a card.".to_string()
                ),
                (
                    "fr_FR".to_string(),
                    "{J} [] Il gagne [FLEETING].  [] Draw a card.".to_string()
                ),
            ])
        );
        assert_eq!(
            json_lines(&card.formatted, "fr_FR"),
            r#"[[{"text":"{J} Il gagne "},{"text":"Fugace","bold":true},{"text":"."}],[{"text":"Draw a card."}]]"#
        );
    }

    #[test]
    fn card_effect_computes_lines_missing_from_the_cache() {
        let catalog = catalog();
        let lines = EffectLineTexts::build(&catalog, &keywords(), &[]);
        let card = lines.card_effect(&idgd_by_id(&catalog), &[[0, 191, 76]]);
        assert_eq!(card.raw["fr_FR"], "[] Draw a card.");
        assert_eq!(
            json_lines(&card.formatted, "en_US"),
            r#"[[{"text":"Draw a card."}]]"#
        );
    }

    #[test]
    fn card_without_ability_has_no_locales() {
        let catalog = catalog();
        let lines = EffectLineTexts::build(&catalog, &keywords(), &[]);
        let card = lines.card_effect(&idgd_by_id(&catalog), &[[0, 0, 0]]);
        assert!(card.raw.is_empty());
        assert!(card.formatted.is_empty());
    }
}
