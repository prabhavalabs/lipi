//! Pinned downloads. Every asset lipi fetches is listed here with its SHA-256; nothing else is
//! downloaded, and nothing is used unless its checksum matches.

/// A downloadable file.
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    /// Short name shown to the user.
    pub name: &'static str,
    /// Download URL.
    pub url: &'static str,
    /// Expected SHA-256 (lower-case hex).
    pub sha256: &'static str,
    /// Size in bytes.
    pub bytes: u64,
    /// Path relative to the install directory.
    pub path: &'static str,
    /// Licence of the asset.
    pub license: &'static str,
}

/// Identifier of the pinned Tesseract model set (directory name under `tessdata/`).
pub const TESSDATA_SET: &str = "best-e12c65a";

macro_rules! tessdata {
    ($name:literal, $file:literal, $sha:literal, $bytes:literal) => {
        Asset {
            name: $name,
            url: concat!(
                "https://raw.githubusercontent.com/tesseract-ocr/tessdata_best/",
                "e12c65a915945e4c28e237a9b52bc4a8f39a0cec/",
                $file
            ),
            sha256: $sha,
            bytes: $bytes,
            path: $file,
            license: "Apache-2.0",
        }
    };
}

/// Tesseract `tessdata_best` models (LSTM, float): the most accurate published models.
pub const TESSDATA: &[Asset] = &[
    tessdata!(
        "English",
        "eng.traineddata",
        "8280aed0782fe27257a68ea10fe7ef324ca0f8d85bd2fd145d1c2b560bcb66ba",
        15400601
    ),
    tessdata!(
        "Sinhala",
        "sin.traineddata",
        "1d95b36e935c8c12cc890684fad15bd3ad8d904c3a6ffa2fccb81658eabd8454",
        8282713
    ),
    tessdata!(
        "Tamil",
        "tam.traineddata",
        "4b9ce85987f629dd31eaf87443a1646452a43cdf91fcf05e017382ad595dcb9e",
        6023201
    ),
    tessdata!(
        "Orientation and script",
        "osd.traineddata",
        "9cf5d576fcc47564f11265841e5ca839001e7e6f38ff7f7aacf46d15a96b00ff",
        10562727
    ),
];

/// Pinned PDFium release (bblanchon/pdfium-binaries).
pub const PDFIUM_TAG: &str = "chromium/8086";

macro_rules! pdfium {
    ($name:literal, $file:literal, $sha:literal, $bytes:literal) => {
        Asset {
            name: $name,
            url: concat!(
                "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8086/",
                $file
            ),
            sha256: $sha,
            bytes: $bytes,
            path: $file,
            license: "BSD-3-Clause (PDFium), Apache-2.0 (build scripts)",
        }
    };
}

/// PDFium archive for the running platform, if one is published.
pub fn pdfium_asset() -> Option<Asset> {
    let a = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => pdfium!(
            "PDFium (macOS arm64)",
            "pdfium-mac-arm64.tgz",
            "e98679e052c07edbb5a627980902abb823d4b3f35744d877bd21668bd9fc13ab",
            3521983
        ),
        ("macos", "x86_64") => pdfium!(
            "PDFium (macOS x64)",
            "pdfium-mac-x64.tgz",
            "933a85a138f6027243c56bff8676375c33ceeb767401389415ffc44d689ca85d",
            3717448
        ),
        ("linux", "x86_64") => pdfium!(
            "PDFium (Linux x64)",
            "pdfium-linux-x64.tgz",
            "588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935",
            3788766
        ),
        ("linux", "aarch64") => pdfium!(
            "PDFium (Linux arm64)",
            "pdfium-linux-arm64.tgz",
            "e7e2fe4686925618330103cb167950aca5a84bb00fd977a41b86be59dd1480a2",
            3708875
        ),
        ("windows", "x86_64") => pdfium!(
            "PDFium (Windows x64)",
            "pdfium-win-x64.tgz",
            "1fd8af952832dbb0eb16d9249f68fe09e5f5ebf7c3dd9f6066ea2720cc28487d",
            3866531
        ),
        ("windows", "aarch64") => pdfium!(
            "PDFium (Windows arm64)",
            "pdfium-win-arm64.tgz",
            "1799b8034e6d64946fec0ae79f3edfc8ba58ccd80c70a46194803b1eba408344",
            3636496
        ),
        _ => return None,
    };
    Some(a)
}
