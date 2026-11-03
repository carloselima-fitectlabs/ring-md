//! Markdown → HTML for the preview and PDF export (T3.1, T3.2).
//!
//! comrak parses CommonMark + GFM and tags every block with
//! `data-sourcepos` (used for scroll sync, F4.R4). Fenced code is
//! highlighted by syntect as CSS classes, so the theme decides the colours.
//! Raw HTML is allowed through the parser and then sanitized with ammonia
//! (F4.R8): no scripts, styles, frames, forms or event handlers survive.

use std::collections::HashSet;
use std::sync::OnceLock;

use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Options, markdown_to_html_with_plugins};

/// The preview's base stylesheet (colours are CSS custom properties).
pub const PREVIEW_CSS: &str = include_str!("preview.css");

/// Content-Security-Policy for the preview page (F4.R10, N3): images may
/// come from the document's folder (`file:`), `https:` or `data:`; nothing
/// else (scripts, stylesheets, fonts, frames, connections) is loaded from
/// anywhere. RingMD's own patching goes through the WebView API, which this
/// policy does not restrict.
pub const PREVIEW_CSP: &str =
    "default-src 'none'; img-src file: https: data:; style-src 'unsafe-inline'";

/// Id of the element whose contents are the rendered document.
pub const ARTICLE_ID: &str = "ringmd-article";

/// Id of the `<style>` element holding the theme's custom properties.
pub const THEME_STYLE_ID: &str = "ringmd-theme";

/// The full preview page around already-rendered `article` HTML. `css` is
/// the base stylesheet; `theme_css` overrides its colours and is kept in
/// its own element so a theme switch can replace it in place.
pub fn preview_page(article: &str, css: &str, theme_css: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{PREVIEW_CSP}\">\n\
         <style>{css}</style>\n<style id=\"{THEME_STYLE_ID}\">{theme_css}</style></head>\n\
         <body><article id=\"{ARTICLE_ID}\">{article}</article></body></html>\n"
    )
}

/// Prefix for syntect's CSS classes, so they cannot clash with ours.
pub const CODE_CLASS_PREFIX: &str = "hl-";

/// Renders Markdown documents. Cheap to share: the syntax definitions are
/// loaded once per process.
#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownRenderer;

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self
    }

    /// Render `markdown` to sanitized HTML (the contents of `<article>`).
    pub fn render(&self, markdown: &str) -> String {
        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(highlighter());
        let html = markdown_to_html_with_plugins(markdown, &options(), &plugins);
        sanitizer().clean(&html).to_string()
    }
}

fn options() -> Options<'static> {
    let mut options = Options::default();
    let ext = &mut options.extension;
    ext.table = true;
    ext.tasklist = true;
    ext.strikethrough = true;
    ext.autolink = true;
    ext.footnotes = true;
    // Raw HTML reaches the sanitizer, which decides what is kept.
    options.render.r#unsafe = true;
    options.render.sourcepos = true;
    options.render.tasklist_classes = true;
    options
}

fn highlighter() -> &'static SyntectAdapter {
    static ADAPTER: OnceLock<SyntectAdapter> = OnceLock::new();
    ADAPTER.get_or_init(|| {
        SyntectAdapterBuilder::new()
            .css_with_class_prefix(CODE_CLASS_PREFIX)
            .build()
    })
}

fn sanitizer() -> &'static ammonia::Builder<'static> {
    static SANITIZER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    SANITIZER.get_or_init(|| {
        let mut builder = ammonia::Builder::default();
        builder
            // Block positions, footnote markers and highlighting classes.
            .add_generic_attributes(["class", "id", "align"])
            .add_generic_attribute_prefixes(["data-"])
            // GFM task list checkboxes; only read-only checkboxes survive.
            .add_tags(["input", "section"])
            .add_tag_attributes("input", ["type", "checked", "disabled"])
            .attribute_filter(|element, attribute, value| match (element, attribute) {
                ("input", "type") if value != "checkbox" => None,
                _ => Some(value.into()),
            })
            .set_tag_attribute_value("input", "disabled", "")
            .url_schemes(HashSet::from(["http", "https", "mailto", "file"]))
            .link_rel(Some("noopener noreferrer"));
        builder
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(markdown: &str) -> String {
        MarkdownRenderer::new().render(markdown)
    }

    #[test]
    fn blocks_carry_source_positions() {
        let html = render("# Title\n\nSome *text*.\n");
        assert!(
            html.contains(r#"<h1 data-sourcepos="1:1-1:7">Title</h1>"#),
            "{html}"
        );
        assert!(html.contains(r#"<p data-sourcepos="3:1-3:12">"#), "{html}");
        assert!(html.contains(">text</em>"), "{html}");
    }

    #[test]
    fn gfm_extensions() {
        let html = render(
            "| a | b |\n|:--|--:|\n| 1 | 2 |\n\n\
             - [x] done\n- [ ] open\n\n\
             ~~gone~~ https://example.org\n\n\
             Note[^1].\n\n[^1]: The footnote.\n",
        );
        assert!(html.contains("<table"), "{html}");
        assert!(html.contains(r#"align="left">a</th>"#), "{html}");
        assert!(html.contains(">gone</del>"), "{html}");
        assert!(
            html.contains(r#"href="https://example.org" rel="noopener noreferrer">"#),
            "{html}"
        );
        assert!(html.contains(r#"type="checkbox""#), "{html}");
        assert!(html.contains("checked"), "{html}");
        assert!(html.contains("task-list-item"), "{html}");
        assert!(html.contains(r#"class="footnotes""#), "{html}");
        assert!(html.contains("The footnote."), "{html}");
    }

    #[test]
    fn code_is_highlighted_with_classes() {
        let html = render("```rust\nfn main() {}\n```\n");
        assert!(
            html.contains(r#"class="hl-"#) || html.contains("hl-source"),
            "{html}"
        );
        assert!(html.contains("main"), "{html}");
        assert!(!html.contains("style="), "{html}");
    }

    #[test]
    fn unknown_languages_fall_back_to_plain_text() {
        let html = render("```nosuchlang\n<b>x</b>\n```\n");
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"), "{html}");
    }

    #[test]
    fn scripts_and_handlers_are_removed() {
        let html = render(
            "<script>alert(1)</script>\n\n\
             <img src=\"x.png\" onerror=\"alert(2)\">\n\n\
             <a href=\"javascript:alert(3)\">x</a>\n\n\
             <iframe src=\"https://example.org\"></iframe>\n\n\
             <style>body{}</style><link rel=stylesheet href=\"https://example.org/a.css\">\n\n\
             <input type=\"text\" value=\"x\"> <p style=\"color:red\">styled</p>\n",
        );
        for banned in [
            "script",
            "alert",
            "onerror",
            "javascript",
            "iframe",
            "<style",
            "<link",
            "text",
            "style=",
        ] {
            assert!(!html.contains(banned), "found {banned} in {html}");
        }
        assert!(html.contains(r#"<img src="x.png">"#), "{html}");
        assert!(html.contains("styled"), "{html}");
    }

    #[test]
    fn page_wraps_article_with_policy() {
        let page = preview_page("<p>x</p>", "body{}", ":root{--bg:#000}");
        assert!(page.contains(r#"<article id="ringmd-article"><p>x</p></article>"#));
        assert!(page.contains("default-src 'none'"));
        assert!(page.contains("<style>body{}</style>"));
        assert!(page.contains(r#"<style id="ringmd-theme">:root{--bg:#000}</style>"#));
        assert!(PREVIEW_CSS.contains("--bg: #282828"));
    }

    #[test]
    fn fixture_document_renders_safely() {
        let html = render(include_str!("../../tests/fixtures/preview.md"));
        assert!(html.contains(">Preview fixture</h1>"), "{html}");
        assert!(html.contains(r#"src="img/swatch.png""#), "{html}");
        assert!(html.contains("Raw HTML paragraph"), "{html}");
        for banned in ["<script", "onclick", "<link", "style=", "evil.css"] {
            assert!(!html.contains(banned), "found {banned}");
        }
        // Every section heading keeps its source line for scroll sync.
        assert_eq!(html.matches("<h2 data-sourcepos=").count(), 40);
    }

    #[test]
    fn task_checkboxes_stay_read_only() {
        let html = render("<input type=\"checkbox\" checked>\n");
        assert!(html.contains("disabled"), "{html}");
    }

    #[test]
    fn images_keep_relative_and_remote_sources() {
        let html =
            render("![a](img/a.png) ![b](https://example.org/b.png) ![c](file:///tmp/c.png)\n");
        assert!(html.contains(r#"src="img/a.png""#), "{html}");
        assert!(
            html.contains(r#"src="https://example.org/b.png""#),
            "{html}"
        );
        assert!(html.contains(r#"src="file:///tmp/c.png""#), "{html}");
    }

    /// F4-AC1 budget check: `cargo test --release -- --ignored render_time`.
    #[test]
    #[ignore = "timing; run in release mode"]
    fn render_time_for_a_thousand_lines() {
        let doc: String = (0..100)
            .map(|i| {
                format!(
                    "## Heading {i}\n\nSome *text* with `code` and a [link](https://example.org/{i}).\n\n\
                     - item a\n- item b\n\n```rust\nfn f{i}() -> u32 {{ {i} }}\n```\n\n"
                )
            })
            .collect();
        assert!(doc.lines().count() >= 1000);
        let renderer = MarkdownRenderer::new();
        renderer.render("warm up"); // loads the syntax definitions once
        let start = std::time::Instant::now();
        let runs = 20;
        for _ in 0..runs {
            renderer.render(&doc);
        }
        let each = start.elapsed() / runs;
        println!("render of {} lines: {each:?}", doc.lines().count());
        assert!(each < std::time::Duration::from_millis(100), "{each:?}");
    }
}
