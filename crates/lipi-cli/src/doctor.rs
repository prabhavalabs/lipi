//! `lipi doctor`.

use anyhow::Result;
use lipi_sys::hardware::{Gpu, human_bytes};
use lipi_sys::profile::{Level, capabilities};
use lipi_sys::{Hardware, Profile, deps, paths};
use std::process::ExitCode;

pub fn run(json: bool) -> Result<ExitCode> {
    let hw = Hardware::probe();
    let findings = capabilities(&hw);
    let tesseract = deps::find_tesseract();
    let tessdata = deps::tessdata_state();
    let pdfium = deps::pdfium_library();
    let lexicon = paths::lexicon_file("si");
    let ready = tesseract.as_ref().is_some_and(|t| t.major() >= 4)
        && tessdata.iter().all(|s| s.present)
        && pdfium.is_some();
    let profile =
        if hw.total_gib() < 8.0 || hw.physical_cores <= 4 { Profile::Gentle } else { Profile::Balanced };

    if json {
        let v = serde_json::json!({
            "hardware": hw,
            "findings": findings,
            "dependencies": {
                "tesseract": tesseract,
                "tessdata_dir": paths::tessdata_dir(),
                "tessdata": tessdata,
                "pdfium": pdfium,
                "lexicon_si": lexicon.is_file().then_some(&lexicon),
            },
            "recommended_profile": profile,
            "workers": { "gentle": Profile::Gentle.workers(&hw), "balanced": Profile::Balanced.workers(&hw), "max": Profile::Max.workers(&hw) },
            "ready": ready,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(if ready { ExitCode::SUCCESS } else { ExitCode::from(3) });
    }

    println!("lipi {}  ·  data directory {}", env!("CARGO_PKG_VERSION"), paths::data_dir().display());
    println!();
    println!("Machine");
    println!("  os        {} ({})", hw.os, hw.arch);
    println!("  cpu       {}", hw.cpu);
    let gpu = match &hw.gpu {
        Gpu::AppleMetal { chip } => format!("{chip} (Metal, unified memory)"),
        Gpu::Nvidia { name, memory_mib } => format!("{name} ({memory_mib} MiB, CUDA)"),
        Gpu::None => "none detected".into(),
    };
    println!("  gpu       {gpu}");
    println!(
        "  memory    {} total, {} available",
        human_bytes(hw.total_memory),
        human_bytes(hw.available_memory)
    );
    println!();
    println!("Capabilities");
    for f in &findings {
        let mark = match f.level {
            Level::Ok => "✓",
            Level::Warn => "!",
            Level::Error => "✗",
        };
        println!("  {mark} {:<14} {}", f.subject, f.message);
    }
    println!();
    println!("Dependencies");
    match &tesseract {
        Some(t) if t.major() >= 4 => println!("  ✓ tesseract      {} ({})", t.version, t.path.display()),
        Some(t) => println!("  ✗ tesseract      {} is too old (need 4.1 or later)", t.version),
        None => println!("  ✗ tesseract      not found"),
    }
    for s in &tessdata {
        println!("  {} model          {}", if s.present { "✓" } else { "✗" }, s.name);
    }
    match &pdfium {
        Some(p) => println!("  ✓ pdfium         {}", p.display()),
        None => println!("  ✗ pdfium         not installed (needed to render PDF pages for OCR)"),
    }
    if lexicon.is_file() {
        println!("  ✓ lexicon si     {}", lexicon.display());
    } else {
        println!(
            "  · lexicon si     not built; `lipi lexicon build <text files>` enables Sinhala post-correction"
        );
    }
    println!();
    println!(
        "Profiles  gentle {} worker(s) · balanced {} · max {}   recommended: {}",
        Profile::Gentle.workers(&hw),
        Profile::Balanced.workers(&hw),
        Profile::Max.workers(&hw),
        profile.name()
    );
    if !ready {
        println!();
        println!("Run `lipi setup` to install what is missing.");
    }
    Ok(if ready { ExitCode::SUCCESS } else { ExitCode::from(3) })
}
