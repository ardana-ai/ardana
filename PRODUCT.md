# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users
Developers wiring calibrated, typed decisions into agents and automations. They arrive with a concrete state (a
ticket, a transcript, a resume, a JSON record) and a handful of questions they want answered as `choice`, `score` or
`noul`, and they want to see what a local model answers before they put the same request into code.

## Product Purpose
Ardana runs System 1 decision models locally behind a Jev-compatible HTTP API, from one `ardana` binary. A decision is
one forward pass: the model reads the state and every question once and returns calibrated probabilities, never
generated text. The playground bundled in the binary is where a developer tries a state and questions against a local
model, reads the calibrated answers, and copies the exact request into code. Success is a developer who trusts the
numbers enough to wire them in.

## Positioning
Local, one binary, one forward pass per decision, and a drop-in for Jev and TypeSafe clients: the same request that
runs against Jev's hosted API runs against `ardana serve` unchanged, on models the developer holds.

## Operating Context
- The playground is served by `ardana serve` at `/`, same origin as the public API (`/v1/systemone`, `/v1/models`,
  `/health`); it has no remote endpoints in Step 1.
- Developers move between the playground, their editor and a terminal; they copy curl, Python (`typesafe-sdk`) and
  TypeScript (`@typesafe-ai/sdk`) snippets out of it.
- Jev share links (`#share/<lz-string payload>`) from docs.typesafe.ai open in it.
- Models are pulled with `ardana pull` and listed by `/v1/models`: decider-format System 1 models and stock instruct
  models read in chat layout.

## Capabilities and Constraints
- Inputs: state as text or JSON; a `questions` map with `noul` (optional true/false criteria), `choice` (2..255 named
  options) and `score` (2..10 ordered levels) questions; a model picked from `/v1/models`.
- Outputs: per-question answers with probabilities, confidence, noul and score values, `x_` extras, token usage and
  latency; the raw request and response; `detail` of 413 and 422 errors.
- Every displayed value equals the API response it came from.
- Stack: Leptos CSR built with trunk and embedded in the binary; styles live in `.css` files.
- Step 1 excludes remote endpoints, theming, image inputs, and Jev's lesson walkthroughs.

## Brand Commitments
- Jev's playground structure is binding: the state on the left, questions and results on the right.
- The product name is Ardana.

## Evidence on Hand
- Real requests: `tests/fixtures/requests/ticket.json`, `sentiment.json`; JevBench's 231 public items under
  `tmp/src/jevbench/datasets/public/`.
- Real model output from decider-2b, Qwen3.5-0.8B, SmolLM3-3B and Ollama `llama3.2`.
- No customers, testimonials, benchmarks for marketing, or pricing exist; none may be invented.

## Product Principles
- Show the numbers as the API returned them; never round away or restyle a value into something the API did not say.
- The playground is a path to code: whatever a developer sees must be one copy away from a request they can send.
- Local and honest: model, latency and token counts are always visible, errors are shown as the API reports them.

## Accessibility & Inclusion
WCAG 2.2 AA: keyboard-operable throughout, visible focus, sufficient contrast, and respect for reduced motion.
