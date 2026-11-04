//! Golden test for PDF export (T5.5, F6-AC1, F6-AC2): render the fixture,
//! print it with the real WebKit service and inspect the PDF.
//!
//! Needs a display (WebKit runs a web process); CI runs it under Xvfb.
//! Without one the test is skipped with a message.

#![allow(clippy::unwrap_used)]

use std::path::Path;

use gtk::glib;
use ringmd::model::render::{self, MarkdownRenderer};
use ringmd::services::webkit::WebKitPdfService;
use ringmd::services::{PageSize, PdfOptions, PdfService};

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Export the fixture to `out_dir/fixture.pdf`, or `None` without a display.
fn export_fixture(out_dir: &Path) -> Option<std::path::PathBuf> {
    if gtk::init().is_err() {
        eprintln!("skipping: no display for WebKit (run under xvfb-run)");
        return None;
    }
    let markdown = std::fs::read_to_string(fixtures().join("preview.md")).unwrap();
    let page = render::pdf_page(&MarkdownRenderer::new().render(&markdown));
    let base = format!("{}/", glib::filename_to_uri(fixtures(), None).unwrap());
    std::fs::create_dir_all(out_dir).unwrap();
    let output = out_dir.join("fixture.pdf");
    let options = PdfOptions {
        page_size: PageSize::A4,
        margin_mm: 20.0,
    };
    glib::MainContext::default()
        .block_on(WebKitPdfService.export(&page, Some(&base), &options, &output))
        .unwrap();
    // Set RINGMD_KEEP_PDF=<path> to keep a copy for manual inspection.
    if let Some(keep) = std::env::var_os("RINGMD_KEEP_PDF") {
        std::fs::copy(&output, keep).unwrap();
    }
    Some(output)
}

fn out_dir(test: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ringmd-pdf-{}-{test}", std::process::id()))
}

/// URIs of all link annotations in the PDF.
fn link_uris(doc: &lopdf::Document) -> Vec<String> {
    let mut uris = Vec::new();
    for object in doc.objects.values() {
        if let Ok(dict) = object.as_dict()
            && let Ok(action) = dict.get(b"A").and_then(|a| a.as_dict())
            && let Ok(uri) = action.get(b"URI").and_then(|u| u.as_str())
        {
            uris.push(String::from_utf8_lossy(uri).into_owned());
        }
    }
    uris
}

#[test]
fn fixture_exports_to_a_valid_pdf() {
    let out_dir = out_dir("valid");
    let Some(output) = export_fixture(&out_dir) else {
        return;
    };

    // F6-AC1: a valid PDF with several A4 pages.
    let doc = lopdf::Document::load(&output).unwrap();
    let pages = doc.get_pages();
    assert!(
        pages.len() >= 2,
        "expected several pages, got {}",
        pages.len()
    );
    let (_, first) = pages.iter().next().unwrap();
    let media_box = doc
        .get_object(*first)
        .and_then(|o| o.as_dict())
        .and_then(|d| d.get(b"MediaBox"))
        .and_then(|b| b.as_array())
        .unwrap()
        .iter()
        .map(|n| n.as_float().unwrap_or_else(|_| n.as_i64().unwrap() as f32))
        .collect::<Vec<f32>>();
    // A4 is 595 × 842 points.
    assert!((media_box[2] - 595.0).abs() < 2.0, "{media_box:?}");
    assert!((media_box[3] - 842.0).abs() < 2.0, "{media_box:?}");

    // F6-AC2: white page background, checked on a rendering when poppler's
    // pdftoppm is available.
    if let Some(corner) = corner_pixel(&output, &out_dir) {
        assert_eq!(corner, [255, 255, 255], "page background is not white");
    } else {
        eprintln!("pdftoppm not found; skipping the background check");
    }

    std::fs::remove_dir_all(&out_dir).unwrap();
}

/// F6-AC3 (F6.R5). Known gap: WebKitGTK's Skia print backend (2.52) writes
/// no link annotations, so this fails; the print stylesheet prints link
/// addresses instead. Run with `--ignored` after a WebKitGTK upgrade.
#[test]
#[ignore = "WebKitGTK (Skia) PDFs have no link annotations yet; see docs/SPEC.md F6.R5"]
fn links_are_clickable() {
    let out_dir = out_dir("links");
    let Some(output) = export_fixture(&out_dir) else {
        return;
    };
    let doc = lopdf::Document::load(&output).unwrap();
    let uris = link_uris(&doc);
    std::fs::remove_dir_all(&out_dir).unwrap();
    assert!(
        uris.iter().any(|u| u.starts_with("https://example.org")),
        "no link annotation: {uris:?}"
    );
}

/// The top-left pixel of page 1, via `pdftoppm -ppm`.
fn corner_pixel(pdf: &Path, dir: &Path) -> Option<[u8; 3]> {
    let prefix = dir.join("page");
    let status = std::process::Command::new("pdftoppm")
        .args(["-r", "20", "-f", "1", "-l", "1", "-singlefile"])
        .arg(pdf)
        .arg(&prefix)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let ppm = std::fs::read(prefix.with_extension("ppm")).ok()?;
    // Header: "P6\n<w> <h>\n255\n", then RGB bytes.
    let mut newlines = 0;
    let start = ppm.iter().position(|&b| {
        if b == b'\n' {
            newlines += 1;
        }
        newlines == 3
    })? + 1;
    Some([ppm[start], ppm[start + 1], ppm[start + 2]])
}
