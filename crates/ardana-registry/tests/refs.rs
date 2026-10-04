//! R4.5: model references parse as `hf.co/org/repo[:quant]`, `hf.co/org/repo:file.gguf`, `ollama:[ns/]name[:tag]`
//! and local paths, with a clear error for anything else.

use std::path::PathBuf;

use ardana_registry::refs::{HfFile, Ref, companion_of, matches_quant};

fn hf(org: &str, repo: &str, file: Option<HfFile>) -> Ref {
    Ref::Hf {
        org: org.into(),
        repo: repo.into(),
        file,
    }
}

fn ollama(namespace: &str, name: &str, tag: &str) -> Ref {
    Ref::Ollama {
        namespace: namespace.into(),
        name: name.into(),
        tag: tag.into(),
    }
}

#[test]
fn grammar() {
    let ok = |s: &str| Ref::parse(s).unwrap_or_else(|err| panic!("{s}: {err}"));
    assert_eq!(
        ok("hf.co/Mapika/decider-2b-GGUF"),
        hf("Mapika", "decider-2b-GGUF", None)
    );
    assert_eq!(
        ok("hf.co/Mapika/decider-2b-GGUF:q4_k_m"),
        hf(
            "Mapika",
            "decider-2b-GGUF",
            Some(HfFile::Quant("q4_k_m".into()))
        )
    );
    assert_eq!(
        ok("hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Qwen3.5-0.8B-Q4_0.gguf"),
        hf(
            "ggml-org",
            "Qwen3.5-0.8B-GGUF",
            Some(HfFile::Name("Qwen3.5-0.8B-Q4_0.gguf".into()))
        )
    );
    assert_eq!(
        ok("hf.co/org/repo:sub/dir/model-IQ4_XS.GGUF"),
        hf(
            "org",
            "repo",
            Some(HfFile::Name("sub/dir/model-IQ4_XS.GGUF".into()))
        )
    );
    assert_eq!(
        ok("ollama:llama3.2"),
        ollama("library", "llama3.2", "latest")
    );
    assert_eq!(
        ok("ollama:llama3.2:1b"),
        ollama("library", "llama3.2", "1b")
    );
    assert_eq!(
        ok("ollama:someone/model:q8_0"),
        ollama("someone", "model", "q8_0")
    );
    assert_eq!(
        ok("/models/decider.gguf"),
        Ref::Local(PathBuf::from("/models/decider.gguf"))
    );
    assert_eq!(
        ok("./decider-2b-v11-Q4_K_M.gguf"),
        Ref::Local(PathBuf::from("./decider-2b-v11-Q4_K_M.gguf"))
    );
    assert_eq!(ok("model.gguf"), Ref::Local(PathBuf::from("model.gguf")));

    let err = |s: &str| match Ref::parse(s) {
        Ok(parsed) => panic!("{s} parsed as {parsed:?}"),
        Err(err) => err.to_string(),
    };
    for bad in [
        "",
        " hf.co/a/b",
        "hf.co/",
        "hf.co/Mapika",
        "hf.co/a/b/c",
        "hf.co/a/b:",
        "hf.co/a/b:Q4 K",
        "hf.co/a/b:../x.gguf",
        "hf.co/a b/c",
        "ollama:",
        "ollama:a/b/c",
        "ollama:llama3.2:",
        "ollama:llama 3",
        "https://huggingface.co/Mapika/decider-2b-GGUF",
        "huggingface.co/Mapika/decider-2b-GGUF",
        "llama3.2",
        "what?",
    ] {
        let msg = err(bad);
        assert!(
            msg.contains("is not a model reference")
                && msg.contains("hf.co/<org>/<repo>[:<quant>]")
                && msg.contains("ollama:[<namespace>/]<name>[:<tag>]"),
            "{bad:?}: {msg}"
        );
    }
    assert!(err("llama3.2").contains("write ollama:llama3.2"));
    assert!(err("huggingface.co/a/b").contains("write Hugging Face repositories as hf.co/"));
}

#[test]
fn default_names() {
    let name = |s: &str| Ref::parse(s).unwrap().default_name();
    assert_eq!(name("hf.co/Mapika/decider-2b-GGUF:Q4_K_M"), "decider-2b");
    assert_eq!(name("hf.co/ggml-org/Qwen3.5-0.8B-GGUF"), "qwen3.5-0.8b");
    assert_eq!(name("hf.co/Qwen/Qwen3.5-0.8B"), "qwen3.5-0.8b");
    assert_eq!(name("ollama:llama3.2"), "llama3.2");
    assert_eq!(name("ollama:llama3.2:1b"), "llama3.2");
    assert_eq!(name("/m/Decider-2B.gguf"), "decider-2b");
}

#[test]
fn quants_match_case_insensitively() {
    assert!(matches_quant("decider-2b-v11-Q4_K_M.gguf", "q4_k_m"));
    assert!(matches_quant("decider-2b-v11-Q4_K_M.gguf", "Q4_K_M"));
    assert!(matches_quant("model.Q8_0.gguf", "q8_0"));
    assert!(matches_quant("dir/SmolLM3-Q4_K_M.gguf", "Q4_K_M"));
    assert!(!matches_quant("decider-2b-v11-Q4_K_M.gguf", "K_M"));
    assert!(!matches_quant("decider-2b-v11-Q4_K_M.gguf", "Q4_K"));
    assert!(!matches_quant("decider-2b-v11-Q4_K_M.bin", "Q4_K_M"));
}

#[test]
fn companions_carry_the_model_file_name() {
    let model = "gemma-4-E4B-it-Q4_0.gguf";
    assert!(companion_of("mtp-gemma-4-E4B-it-Q4_0.gguf", model));
    assert!(companion_of("dir/MMPROJ-gemma-4-E4B-it-Q4_0.gguf", model));
    assert!(!companion_of(model, model));
    assert!(!companion_of(model, "mtp-gemma-4-E4B-it-Q4_0.gguf"));
    assert!(!companion_of("xgemma-4-E4B-it-Q4_0.gguf", model));
    assert!(!companion_of("mtp-gemma-4-E4B-it-Q8_0.gguf", model));
}
