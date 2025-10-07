//! Tests for the `AtomTag` identifier, ensuring validation logic is correct.
//!
//! These tests cover valid and invalid formats, including various Unicode characters,
//! edge cases, and specific error conditions to ensure the tag parsing is robust.

use super::*;

#[test]
fn edge_cases() {
    assert_eq!(
        AtomTag::try_from("α"),
        Ok(AtomTag("α".into())),
        "Single valid Unicode character should be accepted"
    );

    assert_eq!(
        AtomTag::try_from("ñ_1"),
        Ok(AtomTag("ñ_1".into())),
        "Mix of Unicode, underscore, and number should be valid"
    );

    assert_eq!(
        AtomTag::try_from("\u{200B}"),
        Err(Error::InvalidStart('\u{200B}')),
        "Zero-width space should be invalid start"
    );

    assert_eq!(
        AtomTag::try_from("α\u{200B}"),
        Err(Error::InvalidCharacters("\u{200B}".into())),
        "Zero-width space should be invalid in the middle"
    );
}

#[test]
fn empty() {
    let res = AtomTag::try_from("");
    assert!(res == Err(Error::Empty));
}

#[test]
fn invalid_chars() {
    let res = AtomTag::try_from("a-!@#$%^&*()_-asdf");
    assert!(res == Err(Error::InvalidCharacters("!@#$%^&*()".into())));
}

#[test]
fn invalid_start() {
    let assert = |s: &str| {
        let res = AtomTag::try_from(s);
        assert!(res == Err(Error::InvalidStart(s.chars().next().unwrap())));
    };
    for a in ["9atom", "'atom", "_atom", "-atom", "%atom"] {
        assert(a)
    }
}

#[test]
fn invalid_unicode_tags() {
    let invalid_tags = [
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

    for tag in invalid_tags {
        assert!(
            AtomTag::try_from(tag).is_err(),
            "Expected '{}' to be invalid",
            tag
        );
    }
}

#[test]
fn specific_unicode_errors() {
    assert_eq!(
        AtomTag::try_from("123αβγ"),
        Err(Error::InvalidStart('1')),
        "Should fail for starting with a number"
    );

    assert_eq!(
        AtomTag::try_from("αβγ!@#"),
        Err(Error::InvalidCharacters("!@#".into())),
        "Should fail for invalid characters"
    );

    assert_eq!(
        AtomTag::try_from("한글 漢字"),
        Err(Error::InvalidCharacters(" ".into())),
        "Should fail for space between valid characters"
    );

    assert_eq!(
        AtomTag::try_from("Café♥"),
        Err(Error::InvalidCharacters("♥".into())),
        "Should fail for heart symbol"
    );
}

#[test]
fn valid_unicode_tags() {
    let valid_tags = [
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

    for tag in valid_tags {
        assert!(
            AtomTag::try_from(tag).is_ok(),
            "Expected '{}' to be valid",
            tag
        );
    }
}
