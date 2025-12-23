use std::collections::HashMap;

use super::{Store, Value};

pub type ImportFunc = Box<dyn FnMut(&mut Store, Vec<Value>) -> anyhow::Result<Option<Value>>>;
pub type Import = HashMap<String, HashMap<String, ImportFunc>>;
