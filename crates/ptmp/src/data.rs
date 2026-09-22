//! Field counts of IPC value objects (type 16), keyed by the class name on the wire.
//!
//! A value object is sent as its class name followed by its fields, with no count.
//! When the layout of a class is registered its fields are read exactly; otherwise
//! fields are read while the next token is a type code, which is unambiguous unless
//! the object is followed by more values in the same pair or event.

use std::{
    collections::HashMap,
    sync::{PoisonError, RwLock},
};

static LAYOUTS: RwLock<Option<HashMap<String, usize>>> = RwLock::new(None);

pub fn register(layouts: impl IntoIterator<Item = (String, usize)>) {
    let mut guard = LAYOUTS.write().unwrap_or_else(PoisonError::into_inner);
    guard.get_or_insert_with(HashMap::new).extend(layouts);
}

pub(crate) fn field_count(class: &str) -> Option<usize> {
    LAYOUTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|layouts| layouts.get(class).copied())
}
