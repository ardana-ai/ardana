"""R5.2: the official TypeSafe Python SDK parses Ardana's answers to a request fixture.

Run by `cargo xtask e2e sdk` with the `tmp/py/sdk` venv's interpreter:

    sdk_ticket.py <request.json>

`TYPESAFE_BASE_URL` points the SDK at `ardana serve`; the SDK requires some `TYPESAFE_API_KEY`, which an open server
ignores. The request goes out with model `jev-latest`; the script prints what the SDK parsed and fails unless every
question comes back as an answer of its own type.
"""

import json
import sys

from typesafe_sdk import TypeSafeClient


def main(path: str) -> int:
    with open(path, encoding="utf-8") as fh:
        request = json.load(fh)
    questions = request["questions"]
    with TypeSafeClient(timeout=120.0) as client:
        response = client.system_one(state=request["state"], questions=questions, model="jev-latest")
    by_type = {"noul": response.nouls, "choice": response.choices, "score": response.scores}
    print(f"typesafe-sdk parsed: model={response.model!r} usage={response.usage.model_dump()}")
    missing = []
    for name, question in questions.items():
        answer = by_type[question["type"]].get(name)
        if answer is None:
            missing.append(name)
            continue
        print(f"  {name}: {type(answer).__name__} {answer.model_dump()}")
    if missing:
        print(f"no answer of the question's type for: {', '.join(missing)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
