use xtask::fetch::Hf;

/// R1.5: `cargo xtask fetch --tests` (CI) leaves out every weight file, GGUF and ONNX, and keeps what plain tests read.
#[test]
fn without_weights() {
    let hf = Hf {
        repo: "ardana-ai/decider-0.8b-ONNX".into(),
        revision: "0".repeat(40),
        files: [
            "decider-0.8b-Q8_0.gguf",
            "model.onnx",
            "model.onnx.data",
            "onnx/model_q4.onnx",
            "tokenizer.json",
            "tokenizer_config.json",
            "chat_template.jinja",
            "decider_config.json",
        ]
        .map(String::from)
        .to_vec(),
    };
    let tests = hf.without_weights();
    assert_eq!((tests.repo, tests.revision), (hf.repo, hf.revision));
    assert_eq!(
        tests.files,
        [
            "tokenizer.json",
            "tokenizer_config.json",
            "chat_template.jinja",
            "decider_config.json"
        ]
    );
}
