/// Removes every saved secret from text before it is kept or shown.
///
/// Short values are skipped because a short string would match ordinary words
/// and hide unrelated text. A session registers only values that are at least
/// eight characters long.
pub(crate) fn redact_secrets(mut data: String, secrets: &[String]) -> String {
    for secret in secrets {
        if secret.chars().count() >= 8 {
            data = data.replace(secret, "[redacted]");
        }
    }
    data
}

/// Cleans a short user-supplied label.
///
/// Control characters and invisible format characters become spaces. Runs of
/// whitespace collapse to one space, and the text is capped at `limit`
/// characters. Returns `None` when nothing remains.
pub(crate) fn clean_label(value: &str, limit: usize) -> Option<String> {
    let collapsed = value
        .chars()
        .map(|character| {
            if character.is_control() || is_invisible_format(character) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(limit).collect())
}

/// Returns true for characters that render as nothing or can reorder text.
pub(crate) fn is_invisible_format(character: char) -> bool {
    let code = character as u32;
    matches!(
        code,
        0x00AD | 0x061C | 0x06DD | 0x070F | 0x08E2 | 0x180E | 0xFEFF | 0x110BD | 0x110CD | 0xE0001
    ) || (0x0600..=0x0605).contains(&code)
        || (0x0890..=0x0891).contains(&code)
        || (0x200B..=0x200F).contains(&code)
        || (0x202A..=0x202E).contains(&code)
        || (0x2060..=0x2064).contains(&code)
        || (0x2066..=0x206F).contains(&code)
        || (0xFFF9..=0xFFFB).contains(&code)
        || (0x13430..=0x1343F).contains(&code)
        || (0x1BCA0..=0x1BCA3).contains(&code)
        || (0x1D173..=0x1D17A).contains(&code)
        || (0xE0020..=0xE007F).contains(&code)
        || (0xE000..=0xF8FF).contains(&code)
        || (0xF0000..=0xFFFFD).contains(&code)
        || (0x100000..=0x10FFFD).contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_label_collapses_control_and_invisible_characters() {
        assert_eq!(
            clean_label("  Phone\u{202e}\u{200b} \n 7F3A  ", 64).as_deref(),
            Some("Phone 7F3A")
        );
        assert_eq!(clean_label(" \u{200e} ", 64), None);
        assert_eq!(clean_label("abcdef", 3).as_deref(), Some("abc"));
    }
}
