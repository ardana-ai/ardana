//! The model library's grammar: `<name>` and `<name>:<quant>` name library models case-insensitively and stand for
//! the `hf.co/` reference they pull; anything else is no library model.

use ardana_registry::library::library;
use ardana_registry::refs::{HfFile, Ref};

#[test]
fn names_and_quants() {
    let pick = |input: &str| {
        library()
            .find(input)
            .unwrap_or_else(|| panic!("{input} is a library model"))
    };

    let decider = pick("decider-2b");
    assert_eq!(decider.name(), "decider-2b");
    assert_eq!(decider.reference(), "hf.co/Mapika/decider-2b-GGUF:Q4_K_M");
    assert_eq!(decider.size(), Some(1_274_396_800));
    assert_eq!(decider.model.tokenizer, None);
    assert_eq!(decider.model.layout, None);
    for same in ["Decider-2B", "decider-2b:q4_k_m", "DECIDER-2B:Q4_K_M"] {
        assert_eq!(pick(same), decider, "{same}");
    }

    let q8 = pick("decider-2b:Q8_0");
    assert_eq!(q8.name(), "decider-2b:q8_0");
    assert_eq!(q8.reference(), "hf.co/Mapika/decider-2b-GGUF:q8_0");
    assert_eq!(q8.size(), None, "only the default quant's size is known");
    assert_eq!(
        Ref::parse(&q8.reference()).unwrap(),
        Ref::Hf {
            org: "Mapika".into(),
            repo: "decider-2b-GGUF".into(),
            file: Some(HfFile::Quant("q8_0".into())),
        }
    );

    let qwen = pick("qwen3.5-0.8b");
    assert_eq!(qwen.reference(), "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0");
    assert_eq!(
        qwen.model.tokenizer.as_deref(),
        Some("hf.co/Qwen/Qwen3.5-0.8B")
    );
    assert_eq!(qwen.model.layout, Some(ardana_registry::LayoutKind::Chat));
    let smollm = pick("smollm3-3b");
    assert_eq!(smollm.reference(), "hf.co/ggml-org/SmolLM3-3B-GGUF:Q4_K_M");
    assert_eq!(
        smollm.model.tokenizer.as_deref(),
        Some("hf.co/HuggingFaceTB/SmolLM3-3B")
    );
    assert_eq!(
        pick("decider-4b").reference(),
        "hf.co/Mapika/decider-4b-GGUF:Q4_K_M"
    );

    assert_eq!(library().default_pick(), decider);
}

#[test]
fn unknown_names() {
    for input in [
        "",
        "decider",
        "decider-2b-GGUF",
        "decider-2b:",
        "decider-2b:Q4 K",
        "decider-2b:a:b",
        "llama3.2",
        "hf.co/Mapika/decider-2b-GGUF",
        "ollama:decider-2b",
        "./decider-2b",
        " decider-2b",
    ] {
        assert_eq!(library().find(input), None, "{input:?}");
    }
    // A bare name outside the library is no reference either; the error lists the library.
    let err = Ref::parse("decider").unwrap_err().to_string();
    assert!(
        err.contains("neither a library model (decider-2b, decider-4b, qwen3.5-0.8b, smollm3-3b)"),
        "{err}"
    );
}
