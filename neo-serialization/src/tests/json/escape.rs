use super::*;
use serde_json::json;

#[test]
fn escapes_html_sensitive_and_non_ascii_like_csharp() {
    // C# JavaScriptEncoder.Default escapes '<' '>' '&' '\'' '+' '`' and all
    // non-ASCII code points as \uXXXX (uppercase hex).
    let value = json!({ "name": "<a> & 'b' + `c` 中😀" });
    let out = to_string(&value, false).expect("serialize");
    assert_eq!(
        out,
        "{\"name\":\"\\u003Ca\\u003E \\u0026 \\u0027b\\u0027 \\u002B \\u0060c\\u0060 \\u4E2D\\uD83D\\uDE00\"}"
    );
}

#[test]
fn escapes_quote_as_unicode_and_uses_short_control_forms() {
    let value = json!("\"\\\n\t\r");
    let out = to_string(&value, false).expect("serialize");
    // Quote -> ", backslash -> \\, newline/tab/cr -> short forms.
    assert_eq!(out, "\"\\u0022\\\\\\n\\t\\r\"");
}

#[test]
fn leaves_plain_ascii_untouched() {
    let value = json!({ "k": "Hello, World! (test) [a]/b:c" });
    let out = to_string(&value, false).expect("serialize");
    assert_eq!(out, "{\"k\":\"Hello, World! (test) [a]/b:c\"}");
}

#[test]
fn pretty_output_preserves_indentation() {
    let value = json!({ "a": "<x>" });
    let out = to_string(&value, true).expect("serialize");
    assert_eq!(out, "{\n  \"a\": \"\\u003Cx\\u003E\"\n}");
}
