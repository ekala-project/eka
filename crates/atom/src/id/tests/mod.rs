use super::*;

#[test]
fn empty() {
    let res = Id::try_from("");
    assert!(res == Err(IdError::Empty));
}

#[test]
fn invalid_start() {
    let assert = |s: &str| {
        let res = Id::try_from(s);
        assert!(res == Err(IdError::InvalidStart(s.chars().next().unwrap())));
    };
    for a in ["9atom", "'atom", "_atom", "-atom", "%atom"] {
        assert(a)
    }
}

#[test]
fn invalid_chars() {
    let res = Id::try_from("a-!@#$%^&*()_-asdf");
    assert!(res == Err(IdError::InvalidCharacters("!@#$%^&*()".into())));
}

#[test]
fn valid_unicode_ids() {
    let valid_ids = [
        "αβγ",              // Greek lowercase
        "ΑΒΓ",              // Greek uppercase
        "кириллица",        // Cyrillic
        "汉字",             // Chinese characters
        "ひらがな",         // Japanese Hiragana
        "한글",             // Korean Hangul
        "Ññ",               // Spanish letter with tilde
        "Öö",               // German umlaut
        "Ææ",               // Latin ligature
        "Łł",               // Polish letter
        "ئ",                // Arabic letter
        "א",                // Hebrew letter
        "ก",                // Thai letter
        "Ա",                // Armenian letter
        "ᚠ",                // Runic letter
        "ᓀ",                // Canadian Aboriginal Syllabics
        "あア",             // Mix of Hiragana and Katakana
        "한글漢字",         // Mix of Korean and Chinese
        "Café_au_lait-123", // Mix of Latin, underscore, hyphen, and numbers
    ];

    for id in valid_ids {
        assert!(Id::try_from(id).is_ok(), "Expected '{}' to be valid", id);
    }
}

#[test]
fn invalid_unicode_ids() {
    let invalid_ids = [
        "123αβγ",             // Starts with number
        "_ΑΒΓ",               // Starts with underscore
        "-кириллица",         // Starts with hyphen
        "汉字!",              // Contains exclamation mark
        "ひらがな。",         // Contains Japanese full stop
        "한글 ",              // Contains space
        "Ññ\u{200B}",         // Contains zero-width space
        "Öö\t",               // Contains tab
        "Ææ\n",               // Contains newline
        "Łł\r",               // Contains carriage return
        "ئ،",                 // Contains Arabic comma
        "א״",                 // Contains Hebrew punctuation
        "ก๏",                 // Contains Thai character not in allowed categories
        "Ա։",                 // Contains Armenian full stop
        "ᚠ᛫",                 // Contains Runic punctuation
        "あア·",              // Contains middle dot
        "한글漢字♥",          // Contains heart symbol
        "Café_au_lait-123☕", // Contains coffee symbol
    ];

    for id in invalid_ids {
        assert!(Id::try_from(id).is_err(), "Expected '{}' to be invalid", id);
    }
}

#[test]
fn specific_unicode_errors() {
    assert_eq!(
        Id::try_from("123αβγ"),
        Err(IdError::InvalidStart('1')),
        "Should fail for starting with a number"
    );

    assert_eq!(
        Id::try_from("αβγ!@#"),
        Err(IdError::InvalidCharacters("!@#".into())),
        "Should fail for invalid characters"
    );

    assert_eq!(
        Id::try_from("한글 漢字"),
        Err(IdError::InvalidCharacters(" ".into())),
        "Should fail for space between valid characters"
    );

    assert_eq!(
        Id::try_from("Café♥"),
        Err(IdError::InvalidCharacters("♥".into())),
        "Should fail for heart symbol"
    );
}

#[test]
fn edge_cases() {
    assert_eq!(
        Id::try_from("α"),
        Ok(Id("α".into())),
        "Single valid Unicode character should be accepted"
    );

    assert_eq!(
        Id::try_from("ñ_1"),
        Ok(Id("ñ_1".into())),
        "Mix of Unicode, underscore, and number should be valid"
    );

    assert_eq!(
        Id::try_from("\u{200B}"),
        Err(IdError::InvalidStart('\u{200B}')),
        "Zero-width space should be invalid start"
    );

    assert_eq!(
        Id::try_from("α\u{200B}"),
        Err(IdError::InvalidCharacters("\u{200B}".into())),
        "Zero-width space should be invalid in the middle"
    );
}
