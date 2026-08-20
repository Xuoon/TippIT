//! Sanitisierung von Clipboard-HTML.
//!
//! Formatierter Inhalt stammt aus beliebigen fremden Programmen und wird später
//! in einer WebView gerendert, die Tauri-Commands aufrufen darf. Deshalb wird
//! bereits beim ERFASSEN sanitisiert und nur das Ergebnis gespeichert — in der
//! Datenbank liegt nie das Original. Damit kann auch ein späterer Renderpfad
//! nichts Gefährliches mehr aus der Historie ziehen.

use std::sync::LazyLock;

/// Größere Fragmente sind fast immer ganze Webseiten und im Vorschaufenster
/// ohnehin nicht sinnvoll darstellbar — dann bleibt es beim Klartext.
const MAX_HTML_BYTES: usize = 512 * 1024;

static CLEANER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut builder = ammonia::Builder::default();
    builder
        .tags(
            [
                "a",
                "b",
                "blockquote",
                "br",
                "code",
                "div",
                "em",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "hr",
                "i",
                "li",
                "ol",
                "p",
                "pre",
                "s",
                "span",
                "strong",
                "sub",
                "sup",
                "table",
                "tbody",
                "td",
                "th",
                "thead",
                "tr",
                "u",
                "ul",
            ]
            .into_iter()
            .collect(),
        )
        .generic_attributes(["style"].into_iter().collect())
        // Nur reine Darstellung: alles, was Inhalte nachladen könnte (background,
        // list-style-image, …), bleibt draußen.
        .filter_style_properties(
            [
                "background-color",
                "color",
                "font-family",
                "font-size",
                "font-style",
                "font-weight",
                "text-align",
                "text-decoration",
            ]
            .into_iter()
            .collect(),
        )
        // Relative Links haben ohne Ursprungsseite keine Bedeutung mehr.
        .url_relative(ammonia::UrlRelative::Deny);
    builder
});

/// Fremd-HTML in eine gefahrlose Darstellungsform bringen. `None`, wenn nach der
/// Reinigung nichts Formatiertes übrig bleibt — dann lohnt das Speichern nicht.
pub fn sanitize(raw: &str) -> Option<String> {
    if raw.len() > MAX_HTML_BYTES {
        return None;
    }
    let clean = CLEANER.clean(raw).to_string();
    let trimmed = clean.trim();
    if trimmed.is_empty() || !trimmed.contains('<') {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_handlers() {
        let dirty = r#"<p onclick="alert(1)">Hallo <script>alert(2)</script><b>Welt</b></p>"#;
        let clean = sanitize(dirty).unwrap();
        assert!(!clean.contains("script"));
        assert!(!clean.contains("onclick"));
        assert!(clean.contains("<b>Welt</b>"));
    }

    #[test]
    fn keeps_colors_but_drops_loading_properties() {
        let clean = sanitize(
            r#"<span style="color: #ff0000; background-image: url(http://x/y.png)">rot</span>"#,
        )
        .unwrap();
        assert!(clean.contains("color"));
        assert!(!clean.contains("background-image"));
    }

    #[test]
    fn plain_text_and_oversized_fragments_are_skipped() {
        assert!(sanitize("nur Text").is_none());
        assert!(sanitize(&format!("<p>{}</p>", "x".repeat(MAX_HTML_BYTES))).is_none());
    }
}
