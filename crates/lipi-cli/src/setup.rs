//! `lipi setup`.

use anyhow::{Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use lipi_sys::hardware::human_bytes;
use lipi_sys::manifest::{TESSDATA, pdfium_asset};
use lipi_sys::{deps, install, paths};
use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, ExitCode};

fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        bail!("not running interactively; re-run with --yes to proceed");
    }
    eprint!("{question} [Y/n] ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes"))
}

fn bar(len: u64, name: &str) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template("  {msg:<32} [{bar:30}] {bytes}/{total_bytes}")
            .expect("valid template")
            .progress_chars("=> "),
    );
    pb.set_message(name.to_string());
    pb
}

pub fn run(yes: bool, skip_engine: bool) -> Result<ExitCode> {
    let tesseract = deps::find_tesseract();
    let missing_models: Vec<_> = deps::tessdata_state().into_iter().filter(|s| !s.present).collect();
    let pdfium = deps::pdfium_library();

    let mut plan: Vec<String> = Vec::new();
    let engine_cmd = if !skip_engine && tesseract.as_ref().is_none_or(|t| t.major() < 4) {
        let cmd = install::tesseract_install_plan();
        match &cmd {
            Some(c) => plan.push(format!("install the Tesseract OCR engine: `{}`", c.join(" "))),
            None => plan.push("install the Tesseract OCR engine manually (no supported package manager found): https://tesseract-ocr.github.io/tessdoc/Installation.html".into()),
        }
        cmd
    } else {
        None
    };
    if !missing_models.is_empty() {
        let bytes: u64 = TESSDATA
            .iter()
            .filter(|a| missing_models.iter().any(|m| m.name == a.name))
            .map(|a| a.bytes)
            .sum();
        plan.push(format!(
            "download {} OCR model(s) ({}) into {}",
            missing_models.len(),
            human_bytes(bytes),
            paths::tessdata_dir().display()
        ));
    }
    if pdfium.is_none() {
        match pdfium_asset() {
            Some(a) => plan.push(format!(
                "download the PDFium renderer ({}) into {}",
                human_bytes(a.bytes),
                paths::pdfium_dir().display()
            )),
            None => plan
                .push("PDFium is not published for this platform; set LIPI_PDFIUM to a local build".into()),
        }
    }
    if plan.is_empty() {
        println!("Everything is installed. `lipi doctor` shows the details.");
        return Ok(ExitCode::SUCCESS);
    }
    println!("lipi setup will:");
    for p in &plan {
        println!("  • {p}");
    }
    println!("All downloads are pinned and verified by SHA-256. After setup lipi runs fully offline.");
    if !confirm("Proceed?", yes)? {
        println!("Nothing changed.");
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(cmd) = engine_cmd {
        println!("→ {}", cmd.join(" "));
        let status = Command::new(&cmd[0]).args(&cmd[1..]).status()?;
        if !status.success() {
            bail!("installing Tesseract failed ({status}); install it manually and run `lipi setup` again");
        }
    }
    if !missing_models.is_empty() {
        println!("→ OCR models");
        let mut current: Option<(&str, ProgressBar)> = None;
        install::install_tessdata(|asset, done, total| {
            if current.as_ref().is_none_or(|(n, _)| *n != asset.name) {
                if let Some((_, pb)) = current.take() {
                    pb.finish();
                }
                current = Some((asset.name, bar(total, asset.name)));
            }
            if let Some((_, pb)) = &current {
                pb.set_position(done);
            }
        })?;
        if let Some((_, pb)) = current {
            pb.finish();
        }
    }
    if pdfium.is_none() && pdfium_asset().is_some() {
        println!("→ PDFium");
        let mut pb: Option<ProgressBar> = None;
        let lib = install::install_pdfium(|asset, done, total| {
            let b = pb.get_or_insert_with(|| bar(total, asset.name));
            b.set_position(done);
        })?;
        if let Some(b) = pb {
            b.finish();
        }
        println!("  installed {}", lib.display());
    }
    println!("Done. Run `lipi doctor` to check, then `lipi extract <file>`.");
    Ok(ExitCode::SUCCESS)
}
