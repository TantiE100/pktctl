use std::ops::Range;

use quick_xml::{Reader, events::Event};

use crate::PktError;

/// An element located by byte offsets: `outer` covers the tags, `inner` the content
/// (`None` for a self-closing element).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Element {
    pub path: Vec<String>,
    pub outer: Range<usize>,
    pub inner: Option<Range<usize>>,
}

impl Element {
    pub(crate) fn name(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }

    pub(crate) fn contains(&self, other: &Self) -> bool {
        self.outer.start <= other.outer.start && other.outer.end <= self.outer.end
    }

    pub(crate) fn is_child_of(&self, parent: &Self) -> bool {
        parent.contains(self) && self.path.len() == parent.path.len() + 1
    }

    pub(crate) fn text<'x>(&self, xml: &'x str) -> &'x str {
        self.inner.clone().map_or("", |inner| &xml[inner])
    }
}

pub(crate) fn elements(xml: &str) -> Result<Vec<Element>, PktError> {
    let mut reader = Reader::from_str(xml);
    let mut open: Vec<(String, usize, usize)> = Vec::new();
    let mut found = Vec::new();
    loop {
        let before = offset(&reader);
        let event = reader
            .read_event()
            .map_err(|error| PktError::Xml(error.to_string()))?;
        let after = offset(&reader);
        match event {
            Event::Start(tag) => {
                let name = String::from_utf8_lossy(tag.name().as_ref()).into_owned();
                open.push((name, before, after));
            }
            Event::Empty(tag) => {
                let mut path: Vec<String> = open.iter().map(|(name, ..)| name.clone()).collect();
                path.push(String::from_utf8_lossy(tag.name().as_ref()).into_owned());
                found.push(Element {
                    path,
                    outer: before..after,
                    inner: None,
                });
            }
            Event::End(_) => {
                let path: Vec<String> = open.iter().map(|(name, ..)| name.clone()).collect();
                if let Some((_, start, content)) = open.pop() {
                    found.push(Element {
                        path,
                        outer: start..after,
                        inner: Some(content..before),
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    found.sort_by_key(|element| element.outer.start);
    Ok(found)
}

fn offset(reader: &Reader<&[u8]>) -> usize {
    usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX)
}

/// Replaces byte ranges, applying the last one first so earlier offsets stay valid.
pub(crate) fn splice(xml: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = xml.to_owned();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    out
}
