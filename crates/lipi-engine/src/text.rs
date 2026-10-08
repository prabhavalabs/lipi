//! Plain text and Markdown inputs: encoding detection and decoding.

/// Decode bytes to a string: byte-order marks first, then strict UTF-8, then a statistical guess
/// (Windows code pages, Latin-1, ...).
pub fn decode(bytes: &[u8]) -> String {
    if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        let (s, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return s.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    let enc = det.guess(None, chardetng::Utf8Detection::Allow);
    let (s, _, _) = enc.decode(bytes);
    s.into_owned()
}

/// Turn plain text into Markdown: paragraphs separated by blank lines, Markdown metacharacters
/// at the start of a line escaped so they are not misread as structure.
pub fn text_to_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim_end();
        let starts_with_meta = t.trim_start().starts_with(['#', '>', '|']);
        if starts_with_meta {
            out.push('\\');
        }
        out.push_str(t);
        out.push('\n');
    }
    // Collapse runs of more than one blank line.
    let mut collapsed = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        collapsed.push_str(line);
        collapsed.push('\n');
    }
    collapsed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_utf16_and_cp1252() {
        assert_eq!(decode("ශ්‍රී ලංකා".as_bytes()), "ශ්‍රී ලංකා");
        let utf16: Vec<u8> =
            [0xFF, 0xFE].into_iter().chain("இலங்கை".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert_eq!(decode(&utf16), "இலங்கை");
        assert_eq!(decode(b"caf\xe9 \x93quoted\x94"), "café “quoted”");
    }

    #[test]
    fn text_paragraphs() {
        assert_eq!(text_to_markdown("a\n\n\n\nb\n# not a heading"), "a\n\nb\n\\# not a heading");
    }
}
