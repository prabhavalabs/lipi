//! End-to-end extraction of inputs that need no OCR engine.

use lipi_core::{InputFormat, Method, OutputFormat, RenderOptions, render};
use lipi_engine::{ExtractConfig, Extractor};
use lipi_script::Lang;
use std::path::PathBuf;

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn html_text_and_markdown_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let ex = Extractor::new(ExtractConfig::default());
    let quiet = |_e: lipi_engine::Event| {};

    let html = write(
        dir.path(),
        "act.html",
        "<html><head><title>Act</title></head><body><h1>ශ්‍රී ලංකා</h1><p>இலங்கை</p>\
         <table><tr><th>No.</th><th>Title</th></tr><tr><td>1</td><td>Short title</td></tr></table></body></html>"
            .as_bytes(),
    );
    let doc = ex.extract_path(&html, &quiet).unwrap();
    assert_eq!(doc.format, InputFormat::Html);
    assert_eq!(doc.metadata.get("title").map(String::as_str), Some("Act"));
    let md = render(&doc, OutputFormat::Markdown, RenderOptions::default());
    assert!(md.starts_with("# ශ්‍රී ලංකා\n\nஇலங்கை\n\n| No. | Title |"), "{md}");
    assert_eq!(doc.pages[0].method, Method::Markup);
    assert!(doc.langs().contains(&Lang::Si) && doc.langs().contains(&Lang::Ta));

    // cp1252 plain text with a stray OCR joiner is decoded and normalised.
    let txt = write(dir.path(), "notes.txt", b"caf\xe9 \x93quoted\x94\r\n\r\n\r\nnext");
    let doc = ex.extract_path(&txt, &quiet).unwrap();
    assert_eq!(doc.format, InputFormat::Text);
    assert_eq!(doc.pages[0].markdown, "café “quoted”\n\nnext");

    let md_in = write(dir.path(), "x.md", "# Title\n\nතීන්දුවක්\u{200C} මත".as_bytes());
    let doc = ex.extract_path(&md_in, &quiet).unwrap();
    assert_eq!(doc.pages[0].markdown, "# Title\n\nතීන්දුවක් මත");

    let json = render(&doc, OutputFormat::Json, RenderOptions::default());
    assert!(json.contains("\"kind\": \"markup\""));
}

#[test]
fn legacy_font_text_in_markup_is_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let ex = Extractor::new(ExtractConfig::default());
    let fm =
        "Y%S ,xld m%cd;dka;%sl iudcjd§ ckrcfha w;s úfYI .eiÜ m;%h wxl 2256$28 – 2021 fkdjeïn¾ ui 30 jeks "
            .repeat(5);
    let p = write(dir.path(), "old.txt", fm.as_bytes());
    let doc = ex.extract_path(&p, &|_| {}).unwrap();
    assert!(doc.pages[0].flags.contains(&"legacy_font_text".to_string()));
    assert_eq!(doc.warnings.len(), 1);
}

#[test]
fn unsupported_binary_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "blob.bin", &[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let ex = Extractor::new(ExtractConfig::default());
    assert!(ex.extract_path(&p, &|_| {}).is_err());
}
