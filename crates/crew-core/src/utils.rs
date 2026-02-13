//! Shared utility functions.

/// Truncate a string in-place at a UTF-8 safe boundary, appending a suffix.
///
/// Does nothing if `s.len() <= max_len`.
pub fn truncate_utf8(s: &mut String, max_len: usize, suffix: &str) {
    if s.len() <= max_len {
        return;
    }
    let mut limit = max_len;
    while limit > 0 && !s.is_char_boundary(limit) {
        limit -= 1;
    }
    s.truncate(limit);
    s.push_str(suffix);
}

/// Return a truncated copy of `s` at a UTF-8 safe boundary with suffix appended.
///
/// Returns the original string unchanged if `s.len() <= max_len`.
pub fn truncated_utf8(s: &str, max_len: usize, suffix: &str) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut limit = max_len;
    while limit > 0 && !s.is_char_boundary(limit) {
        limit -= 1;
    }
    format!("{}{}", &s[..limit], suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_no_op() {
        let mut s = "hello".to_string();
        truncate_utf8(&mut s, 10, "...");
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_truncate_ascii() {
        let mut s = "abcdefghij".to_string();
        truncate_utf8(&mut s, 5, "...");
        assert_eq!(s, "abcde...");
    }

    #[test]
    fn test_truncate_utf8_boundary() {
        // 你好世 = 9 bytes, truncate at 7 should back up to byte 6
        let mut s = "\u{4F60}\u{597D}\u{4E16}".to_string();
        truncate_utf8(&mut s, 7, "...");
        assert_eq!(s, "\u{4F60}\u{597D}...");
    }

    #[test]
    fn test_truncated_utf8_no_op() {
        assert_eq!(truncated_utf8("hello", 10, "..."), "hello");
    }

    #[test]
    fn test_truncated_utf8_ascii() {
        assert_eq!(truncated_utf8("abcdefghij", 5, "..."), "abcde...");
    }

    #[test]
    fn test_truncated_utf8_boundary() {
        let s = "\u{4F60}\u{597D}\u{4E16}"; // 9 bytes
        assert_eq!(truncated_utf8(s, 7, "..."), "\u{4F60}\u{597D}...");
    }
}
