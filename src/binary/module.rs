use nom::IResult;
use nom::bytes::complete::tag;
use nom::number::complete::le_u32;

const WASM_BINARY_MAGIC: &str = "\0asm";
const WASM_BINARY_VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub struct Module {
    pub magic: String,
    pub version: u32,
}

impl Default for Module {
    fn default() -> Self {
        Self {
            magic: WASM_BINARY_MAGIC.to_string(),
            version: WASM_BINARY_VERSION,
        }
    }
}

impl Module {
    pub fn new(input: &[u8]) -> anyhow::Result<Self> {
        let (_, module) =
            Self::decode(input).map_err(|e| anyhow::anyhow!("failed to parse wasm: {e}"))?;
        Ok(module)
    }

    fn decode(input: &[u8]) -> IResult<&[u8], Module> {
        let (input, _) = tag(WASM_BINARY_MAGIC.as_bytes())(input)?;
        let (input, version) = le_u32(input)?;

        let module = Self {
            magic: WASM_BINARY_MAGIC.into(),
            version,
        };
        Ok((input, module))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_simplest_module() -> anyhow::Result<()> {
        // Generate wasm binary with only preamble present
        let wasm = wat::parse_str("(module)")?;
        // Decode binary and generate `Module` structure
        let module = Module::new(&wasm)?;
        // Compare whether the generated `Module` structure is as expected
        assert_eq!(module, Module::default());
        Ok(())
    }
}
