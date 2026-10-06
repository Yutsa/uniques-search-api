//! Readable versions of the raw effect texts, as lists of segments.
//!
//! Raw texts carry Altered markup: `[FLEETING]` keyword codes, `[]` for an empty part, `{H}`-style
//! symbols. The formatted versions print keyword codes as their bold printed name (from the
//! index's `keywords.json`) and tidy spaces. Segments let clients render bold without parsing
//! markup or injecting HTML.

use std::collections::{BTreeMap, BTreeSet};

use index_core::card::LocaleText;
use index_core::idgd_catalog::IdGdCatalog;
use index_core::keyword_catalog::KeywordCatalog;
use serde::Serialize;

/// One run of text; `bold` is omitted from JSON when false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextSegment {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
}

impl TextSegment {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: true,
        }
    }
}

pub type FormattedText = Vec<TextSegment>;

/// Locale used when a text or keyword name is missing in the requested locale.
const FALLBACK_LOCALE: &str = "en_US";

/// Labels for effect parts that are a bare symbol or empty. Card data has no text for these,
/// so they are maintained here.
#[derive(Debug, Clone, Copy)]
enum Label {
    NoCondition,
    PlayedFromHand,
    PlayedFromReserve,
    PlayedFromAnywhere,
    ExhaustMe,
    DiscardMeFromReserve,
}

impl Label {
    fn text(self, locale: &str) -> &'static str {
        let lang = locale.get(..2).unwrap_or(locale);
        match (self, lang) {
            (Label::NoCondition, "fr") => "Sans condition",
            (Label::NoCondition, "de") => "Keine Bedingung",
            (Label::NoCondition, "es") => "Sin condición",
            (Label::NoCondition, "it") => "Nessuna condizione",
            (Label::NoCondition, _) => "No condition",
            (Label::PlayedFromHand, "fr") => "Joué depuis la Main",
            (Label::PlayedFromHand, "de") => "Aus der Hand gespielt",
            (Label::PlayedFromHand, "es") => "Jugado desde la Mano",
            (Label::PlayedFromHand, "it") => "Giocato dalla Mano",
            (Label::PlayedFromHand, _) => "Played from Hand",
            (Label::PlayedFromReserve, "fr") => "Joué depuis la Réserve",
            (Label::PlayedFromReserve, "de") => "Aus der Reserve gespielt",
            (Label::PlayedFromReserve, "es") => "Jugado desde la Reserva",
            (Label::PlayedFromReserve, "it") => "Giocato dalla Riserva",
            (Label::PlayedFromReserve, _) => "Played from Reserve",
            (Label::PlayedFromAnywhere, "fr") => "Joué de partout",
            (Label::PlayedFromAnywhere, "de") => "Von überall gespielt",
            (Label::PlayedFromAnywhere, "es") => "Jugado desde cualquier lugar",
            (Label::PlayedFromAnywhere, "it") => "Giocato da qualsiasi zona",
            (Label::PlayedFromAnywhere, _) => "Played from anywhere",
            (Label::ExhaustMe, "fr") => "Épuisez-moi",
            (Label::ExhaustMe, "de") => "Erschöpfe mich",
            (Label::ExhaustMe, "es") => "Agótame",
            (Label::ExhaustMe, "it") => "Esauriscimi",
            (Label::ExhaustMe, _) => "Exhaust me",
            (Label::DiscardMeFromReserve, "fr") => "Défaussez-moi de la Réserve",
            (Label::DiscardMeFromReserve, "de") => "Lege mich aus der Reserve ab",
            (Label::DiscardMeFromReserve, "es") => "Descártame de la Reserva",
            (Label::DiscardMeFromReserve, "it") => "Scartami dalla Riserva",
            (Label::DiscardMeFromReserve, _) => "Discard me from Reserve",
        }
    }
}

/// Effect part as shown in the trigger / condition / output pickers (`GET /api/v2/effects`):
///
/// - `[]` → "No condition" for a condition, nothing for other parts;
/// - `{H}`, `{R}`, `{J}`, `{T}` alone → "Played from Hand", "Played from Reserve",
///   "Played from anywhere", "Exhaust me"; `{D}` alone (with or without `:`) → "Discard me from
///   Reserve"; a leading `{I}` is dropped;
/// - `[CODE]` keywords → bold printed name;
/// - non-breaking and repeated spaces (also inside keyword names, e.g. `Tough\u{a0}1`) → one
///   space, trailing spaces and `:` removed.
pub fn format_effect_part(
    raw: &str,
    element_type: &str,
    locale: &str,
    keywords: &KeywordCatalog,
) -> FormattedText {
    let spaced = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if spaced == "[]" {
        return match element_type {
            "CONDITION" => vec![TextSegment::plain(Label::NoCondition.text(locale))],
            _ => Vec::new(),
        };
    }
    let text = spaced.trim_end_matches([' ', ':']);
    let label = match text {
        "{H}" => Some(Label::PlayedFromHand),
        "{R}" => Some(Label::PlayedFromReserve),
        "{J}" => Some(Label::PlayedFromAnywhere),
        "{T}" => Some(Label::ExhaustMe),
        "{D}" => Some(Label::DiscardMeFromReserve),
        _ => None,
    };
    if let Some(label) = label {
        return vec![TextSegment::plain(label.text(locale))];
    }
    let text = text.strip_prefix("{I}").unwrap_or(text);
    let segments = keyword_segments(text, locale, keywords)
        .into_iter()
        .map(|seg| TextSegment {
            text: seg
                .text
                .chars()
                .map(|c| if c.is_whitespace() { ' ' } else { c })
                .collect(),
            bold: seg.bold,
        })
        .collect();
    tidy(segments)
}

/// Effect part as printed on a card (`mainEffectFormatted` / `echoEffectFormatted`): `[CODE]`
/// keywords → bold printed name, `[]` dropped, repeated spaces collapsed. Symbols such as `{J}`
/// and non-breaking spaces are kept as printed.
pub fn format_card_part(raw: &str, locale: &str, keywords: &KeywordCatalog) -> FormattedText {
    tidy(keyword_segments(raw, locale, keywords))
}

/// One card ability line: its formatted parts separated by a space, as the raw text joins them.
pub fn join_card_parts<'a>(parts: impl IntoIterator<Item = &'a FormattedText>) -> FormattedText {
    let mut out = Vec::new();
    for part in parts {
        if !out.is_empty() {
            out.push(TextSegment::plain(" "));
        }
        out.extend(part.iter().cloned());
    }
    tidy(out)
}

/// Card-text formatting of every idGd, per locale, computed once at index load.
#[derive(Debug, Default)]
pub struct CardTextParts {
    by_id_gd: BTreeMap<u32, BTreeMap<String, FormattedText>>,
}

impl CardTextParts {
    pub fn build(catalog: &IdGdCatalog, keywords: &KeywordCatalog) -> Self {
        let by_id_gd = catalog
            .entries
            .iter()
            .map(|entry| {
                let per_locale = entry
                    .translations
                    .iter()
                    .map(|(locale, t)| {
                        (locale.clone(), format_card_part(&t.text, locale, keywords))
                    })
                    .collect();
                (entry.id_gd, per_locale)
            })
            .collect();
        Self { by_id_gd }
    }

    /// Formatted part for `locale`, with the same `en_US` then first-locale fallback as the raw text.
    pub fn get(&self, id_gd: u32, locale: &str) -> Option<&FormattedText> {
        let per_locale = self.by_id_gd.get(&id_gd)?;
        per_locale
            .get(locale)
            .or_else(|| per_locale.get(FALLBACK_LOCALE))
            .or_else(|| per_locale.values().next())
    }
}

/// Picker formatting of every translation of an effect part.
pub fn format_effect_part_translations(
    translations: &BTreeMap<String, LocaleText>,
    element_type: &str,
    keywords: &KeywordCatalog,
) -> BTreeMap<String, FormattedText> {
    translations
        .iter()
        .map(|(locale, t)| {
            let formatted = format_effect_part(&t.text, element_type, locale, keywords);
            (locale.clone(), formatted)
        })
        .collect()
}

/// Keyword codes used in the effect texts of `catalog` that `keywords` cannot print.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UnresolvedKeywords {
    /// Codes with no name in any locale: left as `[CODE]` in formatted texts.
    pub unknown: BTreeSet<String>,
    /// Codes printed with their `en_US` name in some locales, as `code -> locales`.
    pub english_fallback: BTreeMap<String, BTreeSet<String>>,
}

pub fn unresolved_keywords(catalog: &IdGdCatalog, keywords: &KeywordCatalog) -> UnresolvedKeywords {
    let mut out = UnresolvedKeywords::default();
    for entry in &catalog.entries {
        for (locale, t) in &entry.translations {
            for code in bracket_codes(&t.text) {
                if keywords.name(code, locale).is_some() {
                    continue;
                }
                if keywords.name(code, FALLBACK_LOCALE).is_some() {
                    out.english_fallback
                        .entry(code.to_string())
                        .or_default()
                        .insert(locale.clone());
                } else {
                    out.unknown.insert(code.to_string());
                }
            }
        }
    }
    out
}

/// Non-empty `[CODE]` keyword codes in `text`.
fn bracket_codes(text: &str) -> impl Iterator<Item = &str> {
    text.split('[').skip(1).filter_map(|rest| {
        let code = &rest[..rest.find(']')?];
        let is_code = !code.is_empty() && code.chars().all(is_code_char);
        is_code.then_some(code)
    })
}

fn is_code_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
}

/// Split `text` on `[CODE]` keyword codes. Known codes become bold printed names, `[]` is dropped,
/// unknown codes are left as written.
fn keyword_segments(text: &str, locale: &str, keywords: &KeywordCatalog) -> FormattedText {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let code = after
            .find(']')
            .map(|end| &after[..end])
            .filter(|code| code.chars().all(is_code_char));
        let Some(code) = code else {
            plain.push_str(&rest[..=start]);
            rest = after;
            continue;
        };
        plain.push_str(&rest[..start]);
        rest = &after[code.len() + 1..];
        if code.is_empty() {
            continue;
        }
        let name = keywords
            .name(code, locale)
            .or_else(|| keywords.name(code, FALLBACK_LOCALE));
        match name {
            Some(name) => {
                out.push(TextSegment::plain(std::mem::take(&mut plain)));
                out.push(TextSegment::bold(name));
            }
            None => {
                plain.push('[');
                plain.push_str(code);
                plain.push(']');
            }
        }
    }
    plain.push_str(rest);
    out.push(TextSegment::plain(plain));
    out
}

/// Collapse runs of spaces (also across segments), trim both ends, drop empty segments and merge
/// neighbours with the same style.
fn tidy(segments: FormattedText) -> FormattedText {
    let mut out: FormattedText = Vec::with_capacity(segments.len());
    for seg in segments {
        let mut text = collapse_spaces(&seg.text);
        let prev_ends_with_space = out.last().is_some_and(|p| p.text.ends_with(' '));
        if prev_ends_with_space || out.is_empty() {
            text = text.trim_start().to_string();
        }
        if text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(prev) if prev.bold == seg.bold => prev.text.push_str(&text),
            _ => out.push(TextSegment {
                text,
                bold: seg.bold,
            }),
        }
    }
    while let Some(last) = out.last_mut() {
        let trimmed = last.text.trim_end();
        if trimmed.is_empty() {
            out.pop();
        } else {
            last.text.truncate(trimmed.len());
            break;
        }
    }
    out
}

fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALES: [&str; 5] = ["de_DE", "en_US", "es_ES", "fr_FR", "it_IT"];

    /// Printed names as found in the card data (`cardKeyword.translations[locale].displayWeb`).
    fn keywords() -> KeywordCatalog {
        let mut k = KeywordCatalog::default();
        k.insert("TOUGH_1", "en_US", "Tough\u{a0}1");
        for (locale, resupply, fleeting) in [
            ("de_DE", "Nachschub", "Vergänglich"),
            ("en_US", "Resupply", "Fleeting"),
            ("es_ES", "reabastece", "Fugacidad"),
            ("fr_FR", "Ravitaillez", "Fugace"),
            ("it_IT", "Rifornisci", "Fugace"),
        ] {
            k.insert("RESUPPLY_LOW", locale, resupply);
            k.insert("FLEETING", locale, fleeting);
        }
        k
    }

    fn picker(raw: &str, element_type: &str, locale: &str) -> FormattedText {
        format_effect_part(raw, element_type, locale, &keywords())
    }

    fn plain(text: &str) -> FormattedText {
        vec![TextSegment::plain(text)]
    }

    #[test]
    fn empty_condition_is_labelled_no_condition() {
        assert_eq!(picker("[]", "CONDITION", "fr_FR"), plain("Sans condition"));
        assert_eq!(picker("[]", "CONDITION", "en_US"), plain("No condition"));
        assert_eq!(
            picker(" [] ", "CONDITION", "de_DE"),
            plain("Keine Bedingung")
        );
    }

    #[test]
    fn empty_trigger_or_output_has_no_segments() {
        assert_eq!(picker("[]", "TRIGGER", "fr_FR"), Vec::new());
        assert_eq!(picker("[]", "OUTPUT", "en_US"), Vec::new());
    }

    #[test]
    fn play_symbols_alone_are_labelled() {
        assert_eq!(
            picker("{H}", "TRIGGER", "fr_FR"),
            plain("Joué depuis la Main")
        );
        assert_eq!(
            picker("{R}", "TRIGGER", "fr_FR"),
            plain("Joué depuis la Réserve")
        );
        assert_eq!(picker("{J}", "TRIGGER", "fr_FR"), plain("Joué de partout"));
        assert_eq!(picker("{H}", "TRIGGER", "en_US"), plain("Played from Hand"));
        assert_eq!(
            picker("{R}", "TRIGGER", "en_US"),
            plain("Played from Reserve")
        );
        assert_eq!(
            picker("{J}", "TRIGGER", "en_US"),
            plain("Played from anywhere")
        );
    }

    #[test]
    fn exhaust_symbol_alone_is_labelled() {
        assert_eq!(picker("{T}", "TRIGGER", "fr_FR"), plain("Épuisez-moi"));
        assert_eq!(picker("{T}", "TRIGGER", "en_US"), plain("Exhaust me"));
    }

    #[test]
    fn discard_symbol_alone_is_labelled_with_or_without_colon() {
        let label = plain("Défaussez-moi de la Réserve");
        assert_eq!(picker("{D}", "TRIGGER", "fr_FR"), label);
        assert_eq!(picker("{D} :", "TRIGGER", "fr_FR"), label);
        assert_eq!(picker("{D}\u{a0}:", "TRIGGER", "fr_FR"), label);
        assert_eq!(
            picker("{D}:", "TRIGGER", "en_US"),
            plain("Discard me from Reserve")
        );
    }

    #[test]
    fn symbols_inside_a_text_are_kept() {
        assert_eq!(
            picker("{J} Piochez une carte.", "OUTPUT", "fr_FR"),
            plain("{J} Piochez une carte.")
        );
    }

    #[test]
    fn leading_i_symbol_is_dropped() {
        assert_eq!(
            picker("{I} Lorsque je vais en Réserve —", "TRIGGER", "fr_FR"),
            plain("Lorsque je vais en Réserve —")
        );
        assert_eq!(picker("{I}", "TRIGGER", "fr_FR"), Vec::new());
    }

    #[test]
    fn keyword_codes_become_bold_printed_names_in_each_locale() {
        let names = [
            ("de_DE", "Nachschub"),
            ("en_US", "Resupply"),
            ("es_ES", "reabastece"),
            ("fr_FR", "Ravitaillez"),
            ("it_IT", "Rifornisci"),
        ];
        for (locale, name) in names {
            assert_eq!(
                picker("[RESUPPLY_LOW].", "OUTPUT", locale),
                vec![TextSegment::bold(name), TextSegment::plain(".")],
                "{locale}"
            );
        }
    }

    #[test]
    fn keyword_inside_a_sentence_keeps_surrounding_text() {
        assert_eq!(
            picker(
                "Vous pouvez cibler un Personnage. Il gagne [FLEETING].",
                "OUTPUT",
                "fr_FR"
            ),
            vec![
                TextSegment::plain("Vous pouvez cibler un Personnage. Il gagne "),
                TextSegment::bold("Fugace"),
                TextSegment::plain("."),
            ]
        );
    }

    #[test]
    fn unknown_keyword_code_is_left_as_written() {
        assert_eq!(
            picker("Je suis [UNKNOWN_CODE].", "OUTPUT", "fr_FR"),
            plain("Je suis [UNKNOWN_CODE].")
        );
    }

    #[test]
    fn keyword_missing_in_locale_falls_back_to_english() {
        let mut k = KeywordCatalog::default();
        k.insert("GIFT", "en_US", "Gift");
        assert_eq!(
            format_effect_part("[GIFT]", "OUTPUT", "fr_FR", &k),
            vec![TextSegment::bold("Gift")]
        );
    }

    #[test]
    fn bracketed_text_that_is_not_a_code_is_kept() {
        assert_eq!(picker("a [b] c", "OUTPUT", "en_US"), plain("a [b] c"));
    }

    #[test]
    fn spaces_are_normalized_and_trailing_colon_removed() {
        assert_eq!(
            picker(
                "  Si j'ai au moins 1\u{a0}boost\u{a0}:  ",
                "CONDITION",
                "fr_FR"
            ),
            plain("Si j'ai au moins 1 boost")
        );
        assert_eq!(
            picker(
                "Lorsque   mon Expédition\u{202f}échoue —",
                "TRIGGER",
                "fr_FR"
            ),
            plain("Lorsque mon Expédition échoue —")
        );
    }

    #[test]
    fn every_locale_is_formatted() {
        for locale in LOCALES {
            assert!(
                !picker("[FLEETING]", "OUTPUT", locale).is_empty(),
                "{locale}"
            );
        }
    }

    #[test]
    fn non_breaking_space_inside_keyword_name_is_normalized_in_pickers_only() {
        assert_eq!(
            picker("I am [TOUGH_1].", "OUTPUT", "en_US"),
            vec![
                TextSegment::plain("I am "),
                TextSegment::bold("Tough 1"),
                TextSegment::plain("."),
            ]
        );
        assert_eq!(
            format_card_part("I am [TOUGH_1].", "en_US", &keywords())[1],
            TextSegment::bold("Tough\u{a0}1")
        );
    }

    #[test]
    fn unresolved_keywords_lists_unknown_codes_and_english_fallbacks() {
        let entry =
            |id_gd: u32, texts: &[(&str, &str)]| index_core::idgd_catalog::IdGdCatalogEntry {
                id_gd,
                card_count: 1,
                bitmap_bytes: 1,
                bitmap_file: String::new(),
                element_type: "OUTPUT".to_string(),
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
            };
        let catalog = IdGdCatalog {
            set: "TEST".to_string(),
            entries: vec![
                entry(
                    1,
                    &[("en_US", "[] [FLEETING] [TOUGH_1]"), ("fr_FR", "[TOUGH_1]")],
                ),
                entry(2, &[("en_US", "[SABOTAGE_LOW] and [b]")]),
            ],
        };
        let unresolved = unresolved_keywords(&catalog, &keywords());
        assert_eq!(
            unresolved.unknown,
            BTreeSet::from(["SABOTAGE_LOW".to_string()])
        );
        assert_eq!(
            unresolved.english_fallback,
            BTreeMap::from([("TOUGH_1".to_string(), BTreeSet::from(["fr_FR".to_string()]))])
        );
    }

    #[test]
    fn card_part_drops_empty_condition_and_keeps_symbols() {
        let k = keywords();
        assert_eq!(format_card_part("[]", "fr_FR", &k), Vec::new());
        assert_eq!(
            format_card_part("{D}\u{a0}:", "fr_FR", &k),
            plain("{D}\u{a0}:")
        );
        assert_eq!(
            format_card_part("Il gagne [FLEETING].", "en_US", &k),
            vec![
                TextSegment::plain("Il gagne "),
                TextSegment::bold("Fleeting"),
                TextSegment::plain("."),
            ]
        );
    }

    #[test]
    fn card_line_joins_parts_with_one_space() {
        let k = keywords();
        let parts = [
            format_card_part("{J}", "fr_FR", &k),
            format_card_part("[]", "fr_FR", &k),
            format_card_part("Il gagne [FLEETING].", "fr_FR", &k),
        ];
        assert_eq!(
            join_card_parts(&parts),
            vec![
                TextSegment::plain("{J} Il gagne "),
                TextSegment::bold("Fugace"),
                TextSegment::plain("."),
            ]
        );
    }

    #[test]
    fn segments_serialize_bold_only_when_set() {
        let json = serde_json::to_value(vec![TextSegment::bold("Fugace"), TextSegment::plain(".")])
            .unwrap();
        assert_eq!(
            json,
            serde_json::json!([{ "text": "Fugace", "bold": true }, { "text": "." }])
        );
    }
}
