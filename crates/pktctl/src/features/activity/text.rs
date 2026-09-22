const HIDDEN: &[&str] = &["style", "script", "head"];
const BREAKS: &[&str] = &[
    "br", "p", "div", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6",
];
const ENTITIES: &[(&str, &str)] = &[
    ("&nbsp;", " "),
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&apos;", "'"),
    ("&amp;", "&"),
];

const INLINE_DATA: &str = "data:";
const REMOVED_IMAGE: &str = "data:,image-removed";

/// Replaces every `data:` URI in the HTML, the embedded images of activity
/// instructions, with a short placeholder.
pub fn without_inline_images(html: &str) -> String {
    let mut kept = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(INLINE_DATA) {
        let quote = rest[..start].chars().next_back();
        let Some(quote @ ('"' | '\'')) = quote else {
            kept.push_str(&rest[..start + INLINE_DATA.len()]);
            rest = &rest[start + INLINE_DATA.len()..];
            continue;
        };
        kept.push_str(&rest[..start]);
        kept.push_str(REMOVED_IMAGE);
        let tail = &rest[start..];
        rest = tail.find(quote).map_or("", |end| &tail[end..]);
    }
    kept.push_str(rest);
    kept
}

/// Reduces Packet Tracer's rich-text HTML to readable lines.
pub fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut rest = html;
    let mut hidden: Option<String> = None;
    while let Some(open) = rest.find('<') {
        if hidden.is_none() {
            text.push_str(&rest[..open]);
        }
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim();
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        match &hidden {
            Some(inside) if closing && *inside == name => hidden = None,
            None if !closing && HIDDEN.contains(&name.as_str()) && !tag.ends_with('/') => {
                hidden = Some(name);
            }
            None if name == "img" => text.push_str("[image]"),
            None if BREAKS.contains(&name.as_str()) => text.push('\n'),
            Some(_) | None => {}
        }
        rest = &rest[open + close + 1..];
    }
    if hidden.is_none() {
        text.push_str(rest);
    }
    let decoded = ENTITIES
        .iter()
        .fold(text, |text, (entity, plain)| text.replace(entity, plain));
    decoded
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_embedded_images_but_keeps_the_tags() {
        let html = r#"<p>Tags:<br><img width="800px" src="data:image/png;base64,iVBORw0KGgo=" alt=""></p><img src='data:image/gif;base64,R0lG'><a href="https://x/data:y">link</a>"#;
        let clean = without_inline_images(html);
        assert_eq!(
            clean,
            r#"<p>Tags:<br><img width="800px" src="data:,image-removed" alt=""></p><img src='data:,image-removed'><a href="https://x/data:y">link</a>"#
        );
        assert_eq!(html_to_text(html), "Tags:\n[image]\n[image]link");
    }

    #[test]
    fn keeps_the_words_and_drops_markup() {
        let html = "<html><head><title>Lab</title><style>body{color:red}</style></head>\
                    <body><h1>Latching &amp; PLC</h1><p>Step 1:&nbsp;cable <b>R1</b></p>\
                    <ul><li>ping PC2</li></ul></body></html>";
        assert_eq!(
            html_to_text(html),
            "Latching & PLC\nStep 1: cable R1\nping PC2"
        );
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(html_to_text("just text"), "just text");
    }
}
