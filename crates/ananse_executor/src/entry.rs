use ananse_decoder::{Image, Module, Word, WordType};

use crate::{ExecuteError, Result};

pub fn validate_args(
    image: &Image,
    func_index: u32,
    params: u32,
    args: &[Word],
) -> Result<Vec<Word>> {
    if args.is_empty() && params > 0 {
        let ty = &image.types[image.func_types[func_index as usize] as usize];
        return Ok(ty.params.iter().map(|t| t.zero()).collect());
    }
    if args.len() != params as usize {
        return Err(ExecuteError::ArgumentCountMismatch {
            expected: params as usize,
            actual: args.len(),
        });
    }
    Ok(args.to_vec())
}

/// Where execution begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Run the exported `_start`, else the first exported function, else the
    /// first defined function; a module with no defined function runs nothing.
    Auto,
    /// Run the function at this index in the module function index space.
    Function(u32),
    /// Run the function exported under this name.
    Export(String),
}

impl Entry {
    pub fn params(&self, module: &Module) -> Result<Vec<WordType>> {
        let image = Image::parse(module.bytes())?;
        let Some(func_index) = self.resolve(&image)? else {
            return Ok(Vec::new());
        };
        let type_idx = *image
            .func_types
            .get(func_index as usize)
            .ok_or(ExecuteError::UndefinedEntry)?;
        let ty = image
            .types
            .get(type_idx as usize)
            .ok_or(ExecuteError::UndefinedEntry)?;
        Ok(ty.params.clone())
    }

    pub fn resolve(&self, image: &Image) -> Result<Option<u32>> {
        match self {
            Self::Function(idx) => Ok(Some(*idx)),
            Self::Export(name) => image
                .func_exports
                .iter()
                .find(|(export, _)| export == name)
                .map(|(_, idx)| Some(*idx))
                .ok_or(ExecuteError::UndefinedEntry),
            Self::Auto => Ok(image
                .func_exports
                .iter()
                .find(|(name, _)| name == "_start")
                .or_else(|| image.func_exports.first())
                .map(|(_, idx)| *idx)
                .or_else(|| (!image.funcs.is_empty()).then_some(image.num_imported))),
        }
    }
}
