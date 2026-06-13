//! .NET `StringInfo` text-element segmentation (`StdLib.strLen` parity).
//!
//! C# `StdLib.StrLen` counts the *text elements* of a string via
//! `StringInfo.GetTextElementEnumerator`, which since .NET 5 walks UAX #29
//! extended grapheme clusters — with two .NET-specific properties that rule
//! out reusing a generic UAX #29 implementation:
//!
//! - **no rule GB9c**: .NET never joins Indic conjunct clusters, so
//!   `U+0915 U+094D U+0915` (KA, virama, KA) is **2** text elements where a
//!   current UAX #29 segmenter produces 1;
//! - **a pinned Unicode snapshot**: the break-property data is whatever ships
//!   inside `System.Private.CoreLib`, not the latest Unicode release.
//!
//! This module therefore evaluates the cluster rules exactly the way .NET's
//! `TextSegmentationUtility.GetLengthOfFirstUtf16ExtendedGraphemeCluster`
//! does (GB1-GB13 *minus* GB9c), over the break-property table in
//! [`crate::dotnet_graphemes`] that was generated from — and verified code
//! point by code point against — the .NET runtime itself. The whole pipeline
//! is gated by `tests/fixtures/dotnet_strlen_oracle.txt`: 5600 strings whose
//! text-element counts were produced by .NET's `StringInfo`, every one of
//! which must be reproduced here.

use crate::dotnet_graphemes::DOTNET_GRAPHEME_BREAK_RANGES;

/// Grapheme-cluster break classes as .NET's internal
/// `CharUnicodeInfo.GraphemeClusterBreakType` distinguishes them. The
/// discriminants match the class ids in the generated
/// [`DOTNET_GRAPHEME_BREAK_RANGES`] table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum GraphemeBreakClass {
    Other = 0,
    Cr = 1,
    Lf = 2,
    Control = 3,
    Extend = 4,
    Zwj = 5,
    RegionalIndicator = 6,
    Prepend = 7,
    SpacingMark = 8,
    HangulL = 9,
    HangulV = 10,
    HangulT = 11,
    HangulLv = 12,
    HangulLvt = 13,
    ExtendedPictographic = 14,
}

/// Looks up the break class of a scalar in the generated range-run table
/// (binary search; everything not covered is `Other`).
pub(crate) fn break_class(c: char) -> GraphemeBreakClass {
    let cp = c as u32;
    let ranges = DOTNET_GRAPHEME_BREAK_RANGES;
    let idx = ranges.partition_point(|&(_, end, _)| end < cp);
    if let Some(&(start, _, class)) = ranges.get(idx) {
        if cp >= start {
            return match class {
                1 => GraphemeBreakClass::Cr,
                2 => GraphemeBreakClass::Lf,
                3 => GraphemeBreakClass::Control,
                4 => GraphemeBreakClass::Extend,
                5 => GraphemeBreakClass::Zwj,
                6 => GraphemeBreakClass::RegionalIndicator,
                7 => GraphemeBreakClass::Prepend,
                8 => GraphemeBreakClass::SpacingMark,
                9 => GraphemeBreakClass::HangulL,
                10 => GraphemeBreakClass::HangulV,
                11 => GraphemeBreakClass::HangulT,
                12 => GraphemeBreakClass::HangulLv,
                13 => GraphemeBreakClass::HangulLvt,
                14 => GraphemeBreakClass::ExtendedPictographic,
                _ => GraphemeBreakClass::Other,
            };
        }
    }
    GraphemeBreakClass::Other
}

/// The number of .NET text elements (extended grapheme clusters, .NET
/// semantics) in `s` — exactly what C# `StdLib.StrLen` returns.
pub(crate) fn text_element_count(s: &str) -> usize {
    let classes: Vec<GraphemeBreakClass> = s.chars().map(break_class).collect();
    let mut count = 0usize;
    let mut pos = 0usize;
    while pos < classes.len() {
        pos += first_cluster_len(&classes[pos..]);
        count += 1;
    }
    count
}

/// Length (in scalars) of the first extended grapheme cluster of the
/// non-empty class slice. This is a direct port of .NET's
/// `TextSegmentationUtility` cluster scan: the UAX #29 rules GB1-GB13 minus
/// GB9c, evaluated in .NET's order.
fn first_cluster_len(classes: &[GraphemeBreakClass]) -> usize {
    use GraphemeBreakClass as C;
    let n = classes.len();
    let mut i = 0usize;

    // GB9b: leading Prepend* attaches to the cluster that follows...
    if classes[0] == C::Prepend {
        while i < n && classes[i] == C::Prepend {
            i += 1;
        }
        // GB2: nothing but Prepend scalars left.
        if i == n {
            return i;
        }
        // ...unless a Control/CR/LF follows (GB5 outranks GB9b).
        if matches!(classes[i], C::Control | C::Cr | C::Lf) {
            return i;
        }
    }

    let mut state = classes[i];
    i += 1;

    // GB3/GB4: CR pairs only with an immediately following LF, and nothing
    // attaches after Control, CR or LF (not even Extend).
    if state == C::Cr {
        if i < n && classes[i] == C::Lf {
            i += 1;
        }
        return i;
    }
    if matches!(state, C::Control | C::Lf) {
        return i;
    }

    // GB6-GB8 (Hangul syllables), GB11 (emoji ZWJ chains), GB12/GB13
    // (regional-indicator pairs).
    loop {
        match state {
            C::HangulL => {
                if i < n
                    && matches!(
                        classes[i],
                        C::HangulL | C::HangulV | C::HangulLv | C::HangulLvt
                    )
                {
                    state = classes[i];
                    i += 1;
                    continue;
                }
            }
            C::HangulLv | C::HangulV => {
                if i < n && matches!(classes[i], C::HangulV | C::HangulT) {
                    state = classes[i];
                    i += 1;
                    continue;
                }
            }
            C::HangulLvt | C::HangulT => {
                if i < n && classes[i] == C::HangulT {
                    i += 1;
                    continue;
                }
            }
            C::ExtendedPictographic => {
                // GB11: Extended_Pictographic Extend* ZWJ x Extended_Pictographic.
                while i < n && classes[i] == C::Extend {
                    i += 1;
                }
                if i < n && classes[i] == C::Zwj {
                    if i + 1 < n && classes[i + 1] == C::ExtendedPictographic {
                        i += 2; // stay in the Extended_Pictographic state
                        continue;
                    }
                    i += 1; // the ZWJ still attaches (GB9); the chain ends here
                }
            }
            C::RegionalIndicator => {
                // GB12/GB13: regional indicators join in pairs.
                if i < n && classes[i] == C::RegionalIndicator {
                    i += 1;
                }
            }
            _ => {}
        }
        break;
    }

    // GB9/GB9a: trailing Extend/ZWJ/SpacingMark always attach.
    while i < n && matches!(classes[i], C::Extend | C::Zwj | C::SpacingMark) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes one oracle-fixture line ("count<TAB>hex hex ...") into the
    /// string and its .NET text-element count.
    fn parse_fixture_line(line: &str) -> (String, usize) {
        let (count, scalars) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("malformed fixture line: {line:?}"));
        let count: usize = count.parse().expect("fixture count");
        let s: String = scalars
            .split_whitespace()
            .map(|hex| {
                let cp = u32::from_str_radix(hex, 16).expect("fixture scalar hex");
                char::from_u32(cp).expect("fixture scalar must be a valid char")
            })
            .collect();
        (s, count)
    }

    /// THE GATE: every string in the .NET-generated oracle fixture (random
    /// multi-script strings plus targeted adversarial clusters, with counts
    /// produced by .NET's own StringInfo) must be reproduced exactly.
    #[test]
    fn matches_dotnet_oracle_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dotnet_strlen_oracle.txt"
        ));
        let mut total = 0usize;
        let mut mismatches: Vec<String> = Vec::new();
        for line in fixture.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (s, expected) = parse_fixture_line(line);
            total += 1;
            let got = text_element_count(&s);
            if got != expected {
                let scalars: Vec<String> =
                    s.chars().map(|c| format!("U+{:04X}", c as u32)).collect();
                mismatches.push(format!(
                    "[{}] expected {expected}, got {got}",
                    scalars.join(" ")
                ));
            }
        }
        assert!(
            total >= 5000,
            "oracle fixture must contain at least 5000 cases, found {total}"
        );
        assert!(
            mismatches.is_empty(),
            "{} of {} oracle cases diverge from .NET StringInfo; first 10:\n{}",
            mismatches.len(),
            total,
            mismatches[..mismatches.len().min(10)].join("\n")
        );
    }

    #[test]
    fn break_class_pins_match_dotnet_table() {
        use GraphemeBreakClass as C;
        let pins = [
            ('\u{0041}', C::Other),
            ('\u{000D}', C::Cr),
            ('\u{000A}', C::Lf),
            ('\u{0001}', C::Control),
            ('\u{200B}', C::Control), // ZERO WIDTH SPACE
            ('\u{0300}', C::Extend),
            ('\u{200C}', C::Extend), // ZWNJ
            ('\u{094D}', C::Extend), // Devanagari virama
            ('\u{200D}', C::Zwj),
            ('\u{1F1E6}', C::RegionalIndicator),
            ('\u{0600}', C::Prepend),
            ('\u{0903}', C::SpacingMark),
            ('\u{1100}', C::HangulL),
            ('\u{1160}', C::HangulV),
            ('\u{11A8}', C::HangulT),
            ('\u{AC00}', C::HangulLv),
            ('\u{AC01}', C::HangulLvt),
            ('\u{1F600}', C::ExtendedPictographic),
            ('\u{00A9}', C::ExtendedPictographic), // COPYRIGHT SIGN
            ('\u{0915}', C::Other),                // Devanagari KA
            ('\u{10FFFF}', C::Other),
        ];
        for (c, expected) in pins {
            assert_eq!(break_class(c), expected, "U+{:04X}", c as u32);
        }
    }

    /// The divergence that disqualifies generic UAX #29 segmenters: .NET has
    /// no GB9c, so Indic virama conjuncts stay split.
    #[test]
    fn no_gb9c_indic_conjuncts_stay_split() {
        assert_eq!(text_element_count("\u{0915}\u{094D}\u{0915}"), 2);
        assert_eq!(
            text_element_count("\u{0915}\u{094D}\u{0915}\u{094D}\u{0915}"),
            3
        );
        // Bengali RA + virama + MA.
        assert_eq!(text_element_count("\u{09B0}\u{09CD}\u{09AE}"), 2);
        // Khmer with coeng.
        assert_eq!(text_element_count("\u{1780}\u{17D2}\u{1780}"), 2);
    }

    #[test]
    fn cluster_rules_match_dotnet() {
        // GB3/GB4/GB5: CRLF is one element; nothing attaches to controls.
        assert_eq!(text_element_count("\r\n"), 1);
        assert_eq!(text_element_count("\n\r"), 2);
        assert_eq!(text_element_count("\r\n\r\n"), 2);
        assert_eq!(text_element_count("\r\u{0300}"), 2);
        // GB6-GB8: Hangul syllables, precomposed and decomposed.
        assert_eq!(text_element_count("\u{1100}\u{1161}\u{11A8}"), 1);
        assert_eq!(text_element_count("\u{1100}\u{AC00}"), 1); // L + LV
        assert_eq!(text_element_count("\u{AC01}\u{11A8}"), 1); // LVT + T
        assert_eq!(text_element_count("\u{11A8}\u{1100}"), 2); // T then L breaks
        // GB9/GB9a: Extend / ZWJ / SpacingMark attach.
        assert_eq!(text_element_count("a\u{0301}\u{0302}\u{0303}"), 1);
        assert_eq!(text_element_count("\u{0E01}\u{0E33}"), 1); // Thai + SARA AM
        // GB9b: Prepend attaches forward, but not to controls.
        assert_eq!(text_element_count("\u{0600}\u{0661}"), 1);
        assert_eq!(text_element_count("\u{0600}\r"), 2);
        // GB11: emoji ZWJ sequences, including non-emoji Extended_Pictographic.
        assert_eq!(
            text_element_count("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"),
            1
        );
        assert_eq!(text_element_count("\u{00A9}\u{200D}\u{00A9}"), 1);
        assert_eq!(text_element_count("\u{1F600}\u{0300}\u{200D}\u{1F600}"), 1);
        // ...but a SpacingMark breaks the GB11 chain.
        assert_eq!(text_element_count("\u{1F600}\u{0903}\u{200D}\u{1F600}"), 2);
        // A dangling ZWJ does not join what follows.
        assert_eq!(text_element_count("a\u{200D}b"), 2);
        // GB12/GB13: regional indicators pair up.
        assert_eq!(text_element_count("\u{1F1FA}\u{1F1F8}"), 1);
        assert_eq!(text_element_count("\u{1F1E6}\u{1F1E7}\u{1F1E8}"), 2);
        assert_eq!(
            text_element_count("\u{1F1E6}\u{1F1E7}\u{1F1E8}\u{1F1E9}"),
            2
        );
        // Keycap: digit + VS16 + COMBINING ENCLOSING KEYCAP.
        assert_eq!(text_element_count("1\u{FE0F}\u{20E3}"), 1);
        // Tag sequence (flag of Scotland).
        assert_eq!(
            text_element_count("\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}"),
            1
        );
        // Empty string.
        assert_eq!(text_element_count(""), 0);
    }
}
