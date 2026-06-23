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
            // GB12/GB13: regional indicators join in pairs.
            C::RegionalIndicator if i < n && classes[i] == C::RegionalIndicator => {
                i += 1;
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
#[path = "tests/dotnet_text_segmentation.rs"]
mod tests;
