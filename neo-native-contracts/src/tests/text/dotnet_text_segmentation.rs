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
            let scalars: Vec<String> = s.chars().map(|c| format!("U+{:04X}", c as u32)).collect();
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
