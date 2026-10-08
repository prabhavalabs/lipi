//! HTML to Markdown, keeping headings, lists, tables, preformatted text and links.

use scraper::{ElementRef, Html, Node};

const SKIP: &[&str] = &[
    "script", "style", "noscript", "nav", "header", "footer", "aside", "form", "button", "iframe", "svg",
    "head", "template", "select", "input", "canvas", "object", "embed",
];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "main",
    "body",
    "html",
    "center",
    "figure",
    "figcaption",
    "dl",
    "dt",
    "dd",
    "address",
    "details",
    "summary",
    "font",
];

/// Convert an HTML document to Markdown.
pub fn html_to_markdown(html: &str) -> (String, Option<String>) {
    let doc = Html::parse_document(html);
    let title = doc
        .select(&scraper::Selector::parse("title").expect("valid selector"))
        .next()
        .map(|t| collapse(&t.text().collect::<String>()))
        .filter(|t| !t.is_empty());
    let mut out = Vec::new();
    block(doc.root_element(), &mut out);
    let md = out
        .into_iter()
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (md, title)
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn name(e: &ElementRef) -> String {
    e.value().name().to_ascii_lowercase()
}

/// Inline content of an element as Markdown on one line.
fn inline(e: ElementRef) -> String {
    let mut s = String::new();
    for child in e.children() {
        match child.value() {
            Node::Text(t) => s.push_str(t),
            Node::Element(_) => {
                let Some(c) = ElementRef::wrap(child) else { continue };
                let n = name(&c);
                if SKIP.contains(&n.as_str()) {
                    continue;
                }
                match n.as_str() {
                    "br" => s.push('\n'),
                    "strong" | "b" => wrap(&mut s, &inline(c), "**"),
                    "em" | "i" => wrap(&mut s, &inline(c), "*"),
                    "code" | "kbd" | "samp" => wrap(&mut s, &inline(c), "`"),
                    "a" => {
                        let text = collapse(&inline(c));
                        match c.value().attr("href").filter(|h| h.starts_with("http")) {
                            Some(h) if !text.is_empty() => s.push_str(&format!("[{text}]({h})")),
                            _ => s.push_str(&text),
                        }
                    }
                    "img" => {}
                    _ => s.push_str(&inline(c)),
                }
            }
            _ => {}
        }
    }
    s
}

fn wrap(s: &mut String, inner: &str, mark: &str) {
    let t = collapse(inner);
    if t.is_empty() {
        s.push_str(if inner.is_empty() { "" } else { " " });
        return;
    }
    // Whitespace inside the element moves outside the markers: `<b> x </b>` -> ` **x** `.
    if inner.starts_with(char::is_whitespace) {
        s.push(' ');
    }
    s.push_str(&format!("{mark}{t}{mark}"));
    if inner.ends_with(char::is_whitespace) {
        s.push(' ');
    }
}

fn clean_inline(s: &str) -> String {
    s.split('\n').map(collapse).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("  \n")
}

/// Block-level conversion, appending Markdown blocks to `out`.
fn block(e: ElementRef, out: &mut Vec<String>) {
    let mut pending = String::new();
    let flush = |pending: &mut String, out: &mut Vec<String>| {
        let t = clean_inline(pending);
        if !t.is_empty() {
            out.push(t);
        }
        pending.clear();
    };
    for child in e.children() {
        match child.value() {
            Node::Text(t) => pending.push_str(t),
            Node::Element(_) => {
                let Some(c) = ElementRef::wrap(child) else { continue };
                let n = name(&c);
                if SKIP.contains(&n.as_str()) {
                    continue;
                }
                match n.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        flush(&mut pending, out);
                        let level = n[1..].parse::<usize>().unwrap_or(1);
                        let t = collapse(&inline(c));
                        if !t.is_empty() {
                            out.push(format!("{} {t}", "#".repeat(level)));
                        }
                    }
                    "ul" | "ol" => {
                        flush(&mut pending, out);
                        out.push(list(c, n == "ol", 0));
                    }
                    "table" => {
                        flush(&mut pending, out);
                        out.push(table(c));
                    }
                    "pre" => {
                        flush(&mut pending, out);
                        let t: String = c.text().collect();
                        out.push(format!("```\n{}\n```", t.trim_end()));
                    }
                    "blockquote" => {
                        flush(&mut pending, out);
                        let mut inner = Vec::new();
                        block(c, &mut inner);
                        let q = inner
                            .join("\n\n")
                            .lines()
                            .map(|l| format!("> {l}"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        out.push(q);
                    }
                    "hr" => {
                        flush(&mut pending, out);
                        out.push("---".into());
                    }
                    "br" => pending.push('\n'),
                    n if BLOCK.contains(&n) || n == "li" => {
                        flush(&mut pending, out);
                        block(c, out);
                    }
                    _ => {
                        // Inline element inside a block: keep accumulating.
                        let mut tmp = String::new();
                        match n.as_str() {
                            "strong" | "b" => wrap(&mut tmp, &inline(c), "**"),
                            "em" | "i" => wrap(&mut tmp, &inline(c), "*"),
                            _ => tmp = inline(c),
                        }
                        if c.children().any(|g| {
                            ElementRef::wrap(g).is_some_and(|ge| {
                                let gn = name(&ge);
                                BLOCK.contains(&gn.as_str())
                                    || matches!(
                                        gn.as_str(),
                                        "table"
                                            | "ul"
                                            | "ol"
                                            | "h1"
                                            | "h2"
                                            | "h3"
                                            | "h4"
                                            | "h5"
                                            | "h6"
                                            | "pre"
                                    )
                            })
                        }) {
                            flush(&mut pending, out);
                            block(c, out);
                        } else {
                            pending.push_str(&tmp);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    flush(&mut pending, out);
}

fn list(e: ElementRef, ordered: bool, depth: usize) -> String {
    let mut lines = Vec::new();
    let mut i = 1;
    for child in e.children().filter_map(ElementRef::wrap) {
        if name(&child) != "li" {
            continue;
        }
        let marker = if ordered { format!("{i}.") } else { "-".to_string() };
        i += 1;
        let mut text = String::new();
        let mut nested = Vec::new();
        for g in child.children() {
            match g.value() {
                Node::Text(t) => text.push_str(t),
                Node::Element(_) => {
                    let Some(ge) = ElementRef::wrap(g) else { continue };
                    let gn = name(&ge);
                    if gn == "ul" || gn == "ol" {
                        nested.push(list(ge, gn == "ol", depth + 1));
                    } else if !SKIP.contains(&gn.as_str()) {
                        text.push_str(&inline(ge));
                    }
                }
                _ => {}
            }
        }
        lines.push(format!("{}{marker} {}", "  ".repeat(depth), collapse(&text)));
        lines.extend(nested);
    }
    lines.join("\n")
}

fn cell_text(c: ElementRef) -> String {
    collapse(&inline(c)).replace('|', "\\|")
}

fn table(e: ElementRef) -> String {
    let mut rows: Vec<(bool, Vec<String>)> = Vec::new();
    fn collect_rows(e: ElementRef, rows: &mut Vec<(bool, Vec<String>)>) {
        for child in e.children().filter_map(ElementRef::wrap) {
            match name(&child).as_str() {
                "tr" => {
                    let mut cells = Vec::new();
                    let mut header = true;
                    for cell in child.children().filter_map(ElementRef::wrap) {
                        let n = name(&cell);
                        if n == "td" || n == "th" {
                            header &= n == "th";
                            let span = cell
                                .value()
                                .attr("colspan")
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(1)
                                .clamp(1, 20);
                            cells.push(cell_text(cell));
                            for _ in 1..span {
                                cells.push(String::new());
                            }
                        }
                    }
                    if !cells.is_empty() {
                        rows.push((header, cells));
                    }
                }
                "thead" | "tbody" | "tfoot" => collect_rows(child, rows),
                _ => {}
            }
        }
    }
    collect_rows(e, &mut rows);
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(1);
    let mut lines = Vec::new();
    let header_first = rows[0].0;
    let fmt = |r: &Vec<String>| {
        let mut r = r.clone();
        r.resize(width, String::new());
        format!("| {} |", r.join(" | "))
    };
    if header_first {
        lines.push(fmt(&rows[0].1));
    } else {
        lines.push(fmt(&vec![String::new(); width]));
    }
    lines.push(format!("|{}|", vec!["---"; width].join("|")));
    for (i, (_, r)) in rows.iter().enumerate() {
        if i == 0 && header_first {
            continue;
        }
        lines.push(fmt(r));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_paragraphs_lists_tables() {
        let html = r#"<html><head><title>Act No. 40</title><style>x{}</style></head><body>
            <nav>menu</nav><h2>Short title</h2><p>This Act may be cited as the <b>Law</b>.</p>
            <ul><li>one</li><li>two<ol><li>a</li></ol></li></ul>
            <table><tr><th>Section</th><th>Title</th></tr><tr><td>1</td><td>Short | title</td></tr></table>
            <p>Present:<b> Swan J. </b>Appellant</p><pre>  code
  block</pre><p>ශ්‍රී ලංකා</p></body></html>"#;
        let (md, title) = html_to_markdown(html);
        assert_eq!(title.as_deref(), Some("Act No. 40"));
        assert!(!md.contains("menu"));
        assert!(md.contains("## Short title"));
        assert!(md.contains("This Act may be cited as the **Law**."), "{md}");
        assert!(md.contains("- one\n- two\n  1. a"));
        assert!(md.contains("| Section | Title |\n|---|---|\n| 1 | Short \\| title |"));
        assert!(md.contains("Present: **Swan J.** Appellant"), "{md}");
        assert!(md.contains("```\n  code\n  block\n```"));
        assert!(md.ends_with("ශ්‍රී ලංකා"));
    }
}
