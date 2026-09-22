//! Field counts of IPC value objects (type 16), keyed by the class name on the wire.
//!
//! A value object is sent as its class name followed by its fields, with no count, so a
//! decoder needs the layout of each class. With no layouts every class is read field by
//! field while the next token is a type code. Once layouts are known, fixed classes are
//! read exactly, variable ones field by field, and a type-16 token that names no class
//! is a plain string: Packet Tracer sends some string fields of value objects that way.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataLayouts {
    classes: HashMap<String, Option<usize>>,
}

impl DataLayouts {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Some(count)` for a fixed number of fields, `None` for a class whose count varies.
    pub fn insert(&mut self, class: impl Into<String>, fields: Option<usize>) {
        self.classes.insert(class.into(), fields);
    }

    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    pub(crate) fn layout(&self, class: &str) -> Layout {
        if self.classes.is_empty() {
            return Layout::Variable;
        }
        match self.classes.get(class) {
            Some(Some(count)) => Layout::Fixed(*count),
            Some(None) => Layout::Variable,
            None => Layout::NotAClass,
        }
    }
}

impl<S: Into<String>> FromIterator<(S, Option<usize>)> for DataLayouts {
    fn from_iter<I: IntoIterator<Item = (S, Option<usize>)>>(iter: I) -> Self {
        let mut layouts = Self::new();
        for (class, fields) in iter {
            layouts.insert(class, fields);
        }
        layouts
    }
}

pub(crate) enum Layout {
    Fixed(usize),
    Variable,
    NotAClass,
}
