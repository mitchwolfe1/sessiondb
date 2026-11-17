"""Validate the public contract, synthetic fixtures, and repository documentation."""
from pathlib import Path
from urllib.parse import unquote
import base64
import json
import re
import struct
import subprocess
import tomllib
import zlib

import yaml
from jsonschema import Draft202012Validator, FormatChecker
from openapi_spec_validator import validate
from referencing import Registry, Resource
from referencing.jsonschema import DRAFT202012

ROOT = Path(__file__).resolve().parents[1]
SPEC = yaml.safe_load((ROOT / "docs/openapi.yaml").read_text())
validate(SPEC)


def validator(name):
    return Draft202012Validator(
        {"$ref": f"#/components/schemas/{name}", "components": SPEC["components"]},
        format_checker=FormatChecker(),
    )


EXAMPLES = {
    "session.json": "Session", "session-unassigned.json": "Session",
    "session-update.json": "SessionPatch", "source-create.json": "SourceCreate",
    "search-request.json": "SearchRequest", "search-response.json": "SearchResponse",
    "import-request.json": "ImportManifest", "import-response.json": "Import",
    "import-accepted.json": "Import", "embedding-profile.json": "EmbeddingProfileCreate",
    "problem.json": "Problem", "message.json": "Message",
    "content-reference.json": "ContentReferenceBlock",
}
assert {path.name for path in ROOT.glob("docs/examples/*.json")} == set(EXAMPLES)
examples = {}
for filename, name in EXAMPLES.items():
    examples[filename] = json.loads((ROOT / "docs/examples" / filename).read_text())
    validator(name).validate(examples[filename])
for name, schema in SPEC["components"]["schemas"].items():
    for example in schema.get("examples", []):
        validator(name).validate(example)

for path in [*ROOT.glob("docs/examples/*.toml"), ROOT / "dev/server.toml"]:
    config = tomllib.loads(path.read_text())
    if path.name == "client.toml":
        assert all(set(context) == {"server"} for context in config["contexts"].values())
        assert all(set(source) == {"context", "harness", "root"} for source in config["sources"])
    else:
        assert config["auth"] == {"api_key_env": "SESSIONDB_API_KEY"}
        assert "bootstrap" not in config

# Response extensibility must not reintroduce identity policy in examples or schemas.
identity_fields = {
    "organization_id", "org_id", "owner_id", "creator_id", "author_id", "author_ids",
    "team_id", "user_id", "default_visibility", "visibility", "principal_id", "principal_type",
}

def check_no_identity(value):
    if isinstance(value, dict):
        assert not identity_fields.intersection(value), identity_fields.intersection(value)
        for child in value.values():
            check_no_identity(child)
    elif isinstance(value, list):
        for child in value:
            check_no_identity(child)

check_no_identity(SPEC)
check_no_identity(examples)
removed_schemas = {
    "AuthConfig", "User", "Organization", "OrganizationPatch", "Membership", "MembershipPatch",
    "Me", "Invitation", "InvitationCreate", "Team", "TeamCreate", "TeamPatch", "TeamMember",
    "Token", "TokenCreate", "CreatedToken", "SessionGrant", "SessionGrantCreate",
}
assert not any(name.removesuffix("Page") in removed_schemas for name in SPEC["components"]["schemas"])
assert set(SPEC["components"]["schemas"]["Source"]["properties"]) == {
    "id", "harness", "device_label", "paused", "last_synced_at", "root",
}
assert set(SPEC["components"]["schemas"]["SourceCreate"]["properties"]) == {
    "harness", "device_label", "root",
}
assert set(SPEC["components"]["schemas"]["Session"]["properties"]) == {
    "id", "project_ids", "harness", "native_session_id", "title", "tags", "recorded_cwd",
    "started_at", "updated_at", "message_count", "processing", "relations", "warnings",
}
assert set(SPEC["components"]["schemas"]["Import"]["properties"]) == {
    "id", "source_id", "status", "uploads", "results", "created_at", "job_id", "accepted_at",
    "errors", "upload_expires_at",
}
for name, sample, field in [
    ("SourceCreate", examples["source-create.json"], "default_visibility"),
    ("SessionPatch", examples["session-update.json"], "visibility"),
    ("SearchFilters", {}, "author_ids"),
    ("SearchFilters", {}, "team_id"),
]:
    assert list(validator(name).iter_errors({**sample, field: "removed"}))
assert examples["session-unassigned.json"]["project_ids"] == []
assert examples["import-response.json"]["source_id"] == examples["import-request.json"]["source_id"]
assert examples["import-accepted.json"]["id"] == examples["import-response.json"]["id"]
assert examples["import-response.json"]["accepted_at"] is None
assert examples["import-accepted.json"]["accepted_at"] is not None
assert list(validator("ContentBlock").iter_errors({"type": "tool_call", "name": "bash"}))
reference = json.loads((ROOT / "docs/examples/content-reference.json").read_text())
assert list(validator("FullContentBlock").iter_errors(reference))

schema_path = ROOT / "fixtures/expectation.schema.json"
schema = json.loads(schema_path.read_text())
schema["$id"] = schema_path.as_uri()
registry = Registry().with_resources([
    ((ROOT / "docs/openapi.yaml").as_uri(), Resource.from_contents(SPEC, default_specification=DRAFT202012)),
    (schema_path.as_uri(), Resource.from_contents(schema)),
])
expected_validator = Draft202012Validator(schema, registry=registry, format_checker=FormatChecker())
manifest = json.loads((ROOT / "fixtures/manifest.json").read_text())
native_files = messages = 0
for case in manifest["cases"]:
    expected = json.loads((ROOT / "fixtures" / case["expected"]).read_text())
    expected_validator.validate(expected)
    assert expected["case"] == case["id"]
    assert set(case["native_paths"]) == set(case["inputs"])
    anchors = set()
    for name in case["inputs"]:
        data = (ROOT / "fixtures" / name).read_bytes()
        end = data.rfind(b"\n") + 1
        bad = []
        for line_number, line in enumerate(data[:end].splitlines(), 1):
            anchors.add(f"line:{line_number}")
            try:
                record = json.loads(line)
            except ValueError:
                bad.append(line_number)
                continue
            for obj in [record, record.get("payload", {})]:
                for key in ["uuid", "id"]:
                    if isinstance(obj.get(key), str):
                        anchors.add(obj[key])
            boundary = record.get("payload", {}).get("history_base")
            if record.get("type") == "session_meta" and boundary:
                parents = [ROOT / "fixtures" / p for p in case["inputs"] if p != name]
                parents = [p for p in parents if json.loads(p.read_bytes().splitlines()[0])["payload"]["id"] == boundary["thread_id"]]
                assert len(parents) == 1 and parents[0].stat().st_size == boundary["end_byte_offset"]
            content = record.get("message", {}).get("content", [])
            for block in content if isinstance(content, list) else []:
                if block.get("type") == "image":
                    image = base64.b64decode(block["data"])
                    assert image[:8] == b"\x89PNG\r\n\x1a\n"
                    offset = 8
                    while offset < len(image):
                        size = struct.unpack(">I", image[offset:offset + 4])[0]
                        chunk = image[offset + 4:offset + 8 + size]
                        crc = struct.unpack(">I", image[offset + 8 + size:offset + 12 + size])[0]
                        assert zlib.crc32(chunk) == crc
                        offset += size + 12
                    assert offset == len(image)
        assert bad == case.get("allow_malformed_lines", [])
        if len(data) != end:
            assert case.get("allow_incomplete_tail")
        if expected.get("capture"):
            assert expected["capture"]["complete_lines"] == data[:end].count(b"\n")
            assert expected["capture"]["pending_tail_bytes"] == len(data) - end
        native_files += 1
    for session in expected["sessions"]:
        by_key = {message["key"]: message for message in session["messages"]}
        assert len(by_key) == len(session["messages"])
        messages += len(by_key)
        for message in session["messages"]:
            assert set(message["native_ids"]) <= anchors
            parent = message["parent_key"]
            seen = {message["key"]}
            while parent is not None:
                assert parent in by_key and parent not in seen
                seen.add(parent)
                parent = by_key[parent]["parent_key"]

operations = {(method.upper(), path) for path, item in SPEC["paths"].items() for method in item if method != "parameters"}
inventory = set()
for line in (ROOT / "docs/api.md").read_text().splitlines():
    match = re.match(r"^\| ((?:GET|POST|PUT|PATCH|DELETE)(?:, (?:GET|POST|PUT|PATCH|DELETE))*) \| `([^`]+)`", line)
    if match:
        methods, path = match.groups()
        if not path.startswith(("/v1", "/healthz", "/readyz")):
            path = "/v1" + path
        inventory.update((method, path) for method in methods.split(", "))
assert inventory == operations
first = {op["operationId"] for item in SPEC["paths"].values() for method, op in item.items() if method != "parameters" and op.get("x-first-build")}
implementation = {op["operationId"] for item in SPEC["paths"].values() for method, op in item.items() if method != "parameters" and op.get("x-implemented")}
source = (ROOT / "crates/types/src/lib.rs").read_text().split("FIRST_BUILD_OPERATIONS:", 1)[1]
assert first == set(re.findall(r'"([a-zA-Z]+)"', source))
assert len(first) == 18
assert implementation == first | {"getArtifact", "getArtifactContent"}
assert SPEC["security"] == [{"bearerAuth": []}]
assert set(SPEC["components"]["securitySchemes"]) == {"bearerAuth"}
assert SPEC["components"]["securitySchemes"]["bearerAuth"]["type"] == "http"
assert SPEC["components"]["securitySchemes"]["bearerAuth"]["scheme"] == "bearer"
for path, item in SPEC["paths"].items():
    assert path in {"/healthz", "/readyz"} or path.startswith("/v1/")
    assert not re.search(r"/(?:auth|me|organizations|members|teams|invitations|tokens|grants)(?:/|$)", path)
    for method, operation in item.items():
        if method == "parameters":
            continue
        effective_security = operation.get("security", SPEC["security"])
        assert effective_security == ([] if path in {"/healthz", "/readyz"} else [{"bearerAuth": []}])
        assert not any(parameter["name"] in identity_fields for parameter in operation.get("parameters", []))


def heading_anchors(path):
    result, seen, fenced = set(), {}, False
    for line in path.read_text().splitlines():
        if line.startswith("```"):
            fenced = not fenced
        if not fenced and (match := re.match(r"^#{1,6} (.+)$", line)):
            text = re.sub(r"[`*_]", "", match[1]).lower()
            slug = "".join(c for c in text if c.isalnum() or c in " -_").replace(" ", "-")
            count = seen.get(slug, 0)
            seen[slug] = count + 1
            result.add(slug + (f"-{count}" if count else ""))
    return result


files = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard"], cwd=ROOT, text=True).splitlines()
links = 0
for filename in files:
    path = ROOT / filename
    if path.suffix != ".md":
        continue
    body = path.read_text()
    assert sum(line.startswith("```") for line in body.splitlines()) % 2 == 0
    for target in re.findall(r"(?<!!)\[[^\]]+\]\(([^)]+)\)", body):
        if re.match(r"^[a-z]+:", target):
            continue
        name, _, fragment = target.partition("#")
        destination = (path.parent / unquote(name)).resolve() if name else path
        assert destination.exists(), (filename, target)
        if fragment and destination.suffix == ".md":
            assert unquote(fragment) in heading_anchors(destination), (filename, target)
        links += 1

print(f"PASS: {len(operations)} target operations, {len(implementation)} implemented operations, {len(EXAMPLES)} API examples")
print(f"PASS: {len(manifest['cases'])} native cases, {native_files} native files, {messages} expected messages")
print(f"PASS: {links} documentation links; JSON, TOML, OpenAPI, fixture anchors and PNG structure")
