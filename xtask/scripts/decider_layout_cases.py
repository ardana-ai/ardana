"""Exports the prompt cases of decider's tests/layout_cases.py for crates/ardana-core/tests/prompt.rs (R2.2).

Run by `cargo xtask export-decider`, never by hand: it needs the decider clone at 23579f7 (`cargo xtask fetch`), the
decider-2b tokenizer in tmp/hf and the tmp/py/decider-export venv the step creates.

The script calls decider's own `layout_cases.prompt_cases` and `serve_cases` with the decider-2b tokenizer and records, for
every pinned case, the inputs of the top-level decider call that produced it (the prompt builders are wrapped, not
reimplemented). Values that come from decider's random option sampling (`perms`) and from its /decide schema parsing
(`options`) are taken from decider's own output. It checks every produced item against tests/data/plain_layout_pins.json
and writes, into the output directory:

    layout_cases.json        the recorded inputs, one spec per `prompt:` and `serve:` pin
    plain_layout_pins.json   decider's pins, copied unchanged
    LICENSE                  decider's Apache-2.0 license, copied unchanged

    python decider_layout_cases.py <decider clone> <tokenizer dir> <output dir>
"""
import functools
import json
import shutil
import sys
from pathlib import Path


def main(decider_dir, tokenizer_dir, out_dir):
    decider_dir, out_dir = Path(decider_dir), Path(out_dir)
    sys.path[:0] = [str(decider_dir), str(decider_dir / "tests")]
    import transformers
    import layout_cases as LC
    from decider import prompt as P
    from decider import prompt_fast, serve

    tok = transformers.AutoTokenizer.from_pretrained(tokenizer_dir)
    pins = json.loads((decider_dir / "tests/data/plain_layout_pins.json").read_text())["pins"]

    texts, text_ids = [], {}

    def text(s):
        """Index of a state string in the shared `texts` table (long states repeat across many cases)."""
        if s not in text_ids:
            text_ids[s] = len(texts)
            texts.append(s)
        return text_ids[s]

    states, state_ids = [], {}

    def state(v):
        key = json.dumps(v, sort_keys=True, ensure_ascii=False)
        if key not in state_ids:
            state_ids[key] = len(states)
            states.append(v)
        return state_ids[key]

    calls, depth = [], [0]

    def record(fn, spec_of):
        @functools.wraps(fn)
        def wrapped(*args, **kwargs):
            depth[0] += 1
            try:
                result = fn(*args, **kwargs)
            finally:
                depth[0] -= 1
            if depth[0] == 0:
                calls.append(spec_of(result, *args, **kwargs))
            return result
        return wrapped

    def build_spec(result, ex, tok_, rng=None, max_options=P.NARROW, max_ctx_tokens=1536, layout="state_first", chat=None):
        assert chat is None
        return {"kind": "build", "layout": layout, "context": text(ex.context), "cap": max_ctx_tokens,
                "questions": [[q.text, list(q.options), q.gold] for q in ex.qs], "perms": result["perms"]}

    def prefix_spec(result, tok_, qs, perms=None, chat=None):
        assert chat is None and perms is None
        return {"kind": "schema_prefix", "questions": [[q.text, list(q.options)] for q in qs]}

    def suffix_spec(result, tok_, context, n_q, max_ctx_tokens=1536, chat=None):
        assert chat is None
        return {"kind": "schema_suffix", "context": text(context), "n_q": n_q, "cap": max_ctx_tokens}

    def rows_spec(result, tok_, context, rows, max_ctx_tokens=32768, chat=None):
        assert chat is None
        return {"kind": "build_rows", "context": text(context), "cap": max_ctx_tokens,
                "rows": [[[t, list(o)] for t, o in row] for row in rows]}

    def prepare_spec(result, tok_, st, questions, independent, isolated=False, max_state_tokens=32768, chat=None):
        assert chat is None and questions is LC.QUESTIONS
        return {"kind": "prepare", "state": state(st), "independent": independent, "isolated": isolated,
                "cap": max_state_tokens}

    def decide_spec(result, context, schema):
        qs, _ = result
        assert schema is LC.SCHEMA
        return {"kind": "decide", "context": text(context), "cap": serve.DECIDE_MAX_CTX_TOKENS,
                "questions": [[q["question"], list(q["options"])] for q in qs]}

    P.build = record(P.build, build_spec)
    P.schema_prefix_ids = record(P.schema_prefix_ids, prefix_spec)
    P.schema_suffix_ids = record(P.schema_suffix_ids, suffix_spec)
    prompt_fast.build_rows = record(prompt_fast.build_rows, rows_spec)
    serve.prepare = record(serve.prepare, prepare_spec)
    serve._prepare_decide = record(serve._prepare_decide, decide_spec)

    cases, bad = {}, []

    def collect(prefix, produced):
        assert len(calls) == len(produced), (prefix, len(calls), len(produced))
        for (name, value), spec in zip(produced.items(), calls):
            key = prefix + name
            if pins.get(key) != LC.digest(value):
                bad.append(key)
            cases[key] = spec
        calls.clear()

    collect("prompt:", LC.prompt_cases(tok))
    for neutralize in (True, False):
        collect(f"serve:neutralize={neutralize}:", LC.serve_cases(tok, neutralize=neutralize))
    if bad:
        sys.exit(f"{len(bad)} cases do not match their pins with this tokenizer, e.g. {bad[:5]}")
    counts = {p: sum(k.startswith(p) for k in cases) for p in ("prompt:", "serve:")}
    assert counts == {"prompt:": 149, "serve:": 74}, counts

    out_dir.mkdir(parents=True, exist_ok=True)
    export = {
        "generated_by": "xtask/scripts/decider_layout_cases.py (cargo xtask export-decider)",
        "decider": "Mapika/decider@23579f7a7e8f10e1045be492af3c1c05a005d67c tests/layout_cases.py",
        "tokenizer": f"{Path(tokenizer_dir).parents[1].name}@{Path(tokenizer_dir).name} tokenizer.json",
        "transformers": transformers.__version__,
        "texts": texts,
        "states": states,
        "questions": LC.QUESTIONS,
        "cases": cases,
    }
    (out_dir / "layout_cases.json").write_text(json.dumps(export, ensure_ascii=False, indent=1) + "\n")
    shutil.copyfile(decider_dir / "tests/data/plain_layout_pins.json", out_dir / "plain_layout_pins.json")
    shutil.copyfile(decider_dir / "LICENSE", out_dir / "LICENSE")
    print(f"exported {len(cases)} cases ({counts['prompt:']} prompt, {counts['serve:']} serve), all matching their pins")


if __name__ == "__main__":
    main(*sys.argv[1:])
