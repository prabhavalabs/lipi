//! Output renderers: Markdown (default), JSON and plain text.

use crate::model::Document;
use std::str::FromStr;

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Markdown.
    #[default]
    Markdown,
    /// JSON with per-page provenance.
    Json,
    /// Plain text.
    Text,
}

impl OutputFormat {
    /// File extension for the format.
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Markdown => "md",
            OutputFormat::Json => "json",
            OutputFormat::Text => "txt",
        }
    }
}

impl FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Ok(OutputFormat::Markdown),
            "json" => Ok(OutputFormat::Json),
            "txt" | "text" => Ok(OutputFormat::Text),
            other => Err(format!("unknown output format `{other}` (use md, json or txt)")),
        }
    }
}

/// Rendering options.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    /// Insert `<!-- page N -->` markers between pages (Markdown only).
    pub page_markers: bool,
}

/// Render a document.
pub fn render(doc: &Document, format: OutputFormat, opts: RenderOptions) -> String {
    match format {
        OutputFormat::Markdown => markdown(doc, opts),
        OutputFormat::Json => serde_json::to_string_pretty(doc).expect("document serialises"),
        OutputFormat::Text => plain_text(&markdown(doc, RenderOptions::default())),
    }
}

fn markdown(doc: &Document, opts: RenderOptions) -> String {
    let mut out = String::new();
    for p in &doc.pages {
        let body = p.markdown.trim();
        if opts.page_markers {
            out.push_str(&format!("<!-- page {} -->\n\n", p.number));
        }
        if !body.is_empty() {
            out.push_str(body);
            out.push_str("\n\n");
        }
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() { String::new() } else { format!("{trimmed}\n") }
}

/// Strip Markdown syntax that has no meaning in plain text.
fn plain_text(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for line in md.lines() {
        let t = line.trim_start();
        let t = t.trim_start_matches('#').trim_start();
        if t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            continue; // table separator row
        }
        let t = t.replace("**", "").replace("__", "").replace("<u>", "").replace("</u>", "");
        let t = if t.starts_with('|') {
            t.trim_matches('|').split('|').map(str::trim).collect::<Vec<_>>().join("\t")
        } else {
            t
        };
        out.push_str(&t);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::InputFormat;
    use crate::model::{Method, Page};

    fn doc() -> Document {
        let mut d = Document::new("x.pdf", InputFormat::Pdf);
        let mut p = Page::new(1, Method::TextLayer);
        p.markdown = "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |".into();
        d.pages.push(p);
        let mut p = Page::new(2, Method::Empty);
        p.markdown = String::new();
        d.pages.push(p);
        d
    }

    #[test]
    fn markdown_and_markers() {
        let md = render(&doc(), OutputFormat::Markdown, RenderOptions::default());
        assert!(md.starts_with("# Title") && md.ends_with("| 1 | 2 |\n"));
        let md = render(&doc(), OutputFormat::Markdown, RenderOptions { page_markers: true });
        assert!(md.contains("<!-- page 2 -->"));
    }

    #[test]
    fn text_and_json() {
        let t = render(&doc(), OutputFormat::Text, RenderOptions::default());
        assert_eq!(t, "Title\n\na\tb\n1\t2\n");
        let j = render(&doc(), OutputFormat::Json, RenderOptions::default());
        assert!(j.contains("\"kind\": \"text_layer\""));
    }
}
