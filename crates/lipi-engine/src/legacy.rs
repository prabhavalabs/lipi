//! Converting legacy-font text layers without touching the Latin text next to them.
//!
//! pdf-inspector produces page Markdown (headings, tables, emphasis) but no per-span font
//! information, while a gazette page mixes FM Abhaya body text with Times or Calibri numbers,
//! dates and English words. Converting the whole page would turn `1A` or `No.` into Sinhala
//! glyph soup. lipi therefore reads every character's font with PDFium, collects the words
//! drawn with a legacy font and the words drawn with any other font, and converts a Markdown
//! token only when the text layer shows it was drawn with the legacy font.
//!
//! Words that mix fonts (a Sinhala word with a colon or a Latin reference number set in Times
//! and no space between) are converted glyph run by glyph run. Tokens pdf-inspector joined or
//! split differently from PDFium are looked up again without their edge punctuation, and
//! tokens that still match nothing follow the page majority.

use crate::pdf::pdfium;
use lipi_script::fonts::legacy_font;
use lipi_script::legacy::Converter;
use pdfium_render::prelude::*;
use std::collections::{HashMap, HashSet};

/// Words of one page grouped by the kind of font that drew them.
#[derive(Debug, Default, Clone)]
pub struct FontWords {
    /// Words every glyph of which is drawn with a legacy font.
    pub legacy: HashSet<String>,
    /// Words drawn with any other font.
    pub other: HashSet<String>,
    /// Words mixing both kinds, with one flag per character: `true` for a legacy-font glyph.
    pub mixed: HashMap<String, Vec<bool>>,
    /// Distinct font names on the page, as PDFium reports them.
    pub fonts: Vec<String>,
    /// The page text as PDFium extracts it (lines separated by newlines), a fallback source when
    /// pdf-inspector withholds a text layer it suspects of being garbled.
    pub text: String,
    /// Non-space glyphs drawn with a legacy font.
    pub legacy_chars: usize,
    /// Non-space glyphs drawn with other fonts.
    pub other_chars: usize,
}

impl FontWords {
    /// Share of glyphs drawn with a legacy font.
    pub fn legacy_share(&self) -> f32 {
        let n = self.legacy_chars + self.other_chars;
        if n == 0 { 0.0 } else { self.legacy_chars as f32 / n as f32 }
    }
}

/// Read per-font words for the given pages (0-based) with PDFium. Pages that cannot be read are
/// absent from the result.
pub fn font_words(bytes: &[u8], indices: &[usize]) -> HashMap<usize, FontWords> {
    let mut out = HashMap::new();
    let Ok(pdfium) = pdfium() else { return out };
    let Ok(doc) = pdfium.load_pdf_from_byte_slice(bytes, None) else { return out };
    for &i in indices {
        let Ok(page) = doc.pages().get(i as PdfPageIndex) else { continue };
        let Ok(text) = page.text() else { continue };
        let mut words = FontWords::default();
        let mut is_legacy_font: HashMap<String, bool> = HashMap::new();
        let mut cur = String::new();
        let mut flags: Vec<bool> = Vec::new();
        let flush = |cur: &mut String, flags: &mut Vec<bool>, words: &mut FontWords| {
            if !cur.is_empty() {
                let w = std::mem::take(cur);
                if flags.iter().all(|&f| f) {
                    words.legacy.insert(w);
                } else if flags.iter().all(|&f| !f) {
                    words.other.insert(w);
                } else {
                    words.mixed.insert(w, std::mem::take(flags));
                }
            }
            flags.clear();
        };
        // PDFium inserts "generated" whitespace between glyphs that are merely spaced apart,
        // which splits `Y%S` into `Y %S`. A generated break only counts when the gap is wide or
        // the baseline moves; a real space or newline glyph always counts.
        #[derive(Clone, Copy, PartialEq)]
        enum Break {
            None,
            Soft,
            Hard(char),
        }
        let mut pending = Break::None;
        let mut prev: Option<(f32, f32, f32)> = None; // right edge, bottom, font size
        for ch in text.chars().iter() {
            match ch.unicode_char() {
                Some(c) if !c.is_whitespace() && !c.is_control() => {
                    let bounds = ch.loose_bounds().ok();
                    let size = ch.scaled_font_size().value.max(1.0);
                    let mut sep = match pending {
                        Break::None => None,
                        Break::Hard(sep) => Some(sep),
                        Break::Soft => match (prev, &bounds) {
                            (Some((right, bottom, psize)), Some(b)) => {
                                let gap = b.left().value - right;
                                let moved = (b.bottom().value - bottom).abs() > 0.5 * psize;
                                if moved {
                                    Some('\n')
                                } else if gap > 0.2 * psize {
                                    Some(' ')
                                } else {
                                    None
                                }
                            }
                            _ => Some(' '),
                        },
                    };
                    if words.text.is_empty() {
                        sep = None;
                    }
                    if let Some(sep) = sep {
                        flush(&mut cur, &mut flags, &mut words);
                        words.text.push(sep);
                    }
                    pending = Break::None;
                    let name = ch.font_name();
                    if !is_legacy_font.contains_key(&name) {
                        words.fonts.push(name.clone());
                    }
                    let legacy = *is_legacy_font.entry(name).or_insert_with_key(|n| legacy_font(n).is_some());
                    cur.push(c);
                    flags.push(legacy);
                    words.text.push(c);
                    if legacy {
                        words.legacy_chars += 1;
                    } else {
                        words.other_chars += 1;
                    }
                    if let Some(b) = bounds {
                        prev = Some((b.right().value, b.bottom().value, size));
                    }
                }
                c => {
                    let generated = ch.is_generated().unwrap_or(false);
                    let newline = matches!(c, Some('\n') | Some('\r'));
                    pending = match (pending, generated) {
                        (Break::Hard('\n'), _) => Break::Hard('\n'),
                        (_, false) if newline => Break::Hard('\n'),
                        (_, false) => Break::Hard(' '),
                        (Break::Hard(s), true) => Break::Hard(s),
                        (_, true) => Break::Soft,
                    };
                }
            }
        }
        flush(&mut cur, &mut flags, &mut words);
        out.insert(i, words);
    }
    out
}

/// Counts from [`convert_markdown`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConvertStats {
    /// Tokens converted.
    pub converted: usize,
    /// Tokens kept because another font drew them.
    pub kept: usize,
    /// Tokens matched by neither set, decided by the page majority.
    pub unmatched: usize,
}

const LEAD_MARKUP: &[&str] = &["<u>", "**", "*", "#", ">", "`", "~~"];
const TRAIL_MARKUP: &[&str] = &["</u>", "**", "*", "`", "~~"];
/// Markup that may sit inside a token (`**fldgi**:`); a token is split at these.
const INNER_MARKUP: &[&str] = &["<u>", "</u>", "**", "~~", "`"];

/// Split a Markdown token into leading markup, the text itself and trailing markup.
fn peel_markup(token: &str) -> (&str, &str, &str) {
    let mut start = 0;
    'lead: loop {
        for m in LEAD_MARKUP {
            if token[start..].starts_with(m) {
                start += m.len();
                continue 'lead;
            }
        }
        break;
    }
    let mut end = token.len();
    'trail: loop {
        for m in TRAIL_MARKUP {
            if end > start && token[start..end].ends_with(m) && !token[start..end - m.len()].ends_with('\\') {
                end -= m.len();
                continue 'trail;
            }
        }
        break;
    }
    if end < start {
        return (token, "", "");
    }
    (&token[..start], &token[start..end], &token[end..])
}

/// Split a token at inner markup runs. Markup pieces are returned with `true`.
fn split_inner_markup(token: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut seg_start = 0;
    let mut i = 0;
    while i < token.len() {
        let escaped = i > 0 && token[..i].ends_with('\\');
        let hit = (!escaped).then(|| INNER_MARKUP.iter().find(|m| token[i..].starts_with(*m))).flatten();
        if let Some(m) = hit {
            if seg_start < i {
                out.push((&token[seg_start..i], false));
            }
            out.push((&token[i..i + m.len()], true));
            i += m.len();
            seg_start = i;
        } else {
            i += token[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    if seg_start < token.len() {
        out.push((&token[seg_start..], false));
    }
    out
}

/// Remove the backslashes pdf-inspector adds before Markdown-significant punctuation.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(|n| n.is_ascii_punctuation()) {
            continue;
        }
        out.push(c);
    }
    out
}

/// Split a table row at its unescaped cell delimiters, keeping the delimiters as items.
fn split_cells(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in line.char_indices() {
        if c == '|' && !line[..i].ends_with('\\') {
            out.push(&line[start..i]);
            out.push(&line[i..i + 1]);
            start = i + 1;
        }
    }
    out.push(&line[start..]);
    out
}

#[derive(Debug, PartialEq, Eq)]
enum Class<'a> {
    Legacy,
    Other,
    Mixed(&'a [bool]),
    Unknown,
}

fn classify<'a>(words: &'a FontWords, text: &str) -> Class<'a> {
    if words.legacy.contains(text) {
        Class::Legacy // also when another font drew the same word: digits convert to themselves
    } else if words.other.contains(text) {
        Class::Other
    } else if let Some(flags) = words.mixed.get(text) {
        Class::Mixed(flags)
    } else {
        Class::Unknown
    }
}

/// Look a token up again with up to three characters cut from either end, for tokens that
/// pdf-inspector delimited differently from PDFium. Returns the class of the inner part and the
/// number of characters cut at each end; the smallest cut wins.
fn classify_trimmed<'a>(words: &'a FontWords, text: &str) -> Option<(Class<'a>, usize, usize)> {
    let cs: Vec<char> = text.chars().collect();
    for total in 1..=3usize {
        if total >= cs.len() {
            break;
        }
        for lead in 0..=total {
            let trail = total - lead;
            let inner: String = cs[lead..cs.len() - trail].iter().collect();
            match classify(words, &inner) {
                Class::Unknown => {}
                c => return Some((c, lead, trail)),
            }
        }
    }
    None
}

/// Convert only the legacy-font glyph runs of a word. `flags` has one entry per character.
fn convert_runs(text: &str, flags: &[bool], conv: &Converter) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;
    while i < cs.len() {
        let legacy = flags.get(i).copied().unwrap_or(true);
        let mut j = i;
        while j < cs.len() && flags.get(j).copied().unwrap_or(true) == legacy {
            j += 1;
        }
        let run: String = cs[i..j].iter().collect();
        if legacy {
            out.push_str(&conv.convert(&run))
        } else {
            out.push_str(&run)
        }
        i = j;
    }
    out
}

struct Ctx<'a> {
    words: Option<&'a FontWords>,
    conv: &'a Converter,
    majority_legacy: bool,
    stats: ConvertStats,
}

impl Ctx<'_> {
    /// Classify a piece of text by exact match, then by small edge cuts. `None` means unknown.
    fn lookup(&self, text: &str) -> Option<(Class<'_>, usize, usize)> {
        let w = self.words?;
        match classify(w, text) {
            Class::Unknown => classify_trimmed(w, text),
            c => Some((c, 0, 0)),
        }
    }

    /// Convert one piece of text (no whitespace, markup already removed) and append it.
    fn emit_text(&mut self, text: &str, out: &mut String) {
        if text.is_empty() || !text.chars().any(|c| self.conv.maps(c)) {
            out.push_str(text);
            return;
        }
        if self.words.is_none() {
            self.stats.converted += 1;
            out.push_str(&self.conv.convert(text));
            return;
        }
        match self.lookup(text) {
            Some((Class::Legacy, _, _)) => {
                self.stats.converted += 1;
                out.push_str(&self.conv.convert(text));
            }
            Some((Class::Other, _, _)) => {
                self.stats.kept += 1;
                out.push_str(text);
            }
            Some((Class::Mixed(flags), lead, trail)) => {
                // Cut edge characters have no font of their own: treat them like the page.
                let mut all = vec![true; lead];
                all.extend_from_slice(flags);
                all.extend(std::iter::repeat_n(true, trail));
                self.stats.converted += 1;
                out.push_str(&convert_runs(text, &all, self.conv));
            }
            Some((Class::Unknown, _, _)) | None => {
                self.stats.unmatched += 1;
                if self.majority_legacy && !text.chars().all(|c| c.is_ascii_punctuation()) {
                    out.push_str(&self.conv.convert(text));
                } else {
                    out.push_str(text);
                }
            }
        }
    }

    /// Convert one whitespace-delimited token: an exact text-layer match wins (a word may start
    /// with `*`, the ෆ code); otherwise markup is peeled off and the pieces are converted.
    fn emit_token(&mut self, token: &str, out: &mut String) {
        let plain = unescape(token);
        if matches!(self.lookup(&plain), Some((Class::Legacy | Class::Mixed(_), 0, 0))) {
            self.emit_text(&plain, out);
            return;
        }
        for (piece, is_markup) in split_inner_markup(token) {
            if is_markup {
                out.push_str(piece);
                continue;
            }
            let (lead, core, trail) = peel_markup(piece);
            out.push_str(lead);
            self.emit_text(&unescape(core), out);
            out.push_str(trail);
        }
    }

    /// Convert running text: tokens separated by whitespace, whitespace preserved.
    fn emit_span(&mut self, text: &str, out: &mut String) {
        let mut rest = text;
        while !rest.is_empty() {
            let ws = rest.len() - rest.trim_start().len();
            out.push_str(&rest[..ws]);
            rest = &rest[ws..];
            let n = rest.find(char::is_whitespace).unwrap_or(rest.len());
            if n > 0 {
                self.emit_token(&rest[..n], out);
            }
            rest = &rest[n..];
        }
    }
}

/// Convert the legacy-font tokens of a Markdown page. `words` is `None` when font information
/// is unavailable, in which case every token is converted. Table rows are converted cell by
/// cell, so `|` delimiters (also the code for ඳ) are never mistaken for text.
pub fn convert_markdown(md: &str, words: Option<&FontWords>, conv: &Converter) -> (String, ConvertStats) {
    let mut ctx = Ctx {
        words,
        conv,
        majority_legacy: words.is_none_or(|w| w.legacy_share() >= 0.5),
        stats: ConvertStats::default(),
    };
    let mut out = String::with_capacity(md.len() * 2);
    for line in md.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let newline = &line[body.len()..];
        if body.trim_start().starts_with('|') {
            let cells = split_cells(body);
            let separator = cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '|' | '-' | ':' | ' ')));
            if separator {
                out.push_str(body);
            } else {
                for cell in cells {
                    if cell == "|" {
                        out.push('|');
                    } else {
                        ctx.emit_span(cell, &mut out);
                    }
                }
            }
        } else {
            ctx.emit_span(body, &mut out);
        }
        out.push_str(newline);
    }
    (out, ctx.stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lipi_script::Lang;

    const T: &[(&str, &str)] = &[
        ("Y", "ශ"),
        ("%", "\u{0DCA}\u{200D}ර"),
        ("S", "ී"),
        (",", "ල"),
        ("x", "ං"),
        ("l", "ක"),
        ("d", "ා"),
        ("f", "\u{0DD9}"),
        ("g", "ට"),
        ("i", "ස"),
        ("'", "."),
        ("$", "/"),
        ("A", "ඇ"),
    ];

    fn conv() -> Converter {
        Converter::new("test", Lang::Si, T)
    }

    fn words() -> FontWords {
        let mut w = FontWords { legacy_chars: 90, other_chars: 10, ..Default::default() };
        for s in ["Y%S", ",xld", "fldgi", "2021'11'15", "2256$28"] {
            w.legacy.insert(s.into());
        }
        for s in ["1A", "III", "Act", "2021"] {
            w.other.insert(s.into());
        }
        // "fldgi:" with the colon set in Times; "$WS$" with a Latin reference inside FM text.
        w.mixed.insert("fldgi:".into(), vec![true, true, true, true, true, false]);
        w.mixed.insert("$WS$".into(), vec![true, false, false, true]);
        w
    }

    #[test]
    fn mixed_font_words_convert_only_their_legacy_runs() {
        let (out, stats) =
            convert_markdown("fldgi: $WS$\n|---|:---:|\n|fldgi|1A|Y%S ,xld|", Some(&words()), &conv());
        assert_eq!(out, "කොටස: /WS/\n|---|:---:|\n|කොටස|1A|ශ්‍රී ලංකා|");
        assert_eq!(stats.converted, 5);
        assert_eq!(stats.unmatched, 0);
    }

    #[test]
    fn exact_text_layer_words_beat_markup_and_inner_markup_is_split() {
        let mut w = words();
        w.legacy.insert("*ldgi".into()); // a word starting with the ෆ code
        w.legacy.insert("ms<sn|".into()); // a word ending with the ඳ code
        w.legacy.insert("fldgi".into());
        let t: &[(&str, &str)] = &[
            ("*", "ෆ"),
            ("|", "ඳ"),
            ("l", "ක"),
            ("d", "ා"),
            ("g", "ට"),
            ("i", "ස"),
            ("f", "\u{0DD9}"),
            ("m", "ප"),
            ("s", "ි"),
            ("<", "ළ"),
            ("n", "බ"),
            (":", "ථ"),
        ];
        let c = Converter::new("t", Lang::Si, t);
        let (out, _) = convert_markdown("*ldgi ms<sn| **fldgi**: :", Some(&w), &c);
        assert_eq!(out, "ෆකාටස පිළිබඳ **කොටස**: :");
    }

    #[test]
    fn converts_legacy_tokens_and_keeps_latin_font_tokens() {
        let md = "##### III fldgi – **Y%S ,xld** 2021'11'15\n\n1A Act 2021\n\n| fldgi | 1A |";
        let (out, stats) = convert_markdown(md, Some(&words()), &conv());
        assert_eq!(out, "##### III කොටස – **ශ්‍රී ලංකා** 2021.11.15\n\n1A Act 2021\n\n| කොටස | 1A |");
        assert_eq!(stats.converted, 5);
        assert_eq!(stats.kept, 3); // "1A" twice and "Act"; "III" and "2021" contain no mapped code
        assert_eq!(stats.unmatched, 0);
    }

    #[test]
    fn unknown_tokens_retry_without_edge_punctuation_then_follow_the_majority() {
        let (out, stats) = convert_markdown("Y%S\\\" ,xld: zzz", Some(&words()), &conv());
        assert_eq!(out, "ශ්‍රී\" ලංකා: zzz");
        assert_eq!(stats.unmatched, 0);
        let mut w = words();
        w.legacy_chars = 1;
        w.other_chars = 99;
        let (out, stats) = convert_markdown("YYY", Some(&w), &conv());
        assert_eq!(out, "YYY");
        assert_eq!(stats.unmatched, 1);
    }

    #[test]
    fn without_font_information_everything_is_converted() {
        let (out, _) = convert_markdown("<u>Y%S</u> 1A", None, &conv());
        assert_eq!(out, "<u>ශ්‍රී</u> 1ඇ");
    }

    #[test]
    fn peels_markup_but_not_escaped_literals() {
        assert_eq!(peel_markup("**Y%S**"), ("**", "Y%S", "**"));
        assert_eq!(peel_markup("<u>abc</u>"), ("<u>", "abc", "</u>"));
        assert_eq!(peel_markup("abc\\*"), ("", "abc\\*", ""));
        assert_eq!(split_cells("|a|b\\|c|"), vec!["", "|", "a", "|", "b\\|c", "|", ""]);
        assert_eq!(
            split_inner_markup("**a**b"),
            vec![("**", true), ("a", false), ("**", true), ("b", false)]
        );
        assert_eq!(unescape("a\\\"b\\*c\\d"), "a\"b*c\\d");
    }
}
