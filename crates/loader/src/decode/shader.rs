//! Shader source decoder.

use crate::{DecodedShader, LoadError};

/// Decode WGSL shader bytes as UTF-8 text.
pub fn decode_shader(bytes: &[u8]) -> Result<DecodedShader, LoadError> {
    let source = std::str::from_utf8(bytes).map_err(|err| LoadError::Decode(err.to_string()))?;
    Ok(DecodedShader {
        source: source.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_shader_source() {
        let shader = decode_shader(b"fn vs_main() {}").unwrap();

        assert!(shader.source.contains("vs_main"));
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(matches!(decode_shader(&[0xff]), Err(LoadError::Decode(_))));
    }
}
