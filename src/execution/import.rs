use std::collections::HashMap;

use super::{Store, Value};

/// A host function that can be called from WebAssembly.
pub type ImportFunc = Box<dyn FnMut(&mut Store, Vec<Value>) -> anyhow::Result<Option<Value>>>;

/// Registry of imported functions, organized by module name then function name.
pub type Import = HashMap<String, HashMap<String, ImportFunc>>;
