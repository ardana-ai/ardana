"""Exports a Qwen3.5 checkpoint to the ONNX graph Ardana's browser models run on onnxruntime-web (docs/guidelines/onnx.md).

Run by `cargo xtask onnx convert <name>`, never by hand: it needs the tmp/py/onnx venv the step creates and the
checkpoint snapshot the step downloads into tmp/hf.

    python onnx_export.py <checkpoint dir> <int4|int8> <output dir> <builder cache dir>

The onnxruntime-genai 0.17.1 model builder exports the graph for the WebGPU provider (fp16 inputs and outputs), with
the logits of the last position only and without the multi-token-prediction head:

    int4  every MatMul and the tied embedding in 4-bit k-quant blocks (`algo_config=k_quant shared_embeddings=true`)
    int8  the same graph with every MatMul overridden to 8 bits, so the embedding stays one table shared with the
          LM head (the builder's own int8 precision stores it apart, 1.38 GB for a 0.8B model)

The builder dispatches the vision-language `Qwen3_5ForConditionalGeneration` only, while decider checkpoints are the
text-only `Qwen3_5ForCausalLM` (model type `qwen3_5_text`). One monkeypatch, no fork: the config the builder reads
names the dispatched architecture, and its position tables stop at Ardana's context window. The weights then load
through `AutoModelForCausalLM` (the decoder's model type is that architecture name, which no class of the builder's
loader map matches), whose own config read asks for its unused arguments too, gets a tuple the patch leaves alone and
so loads the checkpoint as the `Qwen3_5ForCausalLM` it is.
"""
import json
import runpy
import sys

from transformers import AutoConfig

# Ardana's context window (`ardana_core::LoadOptions::default().n_ctx`): the position tables of decider exports end
# there, which leaves every answer the same and the download smaller.
MAX_POSITIONS = 40_960

# Every export: no Hub token, last-position logits only (a decider row ends in its answer slot), no MTP head.
COMMON_OPTIONS = ["hf_token=false", "prune_lm_head=true", "exclude_mtp=true"]

# The projections of a full-attention layer, as the builder names their MatMul nodes.
FULL_ATTENTION_PROJECTIONS = ["attn/q_proj", "attn/k_proj", "attn/v_proj", "attn/o_proj", "mlp/gate_proj",
                              "mlp/up_proj", "mlp/down_proj"]


def int8_target(checkpoint):
    """The int8 `target_options`: the int4 base with every MatMul overridden to int8. Overrides take a preset or an
    exact node name (first match wins); the presets cover the LM head, the linear-attention layers with their MLPs
    and llama.cpp's sensitive layers, the names the remaining full-attention layers."""
    with open(f"{checkpoint}/config.json", encoding="utf-8") as fh:
        config = json.load(fh)
    layer_types = config.get("text_config", config)["layer_types"]
    overrides = [{"match": {"preset": preset}, "type": "int8"}
                 for preset in ("last_matmul", "linear_attn", "mixed_layers")]
    overrides += [{"match": {"name": f"/model/layers.{layer}/{projection}/MatMul"}, "type": "int8"}
                  for layer, kind in enumerate(layer_types) if kind == "full_attention"
                  for projection in FULL_ATTENTION_PROJECTIONS]
    weights = {"type": "int4", "method": "k_quant", "op_types": ["MatMul", "Gather"], "overrides": overrides}
    return {"quant_config": {"weights": weights}}


def patch_decider_config():
    """Makes `AutoConfig` present a text-only Qwen3.5 checkpoint as the architecture the builder dispatches."""
    from_pretrained = AutoConfig.from_pretrained.__func__

    def patched(cls, *args, **kwargs):
        config = from_pretrained(cls, *args, **kwargs)
        # A `(config, unused kwargs)` tuple has no model type and passes unchanged.
        if getattr(config, "model_type", None) == "qwen3_5_text" and config.architectures == ["Qwen3_5ForCausalLM"]:
            config.architectures = ["Qwen3_5ForConditionalGeneration"]
            config.max_position_embeddings = MAX_POSITIONS
        return config

    AutoConfig.from_pretrained = classmethod(patched)


def main(checkpoint, quant, out_dir, cache_dir):
    args = ["-i", checkpoint, "-o", out_dir, "-e", "webgpu", "-c", cache_dir]
    if quant == "int4":
        args += ["-p", "int4", "--extra_options", *COMMON_OPTIONS, "algo_config=k_quant", "shared_embeddings=true"]
    elif quant == "int8":
        args += ["--target_options", json.dumps(int8_target(checkpoint)), "--extra_options", *COMMON_OPTIONS]
    else:
        sys.exit(f"no ONNX export recipe for the quant {quant!r}; the recipes are int4 and int8")
    patch_decider_config()
    sys.argv = ["builder", *args]
    runpy.run_module("onnxruntime_genai.models.builder", run_name="__main__")


if __name__ == "__main__":
    main(*sys.argv[1:])
