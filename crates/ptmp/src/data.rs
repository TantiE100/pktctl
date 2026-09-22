//! Field counts of IPC value objects (type 16), keyed by the class name on the wire.
//!
//! A value object is sent as its class name followed by its fields, with no count.
//! Registered fixed layouts are read exactly and variable ones while the next token is
//! a type code. Once layouts are registered, a type-16 token that names no class is a
//! plain string: Packet Tracer sends some string fields of value objects that way.
//! Without any registration every class is read field by field.

use std::{
    collections::HashMap,
    sync::{PoisonError, RwLock},
};

static LAYOUTS: RwLock<Option<HashMap<String, Option<usize>>>> = RwLock::new(None);

/// Registers value-object classes: `Some(count)` for a fixed number of fields, `None` for
/// classes whose field count varies and are read while the next token is a type code.
pub fn register(layouts: impl IntoIterator<Item = (String, Option<usize>)>) {
    let mut guard = LAYOUTS.write().unwrap_or_else(PoisonError::into_inner);
    guard.get_or_insert_with(HashMap::new).extend(layouts);
}

pub(crate) enum Layout {
    Fixed(usize),
    Variable,
    NotAClass,
}

pub(crate) fn layout(class: &str) -> Layout {
    let guard = LAYOUTS.read().unwrap_or_else(PoisonError::into_inner);
    match guard.as_ref() {
        None => Layout::Variable,
        Some(layouts) => match layouts.get(class) {
            Some(Some(count)) => Layout::Fixed(*count),
            Some(None) => Layout::Variable,
            None => Layout::NotAClass,
        },
    }
}
