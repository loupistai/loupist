#!/usr/bin/env python3
"""Refresh the frozen model calls in stages.json after a text change.

The golden tests replay each stage and compare the request Rust builds with the
one frozen here. When a prompt template, the policy, the instructions or a
response-schema description changes, run this from the repository:

    python3 vendor_prompts.py            # rewrite stages.json
    python3 vendor_prompts.py --check    # exit 1 if stages.json is out of date

It reads the previous text from git (--from, default HEAD) and the current text
from the working tree, finds each frozen message's slot values by matching the
previous template, and renders the current template with them. A changed
constant, type description or field description is replaced where it was
frozen. Replies, results and every other fixture value are left as they are.
Review the diff, then run the Rust golden tests: they are the check that this
file and the code agree.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
EXTRACT = HERE.parent
STAGES = HERE / "stages.json"
SCHEMA_MARKER = "\n\nRespond with a JSON object in the following format:\n\n"
TEMPLATES = ["extract_text", "dedupe_nodes", "extract_edges", "resolve_edge", "extract_summaries_batch"]
CONSTANTS = {
    "ontology.rs": ["WORLD_FACT_POLICY", "ENGLISH_EXTRACTION_RULE", "EXTRACTION_INSTRUCTIONS"],
    "prompts.rs": [
        "DO_NOT_ESCAPE_UNICODE",
        "EDGE_BATCH_INSTRUCTIONS",
        "END_VALIDITY_INSTRUCTIONS",
        "ATTRIBUTE_INSTRUCTIONS",
    ],
}
# `("name", "description")` rows in ontology.rs: type fields and relation types.
DESCRIBED = re.compile(r'\(\s*"(\w+)"\s*,\s*"((?:[^"\\]|\\.)*)"\s*\)')
TYPE_DOC = re.compile(r'^const ([A-Z_]+): &str\s*=\s*"((?:[^"\\]|\\.)*)";', re.MULTILINE)
SLOT = re.compile(r"\{\{(\w+)\}\}")
FIELD = re.compile(
    r"(string_field|optional_string_field|integer_list_field|integer_field|model_list_field)\(\s*"
    r'"(\w+)"\s*,\s*(?:"(\w+)"\s*,\s*)?(None|(?:Some\(\s*)?"((?:[^"\\]|\\.)*)")'
)


class Stale(Exception):
    """A frozen message that no template of the given text renders."""


class Text:
    """The model-facing text of one revision: the working tree, or a git revision."""

    def __init__(self, revision=None):
        self.revision = revision
        self.templates = {
            name: {role: self.read(f"prompts/{name}.{role}.txt") for role in ("system", "user")}
            for name in TEMPLATES
        }
        self.constants = {
            name: rust_constant(self.read(file), name)
            for file, names in CONSTANTS.items()
            for name in names
        }
        self.models = schema_models(self.read("schema.rs"))
        ontology = self.read("ontology.rs")
        self.descriptions = [
            (name, rust_string(text)) for name, text in TYPE_DOC.findall(ontology) + DESCRIBED.findall(ontology)
        ]
        self.stages = self.read("golden/stages.json")

    def read(self, relative):
        path = EXTRACT / relative
        if self.revision is None:
            return path.read_text(encoding="utf-8")
        top = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], cwd=HERE, capture_output=True, text=True, check=True
        ).stdout.strip()
        shown = subprocess.run(
            ["git", "show", f"{self.revision}:{path.relative_to(top).as_posix()}"],
            cwd=top,
            capture_output=True,
            check=True,
        )
        return shown.stdout.decode("utf-8")


def rust_string(literal):
    """The value of a Rust string literal's body."""
    out, index = [], 0
    while index < len(literal):
        char = literal[index]
        if char != "\\":
            out.append(char)
            index += 1
            continue
        escape = literal[index + 1]
        index += 2
        if escape == "u":
            end = literal.index("}", index)
            out.append(chr(int(literal[index + 1 : end], 16)))
            index = end + 1
        elif escape == "\n":
            while index < len(literal) and literal[index].isspace():
                index += 1
        else:
            out.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(escape, escape))
    return "".join(out)


def rust_constant(source, name):
    """A `const NAME: &str`, written as one literal or as `concat!(...)`."""
    start = re.search(rf"const {name}: &str\s*=", source)
    if start is None:
        raise SystemExit(f"constant {name} not found")
    index, parts = start.end(), []
    while source[index] != ";":
        if source[index] == '"':
            end = index + 1
            while source[end] != '"':
                end += 2 if source[end] == "\\" else 1
            parts.append(rust_string(source[index + 1 : end]))
            index = end
        index += 1
    return "".join(parts)


def schema_models(source):
    """Each static response model's fields: title -> [(helper, name, ref, description)]."""
    models = {}
    for found in re.finditer(r'model\(\s*"(\w+)"\s*,\s*vec!\[', source):
        index, depth = found.end(), 1
        while depth:
            char = source[index]
            if char == '"':
                index += 1
                while source[index] != '"':
                    index += 2 if source[index] == "\\" else 1
            elif char in "[(":
                depth += 1
            elif char in "])":
                depth -= 1
            index += 1
        fields = []
        for helper, name, ref, described, text in FIELD.findall(source[found.end() : index]):
            fields.append((helper, name, ref, None if described == "None" else rust_string(text)))
        models[found.group(1)] = fields
    return models


def property_of(helper, name, ref, description):
    """The JSON schema Pydantic gives a field, as `schema.rs` builds it."""
    described = {} if description is None else {"description": description}
    title = {"title": name.title().replace("_", " ")}
    if helper == "string_field":
        return {**described, **title, "type": "string"}
    if helper == "integer_field":
        return {**described, **title, "type": "integer"}
    if helper == "integer_list_field":
        return {**described, "items": {"type": "integer"}, **title, "type": "array"}
    if helper == "optional_string_field":
        return {"anyOf": [{"type": "string"}, {"type": "null"}], "default": None, **described, **title}
    return {**described, "items": {"$ref": f"#/$defs/{ref}"}, **title, "type": "array"}


def refresh_schema(node, models):
    if isinstance(node, list):
        for item in node:
            refresh_schema(item, models)
        return
    if not isinstance(node, dict):
        return
    for value in node.values():
        refresh_schema(value, models)
    fields = models.get(node.get("title"))
    properties = node.get("properties")
    if fields is None or not isinstance(properties, dict):
        return
    declared = [name for _, name, _, _ in fields]
    if not set(properties) <= set(declared):
        return
    fresh = {}
    for helper, name, ref, description in fields:
        if name in properties:
            fresh[name] = properties[name]
            if description is not None:
                fresh[name]["description"] = description
        else:
            fresh[name] = property_of(helper, name, ref, description)
    properties.clear()
    properties.update(fresh)


def pattern_of(template):
    """A regex that reads a rendered template back into its slot values."""
    out, seen, index = [], set(), 0
    for slot in SLOT.finditer(template):
        out.append(re.escape(template[index : slot.start()]))
        name = slot.group(1)
        out.append(f"(?P={name})" if name in seen else f"(?P<{name}>.*?)")
        seen.add(name)
        index = slot.end()
    out.append(re.escape(template[index:]))
    return re.compile("".join(out), re.DOTALL)


def escaped(text, ascii_only=False):
    """A string as it reads inside JSON text."""
    return json.dumps(text, ensure_ascii=ascii_only)[1:-1]


def render(template, slots):
    return SLOT.sub(lambda slot: slots[slot.group(1)], template)


class Refresh:
    def __init__(self, old, new, schemas=True):
        self.old, self.new = old, new
        self.models = new.models if schemas else {}
        if [name for name, _ in old.descriptions] != [name for name, _ in new.descriptions]:
            raise SystemExit("ontology.rs gained, lost or reordered a description: refresh stages.json by hand")
        changed = [(old.constants[name], new.constants[name]) for name in old.constants]
        changed += [(before, after) for (_, before), (_, after) in zip(old.descriptions, new.descriptions)]
        # A description is frozen as written, or once more inside JSON a message carries.
        self.constants = sorted(
            {
                (form(before), form(after))
                for before, after in changed
                if before != after
                for form in (str, escaped, lambda text: escaped(text, ascii_only=True))
            },
            key=lambda pair: -len(pair[0]),
        )
        self.users = [
            (pattern_of(old.templates[name]["user"]), new.templates[name]["user"]) for name in TEMPLATES
        ]
        self.systems = sorted(
            ((old.templates[name]["system"], new.templates[name]["system"]) for name in TEMPLATES),
            key=lambda pair: -len(pair[0]),
        )
        self.current = [pattern_of(new.templates[name]["user"]) for name in TEMPLATES]

    def text(self, content):
        for before, after in self.constants:
            content = content.replace(before, after)
        return content

    def system(self, content):
        content = self.text(content)
        for before, after in self.systems:
            if before != after and before in content:
                return content.replace(before, after, 1)
        return content

    def user(self, content, templated):
        body, marker, schema = content.partition(SCHEMA_MARKER)
        if marker:
            schema = self.embedded(self.text(schema), lambda value: refresh_schema(value, self.models))
        if body.startswith('{"case_0": ['):
            body = self.embedded(body, self.cases)
        elif templated:
            body = self.template(self.text(body))
        else:
            body = self.text(body)
        return body + marker + schema

    def template(self, body):
        for pattern, template in self.users:
            found = pattern.fullmatch(body)
            if found:
                return render(template, found.groupdict())
        if any(pattern.fullmatch(body) for pattern in self.current):
            return body
        raise Stale(f"no template matches {body[:60]!r}")

    def cases(self, cases):
        for messages in cases.values():
            self.messages(messages, embedded=True)

    def embedded(self, text, change):
        """Change JSON written inside a message, keeping how it was written."""
        value = json.loads(text)
        for ascii_only in (True, False):
            if json.dumps(value, ensure_ascii=ascii_only) == text:
                change(value)
                return json.dumps(value, ensure_ascii=ascii_only)
        raise SystemExit("embedded JSON is not in json.dumps form")

    def messages(self, messages, embedded=False, templated=True):
        for message in messages:
            if message["role"] == "system":
                message["content"] = self.system(message["content"])
            elif embedded:
                message["content"] = self.template(self.text(message["content"]))
            else:
                message["content"] = self.user(message["content"], templated)

    def strings(self, node):
        if isinstance(node, str):
            return self.text(node)
        if isinstance(node, list):
            return [self.strings(item) for item in node]
        if isinstance(node, dict):
            return {key: self.strings(value) for key, value in node.items()}
        return node

    def stages(self, stages):
        for case in stages["cases"]:
            for call in case["calls"]:
                # The attribute call's user message is built in code, without a template.
                templated = call["response_schema"].get("title") != "CoreAttributeBatch"
                self.messages(call["messages"], templated=templated)
                call["response_schema"] = self.strings(call["response_schema"])
                refresh_schema(call["response_schema"], self.models)
        before = self.old.constants["EXTRACTION_INSTRUCTIONS"]
        after = self.new.constants["EXTRACTION_INSTRUCTIONS"]
        for row in stages["scalars"]["instructions"]:
            if row["text"].startswith(before):
                row["text"] = after + row["text"][len(before) :]
        return stages


def dump(stages):
    return json.dumps(stages, indent=1, ensure_ascii=False) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--from", dest="revision", default="HEAD", help="git revision holding the previous text")
    parser.add_argument("--check", action="store_true", help="report whether stages.json is current; write nothing")
    args = parser.parse_args()

    frozen = STAGES.read_text(encoding="utf-8")
    old = Text(args.revision)
    fresh = refreshed(frozen, old, Text())
    if args.check:
        sys.exit(0 if fresh == frozen else 1)
    STAGES.write_text(fresh, encoding="utf-8")
    print(f"{STAGES.name}: {'updated' if fresh != frozen else 'unchanged'}")


def replays(frozen, text):
    """Whether rendering `text` over a file frozen from it changes nothing.

    The schemas are left out: a field the code gained since the file was frozen
    is added only by the refresh.
    """
    try:
        return dump(Refresh(text, text, schemas=False).stages(json.loads(frozen))) == frozen
    except Stale:
        return False


def refreshed(frozen, old, new):
    # The working file when it still holds the previous text (so a fixture
    # edited by hand is kept); otherwise the previous revision's file, which a
    # refresh already replaced.
    base = frozen if replays(frozen, old) else old.stages
    if not replays(base, old):
        # A slot was read back wrongly, and the refresh cannot be trusted.
        raise SystemExit(f"stages.json does not replay against {old.revision}")
    return dump(Refresh(old, new).stages(json.loads(base)))


if __name__ == "__main__":
    main()
