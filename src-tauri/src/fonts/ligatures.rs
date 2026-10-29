use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
};

use font_kit::{
    family_name::FamilyName, handle::Handle, properties::Properties, source::SystemSource,
};
use rustybuzz::{
    ttf_parser::{gsub::SubstitutionSubtable, opentype_layout::Coverage, GlyphId, Tag},
    Face, Feature, UnicodeBuffer,
};
use serde::Deserialize;

const MAX_CANDIDATES: usize = 4096;
const MAX_LIGATURE_CHARS: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontLigaturesRequest {
    families: Vec<String>,
    candidates: Vec<String>,
}

/// Resolves font-specific sequences once per font change. The renderer only receives
/// compact strings, keeping synchronous character joining off the IPC boundary.
#[tauri::command]
pub async fn font_ligatures_resolve(request: FontLigaturesRequest) -> Option<Vec<String>> {
    match tokio::task::spawn_blocking(move || resolve_ligatures(request)).await {
        Ok(result) => result,
        Err(error) => {
            log::warn!(target: "fonts", "font ligature resolution task failed: {error}");
            None
        }
    }
}

fn resolve_ligatures(request: FontLigaturesRequest) -> Option<Vec<String>> {
    let families: Vec<_> = request
        .families
        .iter()
        .map(String::as_str)
        .filter_map(parse_family)
        .collect();
    if families.is_empty() {
        return None;
    }

    let handle = SystemSource::new()
        .select_best_match(&families, &Properties::new())
        .ok()?;
    let (bytes, face_index) = match &handle {
        Handle::Path { path, font_index } => match std::fs::read(path) {
            Ok(bytes) => (Cow::Owned(bytes), *font_index),
            Err(error) => {
                log::warn!(target: "fonts", "failed to read font {}: {error}", path.display());
                return None;
            }
        },
        Handle::Memory { bytes, font_index } => (Cow::Borrowed(bytes.as_slice()), *font_index),
    };
    let face = Face::from_slice(bytes.as_ref(), face_index)?;
    let mut candidates: BTreeSet<_> = request
        .candidates
        .into_iter()
        .filter(|text| is_valid_candidate(text))
        .take(MAX_CANDIDATES)
        .collect();
    candidates.extend(discover_ligatures(&face));

    Some(
        candidates
            .into_iter()
            .filter(|text| has_ligature_substitution(&face, text))
            .take(MAX_CANDIDATES)
            .collect(),
    )
}

fn parse_family(family: &str) -> Option<FamilyName> {
    let family = family.trim();
    if family.is_empty() {
        return None;
    }
    Some(match family.to_ascii_lowercase().as_str() {
        "serif" => FamilyName::Serif,
        "sans-serif" => FamilyName::SansSerif,
        "monospace" => FamilyName::Monospace,
        "cursive" => FamilyName::Cursive,
        "fantasy" => FamilyName::Fantasy,
        "system-ui" | "emoji" | "math" | "fangsong" => return None,
        _ => FamilyName::Title(family.to_string()),
    })
}

fn is_valid_candidate(text: &str) -> bool {
    let length = text.chars().count();
    (2..=MAX_LIGATURE_CHARS).contains(&length) && !text.chars().any(char::is_control)
}

fn discover_ligatures(face: &Face) -> BTreeSet<String> {
    let mut sequences = BTreeSet::new();
    let Some(gsub) = face.tables().gsub else {
        return sequences;
    };
    let mut glyph_chars = None;

    for lookup in gsub.lookups {
        for subtable in lookup.subtables.into_iter::<SubstitutionSubtable>() {
            let SubstitutionSubtable::Ligature(table) = subtable else {
                continue;
            };
            let chars = glyph_chars.get_or_insert_with(|| glyph_char_map(face));
            for coverage_index in 0..table.ligature_sets.len() {
                let Some(first) = coverage_glyph(&table.coverage, coverage_index) else {
                    continue;
                };
                let Some(set) = table.ligature_sets.get(coverage_index) else {
                    continue;
                };
                for ligature in set {
                    let text = std::iter::once(first)
                        .chain(ligature.components)
                        .map(|glyph| chars.get(&glyph).copied())
                        .collect::<Option<String>>();
                    if let Some(text) = text.filter(|text| is_valid_candidate(text)) {
                        sequences.insert(text);
                        if sequences.len() >= MAX_CANDIDATES {
                            return sequences;
                        }
                    }
                }
            }
        }
    }
    sequences
}

fn glyph_char_map(face: &Face) -> HashMap<GlyphId, char> {
    let mut chars = HashMap::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            if !subtable.is_unicode() {
                continue;
            }
            subtable.codepoints(|codepoint| {
                if let Some(character) = char::from_u32(codepoint) {
                    if let Some(glyph) = subtable.glyph_index(codepoint) {
                        chars.entry(glyph).or_insert(character);
                    }
                }
            });
        }
    }
    chars
}

fn coverage_glyph(coverage: &Coverage, index: u16) -> Option<GlyphId> {
    match coverage {
        Coverage::Format1 { glyphs } => glyphs.get(index),
        Coverage::Format2 { records } => {
            for record in *records {
                let size = record.end.0.checked_sub(record.start.0)?.checked_add(1)?;
                let offset = index.checked_sub(record.value)?;
                if offset < size {
                    return record.start.0.checked_add(offset).map(GlyphId);
                }
            }
            None
        }
    }
}

fn has_ligature_substitution(face: &Face, text: &str) -> bool {
    shape_glyphs(face, text, false) != shape_glyphs(face, text, true)
}

fn shape_glyphs(face: &Face, text: &str, disable_ligatures: bool) -> Vec<(u32, u32)> {
    let disabled_features = [
        Feature::new(Tag::from_bytes(b"calt"), 0, ..),
        Feature::new(Tag::from_bytes(b"liga"), 0, ..),
        Feature::new(Tag::from_bytes(b"clig"), 0, ..),
        Feature::new(Tag::from_bytes(b"rlig"), 0, ..),
    ];
    let features = if disable_ligatures {
        disabled_features.as_slice()
    } else {
        &[]
    };
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    rustybuzz::shape(face, features, buffer)
        .glyph_infos()
        .iter()
        .map(|info| (info.glyph_id, info.cluster))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_validation_rejects_single_and_control_characters() {
        assert!(!is_valid_candidate(""));
        assert!(!is_valid_candidate("="));
        assert!(!is_valid_candidate("=\n="));
        assert!(is_valid_candidate("=>"));
    }

    #[test]
    fn css_generic_families_map_to_font_kit() {
        assert!(matches!(
            parse_family("monospace"),
            Some(FamilyName::Monospace)
        ));
        assert!(matches!(
            parse_family("Fira Code"),
            Some(FamilyName::Title(_))
        ));
        assert!(parse_family("system-ui").is_none());
    }

    #[test]
    fn resolves_cascadia_code_when_installed() {
        let request = FontLigaturesRequest {
            families: vec!["Cascadia Code".into()],
            candidates: ["->", "=>", "==", "fi"].map(String::from).into(),
        };
        let Some(ligatures) = resolve_ligatures(request) else {
            return;
        };
        assert!(ligatures.iter().any(|text| text == "->"));
        assert!(ligatures.iter().any(|text| text == "=>"));
    }
}
