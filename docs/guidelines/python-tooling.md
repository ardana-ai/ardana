# Python tooling guidelines

Covers the Python side of Ardana's evaluation harnesses: uv, the uv-managed Python interpreter, the jevcompat uv tool,
the `typesafe-sdk` venv and the JevBench venv that `cargo xtask fetch` installs and `cargo xtask e2e jevcompat`,
`e2e sdk` and `e2e jevbench` run against `ardana serve`. It also covers the decider research clone in
`tmp/src/decider` that W2 runs to export goldens, and the `tmp/py/onnx` venv in which `cargo xtask onnx` exports the
browser variants (its pins and recipe live in `onnx.md`). Ardana itself ships no Python; nothing here is a runtime
dependency.

## Versions
- `uv` 0.7.3 — the only installer and venv manager for Python work; the dev machine's uv, run inside the sandbox env.
- `jevcompat` 0.1.0 — Jev API conformance runner (`mandu5/jevcompat@0a9752b`), installed as a uv tool; R5.1.
- `typesafe-sdk` 0.7.2 — official TypeSafe Python SDK, a library without a command, so it lives in the `tmp/py/sdk` venv; R5.2.
- `python` 3.10 — minimum interpreter for jevcompat and `typesafe-sdk`; a uv-managed CPython under `UV_PYTHON_INSTALL_DIR`.
- JevBench `fd54ea7` — a source clone in `tmp/src/jevbench` (`fstandhartinger/jevbench@fd54ea7`, MIT); it declares no
  dependencies and runs on the standard library, so `tmp/py/jevbench` holds only the interpreter; R5.3.

## Rules

### Environment first
- Run `eval "$(cargo xtask env)"` before any `uv`, `uvx`, `python` or `pip` command you run by hand; xtask steps get
  the same variables through `Sandbox::command`. Never run them with the plain shell environment.
- The sandbox sets, and uv reads: `UV_CACHE_DIR=tmp/cache/uv` (cache, same filesystem as the venvs so uv can
  hardlink), `UV_PYTHON_INSTALL_DIR=tmp/uv/python` (managed interpreters), `UV_TOOL_DIR=tmp/uv/tools` (tool envs),
  `UV_TOOL_BIN_DIR=tmp/bin` (tool executables, already first on `PATH`), `UV_PYTHON_BIN_DIR=tmp/bin` (where uv links
  managed `python3.x` executables: by default from uv 0.8, in preview mode in 0.7.3). pip reads
  `PIP_CACHE_DIR=tmp/cache/pip`.
- Never let uv or pip fall back to their defaults (`~/.cache/uv`, `~/.local/share/uv`, `~/.local/bin`,
  `~/Library/Caches/pip`); the home guard watches those paths and fails the step on any change.
- Do not pass `--cache-dir`, `--no-cache` or ad-hoc location overrides (`UV_*_DIR`) on the command line; change
  `Sandbox::env` if a location must move, so every caller agrees.
- `rm -rf tmp` must be a complete reset: every venv, tool, interpreter and cache below must be recreatable by
  `cargo xtask fetch` from `xtask/fetch.toml` alone.

### Interpreter
- Request the interpreter per command with `--python 3.10` (or newer) and run uv with
  `UV_PYTHON_PREFERENCE=only-managed`, so uv downloads CPython into `UV_PYTHON_INSTALL_DIR` instead of picking up
  macOS's or Homebrew's Python.
- Do not run `uv python install` by hand; let `uv venv` and `uv tool install` fetch the interpreter on demand. Newer uv
  releases (0.8+) also link a `python3.x` executable into `~/.local/bin` on `uv python install`.
- Never `uv python pin` or write `.python-version` files in the repo; the version lives in `fetch.toml` and xtask.

### Tools (`source = "uv-tool"`)
- Install command-line harnesses with `uv tool install`, pinned exactly: a version (`jevcompat==0.1.0`) or, when the
  pin is a commit, `git+<repo-url>@<rev>` with the full or short rev from `fetch.toml`. A `source = "uv-tool"` entry
  runs `uv tool install --force --python 3.10 <name>==<version>`; its check wants `uv tool list` to show
  `<name> v<version>`, the tool env on a uv-managed Python and the executable in `tmp/bin`.
- Each tool gets its own isolated env under `UV_TOOL_DIR`; never `pip install` into it or edit it. Change the pin in
  `fetch.toml` and reinstall with `uv tool install --force` instead.
- Do not use `uvx`/`uv tool run` in xtask or tests: its envs are temporary cache entries and the version is not
  checked by `cargo xtask fetch --check`. Call the installed executable from `tmp/bin`.
- `cargo xtask e2e jevcompat` runs `jevcompat test <url> --json tmp/evals/jevcompat/<run>.json` twice, against an
  open server and against one started with `ARDANA_API_KEY` (the key reaches jevcompat through `--key-env`, never
  argv), so both conditional auth MUSTs are tested; each run must be conformant (every applicable MUST passes).

### Venvs (`source = "uv-venv"`)
- One venv per harness under `tmp/py/<name>`: `tmp/py/sdk` for `typesafe-sdk`, `tmp/py/jevbench` for JevBench's
  dependencies. Create with `uv venv tmp/py/<name> --python 3.10` (managed-only preference as above).
- Install with `uv pip install --python tmp/py/<name>/bin/python <pkg>==<version>` (or `-r <file>` from the
  clone); never rely on `VIRTUAL_ENV` or a discovered `.venv`, and never use `--system`. A `source = "uv-venv"` entry
  names the venv in `path` (inside `tmp/`) and installs `<name>==<version>`, or, when `url` names a requirements
  source inside `tmp/` (`src/jevbench/pyproject.toml`), that source's declared dependencies; the check for such a
  venv is `uv pip install --dry-run --offline -r <source>` reporting "Would make no changes".
- Use pip only through `uv pip`. uv venvs have no pip unless created with `--seed`; do not seed them, and never run
  `python -m pip` or a bare `pip`.
- Run code with the venv's interpreter directly (`tmp/py/sdk/bin/python ...`); do not `source .../activate` in xtask.
- Treat venvs as disposable: they embed absolute paths, so never move, copy or commit them; recreate them instead.
- JevBench stays a source clone at `tmp/src/jevbench` (`fd54ea7`); install only its declared dependencies into
  `tmp/py/jevbench` and run `tmp/py/jevbench/bin/python -m jevbench.cli run --adapter typesafe ...` from the clone,
  passing `--results`, `--ledger` and `--raw-dir` paths under `tmp/evals/jevbench/`.
- The SDK check (`xtask/scripts/sdk_ticket.py`, run with `tmp/py/sdk/bin/python`) sets `TYPESAFE_BASE_URL` to the
  local server and uses model `jev-latest`; the SDK refuses to start without some `TYPESAFE_API_KEY`, so it gets a
  placeholder the open server ignores. The playground's `snippets` case (Playwright) runs the Python snippet with the same
  interpreter, `TYPESAFE_BASE_URL` and placeholder key. Never point it at the hosted TypeSafe API and never put a real API key in env,
  files or logs.
- `cargo xtask e2e jevbench` empties `tmp/evals/jevbench/` first (JevBench opens results, ledger and raw files
  exclusively), runs `run --adapter typesafe --key-env ""` over `datasets/public/{easy,original,hard}.jsonl`, writes
  `summarize`'s output to `summary.json` and fails on any failed or missing request; accuracy is reported only.

### Research clones
- Clone Python sources for reading, golden export or conversion (decider, jevcompat, JevBench, llama.cpp) only into
  `tmp/src/<name>` at the pinned rev, and run them with a venv under `tmp/py/<name>`, never with a global interpreter.
- Vendored outputs (for example `crates/ardana-core/tests/data/decider/*.json`) are committed with their upstream
  license notice; the clone and venv that produced them are not.
- `cargo xtask export-decider` regenerates the decider goldens: it creates `tmp/py/decider-export` with a managed
  CPython 3.12 (`UV_PYTHON_PREFERENCE=only-managed`; decider needs >= 3.11, and 3.12 is the first CPython whose
  float `sum()` is compensated, which Ardana's readout reproduces), installs the pinned `torch`, `transformers`,
  `numpy`, `fastapi`, `jinja2` and `huggingface-hub` its `layout_cases.py` imports, runs
  `xtask/scripts/decider_layout_cases.py` against `tmp/src/decider` offline, and recreates the venv whenever
  `pyvenv.cfg` points outside `tmp/uv/python`. decider itself is imported from the clone, never installed into it.
- A step that needs its own venv outside `fetch.toml` creates it with `python::pinned_venv` (a managed CPython, exact
  `<name>==<version>` pins, recreated when `pyvenv.cfg` points outside `tmp/uv/python`): `export-decider`, and
  `cargo xtask onnx convert`, which creates `tmp/py/onnx` on CPython 3.12, downloads checkpoints with its `hf` CLI,
  then runs `xtask/scripts/onnx_export.py` and the llama.cpp clone's `convert_hf_to_gguf.py` (with the clone's
  `gguf-py`, never installed) offline. Its `hf` sees no token in the sandbox; only `cargo xtask onnx publish` hands it
  the user's, as `HF_TOKEN`.

### Checks
- `cargo xtask fetch --check` must fail, naming the entry, when a tool is missing from `tmp/bin`, a venv lacks its
  pinned package version, or the interpreter is older than 3.10; check with the venv's own interpreter
  (`tmp/py/<name>/bin/python -c` reading `importlib.metadata.version("<pkg>")`), not with a global `pip`.

## Sources
- https://docs.astral.sh/uv/reference/environment/ — `UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, `UV_TOOL_DIR`, `UV_TOOL_BIN_DIR`, `UV_PYTHON_BIN_DIR`, `UV_PYTHON_PREFERENCE` and the uv version each was added in
- https://docs.astral.sh/uv/concepts/cache/ — cache location order, same-filesystem hardlinking, never edit the cache
- https://docs.astral.sh/uv/concepts/tools/ — `uv tool install` vs `uvx`, isolated tool envs, tool and executable directories
- https://docs.astral.sh/uv/guides/tools/ — pinning tools, installing from `git+<url>@<rev>`, `--python`
- https://docs.astral.sh/uv/concepts/python-versions/ — managed vs system Python, automatic downloads, `python-preference`, version requests
- https://docs.astral.sh/uv/pip/environments/ — `uv venv <path> --python`, environment discovery, `--python` targeting, `--system`
- https://docs.astral.sh/uv/reference/cli/ — `uv venv --seed`, `uv pip install --python`, `uv tool install --force`
- https://github.com/astral-sh/uv/releases/tag/0.7.3 — the pinned uv release
- https://github.com/astral-sh/uv/releases/tag/0.8.0 — `uv python install` links executables into `~/.local/bin` by default
- https://pip.pypa.io/en/stable/topics/caching/ — pip's default macOS cache and `PIP_CACHE_DIR`
- https://docs.python.org/3/library/venv.html — venvs are disposable, not relocatable, not committed; run without activation
