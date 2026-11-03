//! The preview pane: a WebKitGTK view showing the rendered document (T3.3).
//!
//! The page is loaded once per base directory; after that each update only
//! replaces the `<article>` contents through the WebView API, so the page
//! never reloads and its scroll position is kept (F4.R2, F4.R3). Scripts in
//! the document never run: JavaScript markup is disabled, the HTML is
//! sanitized, and the page's CSP forbids scripts (F4.R8).

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use webkit6::prelude::*;

use crate::model::render::{self, ARTICLE_ID};
use crate::viewmodel::PreviewViewModel;

/// The preview widget. Cheap to clone.
#[derive(Debug, Clone)]
pub struct PreviewView {
    web_view: webkit6::WebView,
}

impl PreviewView {
    pub fn new(vm: &PreviewViewModel) -> Self {
        let web_view = webkit6::WebView::new();
        web_view.set_hexpand(true);
        web_view.set_vexpand(true);
        if let Some(settings) = WebViewExt::settings(&web_view) {
            settings.set_enable_javascript_markup(false);
            settings.set_enable_developer_extras(false);
            settings.set_enable_page_cache(false);
            settings.set_enable_back_forward_navigation_gestures(false);
        }
        // Avoid a white flash before the stylesheet applies.
        web_view.set_background_color(&gtk::gdk::RGBA::new(0.157, 0.157, 0.157, 1.0));

        let loaded = Rc::new(Cell::new(false));
        let view = Self { web_view };
        view.load_page(vm, &loaded);

        view.web_view.connect_load_changed(glib::clone!(
            #[weak]
            vm,
            #[strong]
            loaded,
            move |web_view, event| {
                tracing::debug!(?event, uri = ?web_view.uri(), "preview load");
                if event == webkit6::LoadEvent::Finished {
                    loaded.set(true);
                    // Content may have changed while the page loaded.
                    run(web_view, &patch_script(&vm.html()));
                    run(web_view, &scroll_script(vm.scroll_target_line()));
                }
            }
        ));
        view.web_view.connect_decide_policy(glib::clone!(
            #[weak]
            vm,
            #[upgrade_or]
            false,
            move |web_view, decision, kind| decide_policy(web_view, &vm, decision, kind)
        ));
        // The context menu only offers navigation (back, reload) that
        // would break the preview; copying works without it.
        view.web_view.connect_context_menu(|_, _, _| true);

        vm.connect_html_notify(glib::clone!(
            #[weak(rename_to = web_view)]
            view.web_view,
            #[strong]
            loaded,
            move |vm| {
                tracing::debug!(
                    loaded = loaded.get(),
                    len = vm.html().len(),
                    "preview html changed"
                );
                if loaded.get() {
                    run(&web_view, &patch_script(&vm.html()));
                }
            }
        ));
        vm.connect_scroll_target_line_notify(glib::clone!(
            #[weak(rename_to = web_view)]
            view.web_view,
            #[strong]
            loaded,
            move |vm| {
                if loaded.get() {
                    run(&web_view, &scroll_script(vm.scroll_target_line()));
                }
            }
        ));
        vm.connect_base_uri_notify(glib::clone!(
            #[strong]
            view,
            #[strong]
            loaded,
            move |vm| view.load_page(vm, &loaded)
        ));
        view
    }

    pub fn widget(&self) -> &webkit6::WebView {
        &self.web_view
    }

    /// (Re)load the page with the current content and base directory.
    fn load_page(&self, vm: &PreviewViewModel, loaded: &Cell<bool>) {
        loaded.set(false);
        tracing::debug!(base = %vm.base_uri(), "loading preview page");
        let page = render::preview_page(&vm.html(), render::PREVIEW_CSS);
        let base = vm.base_uri();
        self.web_view
            .load_html(&page, (!base.is_empty()).then_some(base.as_str()));
    }
}

/// Keep navigation inside the preview to same-page anchors (footnotes);
/// every other link goes to the ViewModel, which opens it externally.
fn decide_policy(
    web_view: &webkit6::WebView,
    vm: &PreviewViewModel,
    decision: &webkit6::PolicyDecision,
    kind: webkit6::PolicyDecisionType,
) -> bool {
    use webkit6::PolicyDecisionType as Kind;
    let Some(navigation) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
        // Responses (resource loads) keep WebKit's default handling.
        return false;
    };
    let Some(action) = navigation.navigation_action() else {
        return false;
    };
    let uri = action
        .request()
        .and_then(|request| request.uri())
        .map(|uri| uri.to_string())
        .unwrap_or_default();
    tracing::debug!(%uri, ?kind, navigation = ?action.navigation_type(), "preview navigation");
    let user_initiated = action.navigation_type() == webkit6::NavigationType::LinkClicked
        || kind == Kind::NewWindowAction;
    if !user_initiated {
        // Our own load_html.
        return false;
    }
    let current = web_view.uri().map(|u| u.to_string()).unwrap_or_default();
    if kind == Kind::NavigationAction && is_same_page_anchor(&uri, &current) {
        return false;
    }
    decision.ignore();
    vm.link_activated(&uri);
    true
}

/// Whether `target` only jumps to an anchor on the page at `current`.
fn is_same_page_anchor(target: &str, current: &str) -> bool {
    let strip = |uri: &str| uri.split('#').next().unwrap_or_default().to_owned();
    target.contains('#') && strip(target) == strip(current)
}

fn run(web_view: &webkit6::WebView, script: &str) {
    web_view.evaluate_javascript(script, None, None, gtk::gio::Cancellable::NONE, |result| {
        if let Err(e) = result {
            tracing::warn!(error = %e, "preview script failed");
        }
    });
}

/// Replace the article contents; the page keeps its scroll position.
fn patch_script(html: &str) -> String {
    format!(
        "document.getElementById({}).innerHTML = {}; void 0;",
        js_string(ARTICLE_ID),
        js_string(html)
    )
}

/// Bring the block for source `line` into view, unless it already is
/// (F4.R4). The deepest element starting at or before `line` wins.
fn scroll_script(line: u32) -> String {
    format!(
        "(function (line) {{\
           var best = null, bestLine = -1;\
           var all = document.querySelectorAll('#{ARTICLE_ID} [data-sourcepos]');\
           for (var i = 0; i < all.length; i++) {{\
             var start = parseInt(all[i].getAttribute('data-sourcepos'), 10);\
             if (start <= line && start >= bestLine) {{ best = all[i]; bestLine = start; }}\
           }}\
           if (!best) {{ window.scrollTo(0, 0); return; }}\
           var r = best.getBoundingClientRect();\
           if (r.top < 0 || r.top > window.innerHeight - 24) {{\
             best.scrollIntoView({{ block: 'center' }});\
           }}\
         }})({line}); void 0;"
    )
}

/// A JavaScript string literal for `text`.
fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Line separators end statements in older engines; `<` keeps
            // `</script>` from ever appearing literally.
            '\u{2028}' | '\u{2029}' | '<' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_strings_are_escaped() {
        assert_eq!(js_string("plain"), r#""plain""#);
        assert_eq!(
            js_string("a\"b\\c\nd\u{2028}</script>\u{1}"),
            r#""a\"b\\c\nd\u2028\u003c/script>\u0001""#
        );
    }

    #[test]
    fn patch_targets_the_article() {
        assert_eq!(
            patch_script("<p>x</p>"),
            r#"document.getElementById("ringmd-article").innerHTML = "\u003cp>x\u003c/p>"; void 0;"#
        );
    }

    #[test]
    fn scroll_script_embeds_the_line() {
        let script = scroll_script(42);
        assert!(script.ends_with("})(42); void 0;"), "{script}");
        assert!(
            script.contains("#ringmd-article [data-sourcepos]"),
            "{script}"
        );
    }

    #[test]
    fn same_page_anchors() {
        assert!(is_same_page_anchor("file:///notes/#fn-1", "file:///notes/"));
        assert!(is_same_page_anchor("about:blank#fn-1", "about:blank"));
        assert!(!is_same_page_anchor(
            "file:///notes/other.md",
            "file:///notes/"
        ));
        assert!(!is_same_page_anchor(
            "https://example.org/#x",
            "file:///notes/"
        ));
    }
}
