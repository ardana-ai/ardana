//! GGUF quantization with the linked llama.cpp (`llama_model_quantize`), as `llama-quantize <in> <out> <type>` does it
//! without an importance matrix: the types that need none, by the names `llama-quantize` gives them.

use std::ffi::CString;
use std::path::Path;

use anyhow::{Context, Result, bail};
use llama_cpp_sys_2 as sys;

/// The llama.cpp file types [`quantize`] writes, lowercase as the library names its quants: every type
/// `llama-quantize` lists that needs no importance matrix, its aliases in [`ALIASES`].
pub const QUANTS: &[(&str, sys::llama_ftype)] = &[
    ("q4_0", sys::LLAMA_FTYPE_MOSTLY_Q4_0),
    ("q4_1", sys::LLAMA_FTYPE_MOSTLY_Q4_1),
    ("q5_0", sys::LLAMA_FTYPE_MOSTLY_Q5_0),
    ("q5_1", sys::LLAMA_FTYPE_MOSTLY_Q5_1),
    ("q2_k", sys::LLAMA_FTYPE_MOSTLY_Q2_K),
    ("q3_k_s", sys::LLAMA_FTYPE_MOSTLY_Q3_K_S),
    ("q3_k_m", sys::LLAMA_FTYPE_MOSTLY_Q3_K_M),
    ("q3_k_l", sys::LLAMA_FTYPE_MOSTLY_Q3_K_L),
    ("iq4_nl", sys::LLAMA_FTYPE_MOSTLY_IQ4_NL),
    ("iq4_xs", sys::LLAMA_FTYPE_MOSTLY_IQ4_XS),
    ("q4_k_s", sys::LLAMA_FTYPE_MOSTLY_Q4_K_S),
    ("q4_k_m", sys::LLAMA_FTYPE_MOSTLY_Q4_K_M),
    ("q5_k_s", sys::LLAMA_FTYPE_MOSTLY_Q5_K_S),
    ("q5_k_m", sys::LLAMA_FTYPE_MOSTLY_Q5_K_M),
    ("q6_k", sys::LLAMA_FTYPE_MOSTLY_Q6_K),
    ("q8_0", sys::LLAMA_FTYPE_MOSTLY_Q8_0),
    ("f16", sys::LLAMA_FTYPE_MOSTLY_F16),
    ("bf16", sys::LLAMA_FTYPE_MOSTLY_BF16),
    ("f32", sys::LLAMA_FTYPE_ALL_F32),
];

/// The aliases `llama-quantize` takes for a [`QUANTS`] name, lowercase: the library names phi-4:14b's default quant
/// `q4_k`, as its GGUF's file name does.
pub const ALIASES: &[(&str, &str)] = &[("q3_k", "q3_k_m"), ("q4_k", "q4_k_m"), ("q5_k", "q5_k_m")];

/// The file type `quant` names, or the quant it is an alias of, ASCII case ignored; `None` for a name [`QUANTS`] and
/// [`ALIASES`] lack.
fn file_type(quant: &str) -> Option<sys::llama_ftype> {
    let quant = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(quant))
        .map_or(quant, |&(_, name)| name);
    QUANTS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(quant))
        .map(|&(_, ftype)| ftype)
}

/// Whether [`quantize`] writes `quant`, a [`QUANTS`] name or an alias of one.
pub fn is_quant(quant: &str) -> bool {
    file_type(quant).is_some()
}

/// Writes the GGUF `input` quantized to `quant` (a [`QUANTS`] name or an alias) into `output`, with llama.cpp's
/// defaults for that type (its output and embedding tensors as the type's mix picks them).
pub fn quantize(input: &Path, output: &Path, quant: &str) -> Result<()> {
    let Some(ftype) = file_type(quant) else {
        bail!(
            "llama.cpp writes no quant {quant:?} without an importance matrix; the quants are {}",
            QUANTS
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    crate::backend()?;
    let path = |path: &Path| {
        CString::new(path.as_os_str().as_encoded_bytes())
            .with_context(|| format!("{} holds a NUL byte", path.display()))
    };
    let (input_c, output_c) = (path(input)?, path(output)?);
    // SAFETY: `llama_model_quantize_default_params` takes nothing and returns a plain struct by value.
    let mut params = unsafe { sys::llama_model_quantize_default_params() };
    params.ftype = ftype;
    // SAFETY: both paths are NUL-terminated strings that outlive the call, and `params` is the default parameters
    // with its file type set, its pointers left null as llama.cpp's defaults have them.
    let status = unsafe { sys::llama_model_quantize(input_c.as_ptr(), output_c.as_ptr(), &params) };
    if status != 0 {
        bail!(
            "llama.cpp could not quantize {} to {quant} (status {status})",
            input.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names are the library's lowercase quants, each once, read whatever their case, and `llama-quantize`'s
    /// aliases the type of the quant they stand for (`q4_k`, phi-4:14b's library quant, is `q4_k_m`); one that needs
    /// an importance matrix is refused, naming the others.
    #[test]
    fn quants_are_named_like_the_library() {
        assert!(is_quant("q8_0") && is_quant("Q4_K_M") && is_quant("bf16"));
        assert!(!is_quant("iq2_xxs") && !is_quant("q4_k_l"));
        for (i, (name, _)) in QUANTS.iter().enumerate() {
            assert_eq!(*name, name.to_ascii_lowercase());
            assert!(QUANTS[..i].iter().all(|(other, _)| other != name));
        }
        assert_eq!(file_type("q4_k"), Some(sys::LLAMA_FTYPE_MOSTLY_Q4_K_M));
        assert_eq!(file_type("Q4_K"), Some(sys::LLAMA_FTYPE_MOSTLY_Q4_K_M));
        assert_eq!(file_type("q3_k"), Some(sys::LLAMA_FTYPE_MOSTLY_Q3_K_M));
        assert_eq!(file_type("q5_k"), Some(sys::LLAMA_FTYPE_MOSTLY_Q5_K_M));
        for (alias, name) in ALIASES {
            assert!(QUANTS.iter().all(|(quant, _)| quant != alias), "{alias}");
            assert_eq!(file_type(alias), file_type(name), "{alias}");
        }
        let err = quantize(Path::new("in.gguf"), Path::new("out.gguf"), "iq2_xxs").unwrap_err();
        assert!(err.to_string().contains("q8_0"), "{err}");
    }
}
