//! Reads the Javadoc that ships beside the framework jar for the one thing the
//! bytecode does not carry: the name of each parameter. Method summaries are
//! Cisco's prose and are only copied in when asked for.

use std::collections::HashMap;
use std::io::Read;

use regex::Regex;

/// Parameter names and summary of a method, keyed by class, method and arity.
pub type Docs = HashMap<(String, String, usize), (Vec<String>, String)>;

pub fn read(path: Option<&str>) -> Result<Docs, Box<dyn std::error::Error>> {
    let mut docs = Docs::new();
    let Some(path) = path else {
        return Ok(docs);
    };
    let anchor = Regex::new(r#"<a name="(\w+)-([^"]*)">"#)?;
    let signature = Regex::new(r"(?s)<pre>(.*?)</pre>")?;
    let brief = Regex::new(r"(?s)\\brief(.*?)(?:\\param|\\return|</pre>|$)")?;
    let tag = Regex::new(r"<[^>]+>")?;

    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_owned();
        if !name.contains("/com/cisco/pt/ipc/")
            || !has_extension(&name, "html")
            || name.contains("/class-use/")
        {
            continue;
        }
        let mut page = String::new();
        if file.read_to_string(&mut page).is_err() {
            continue;
        }
        let owner = name.rsplit('/').next().unwrap_or(&name);
        let owner = owner[..owner.len() - ".html".len()].to_owned();
        let anchors: Vec<_> = anchor.captures_iter(&page).collect();
        for (position, found) in anchors.iter().enumerate() {
            let whole = found.get(0).unwrap();
            let end = anchors
                .get(position + 1)
                .map_or(page.len(), |next| next.get(0).unwrap().start());
            let section = &page[whole.start()..end];
            let Some(printed) = signature.captures(section) else {
                continue;
            };
            if !section.contains("<h4>") {
                continue;
            }
            let text = unescape(&tag.replace_all(&printed[1], ""));
            let (Some(open), Some(close)) = (text.find('('), text.rfind(')')) else {
                continue;
            };
            let names: Vec<String> = arguments(&text[open + 1..close])
                .iter()
                .filter_map(|part| part.split_whitespace().last().map(str::to_owned))
                .collect();
            let summary = brief.captures(section).map_or_else(String::new, |brief| {
                let text = unescape(&tag.replace_all(&brief[1], ""));
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            });
            let summary: String = summary.chars().take(400).collect();
            docs.entry((owner.clone(), found[1].to_owned(), names.len()))
                .or_insert((names, summary));
        }
    }
    Ok(docs)
}

/// Splits a parameter list on the commas that separate arguments, leaving alone
/// the ones inside type arguments: `Pair<IPAddress,IPAddress> ipAndMask` is one.
fn arguments(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for character in text.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    parts.push(current);
    parts.retain(|part| !part.trim().is_empty());
    parts
}

fn has_extension(name: &str, extension: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

fn unescape(text: &str) -> String {
    text.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::arguments;

    #[test]
    fn keeps_a_generic_argument_whole() {
        assert_eq!(
            arguments("Pair<IPAddress,IPAddress>\u{a0}ipAndMask"),
            ["Pair<IPAddress,IPAddress>\u{a0}ipAndMask"]
        );
        assert_eq!(arguments("int\u{a0}a,\u{a0}String\u{a0}b").len(), 2);
        assert!(arguments("").is_empty());
    }
}
