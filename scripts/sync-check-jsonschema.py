# /// script
# requires-python = ">=3.12"
# ///
"""Update the schemas embedded in the `check-jsonschema` builtin hook.

Port of upstream check-jsonschema `scripts/vendor-schemas.py`: every vendored schema is
downloaded from its publisher, normalized, and written only when its sha256 changed.
The source URLs are those of upstream `src/check_jsonschema/catalog.py`.

Usage: uv run scripts/sync-check-jsonschema.py [NAME ...]
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import urllib.request
from pathlib import Path

VENDOR_DIR = Path(
    "crates/prek/src/hooks/builtin_hooks/check_jsonschema/schemas/vendor"
)

# Kept in sync with upstream `SCHEMA_CATALOG`, in alphabetical order by name.
SCHEMA_URLS = {
    "azure-pipelines": "https://raw.githubusercontent.com/microsoft/azure-pipelines-vscode/main/service-schema.json",
    "bamboo-spec": "https://json.schemastore.org/bamboo-spec.json",
    "bitbucket-pipelines": "https://api.bitbucket.org/schemas/pipelines-configuration",
    "buildkite": "https://raw.githubusercontent.com/buildkite/pipeline-schema/main/schema.json",
    "changie": "https://changie.dev/schema.json",
    "circle-ci": "https://raw.githubusercontent.com/CircleCI-Public/circleci-yaml-language-server/main/schema.json",
    "citation-file-format": "https://raw.githubusercontent.com/citation-file-format/citation-file-format/main/schema.json",
    "cloudbuild": "https://json.schemastore.org/cloudbuild.json",
    "codecov": "https://www.schemastore.org/codecov.json",
    "compose-spec": "https://raw.githubusercontent.com/compose-spec/compose-spec/master/schema/compose-spec.json",
    "dependabot": "https://json.schemastore.org/dependabot-2.0.json",
    "drone-ci": "https://json.schemastore.org/drone.json",
    "github-actions": "https://json.schemastore.org/github-action",
    "github-discussion": "https://www.schemastore.org/github-discussion.json",
    "github-issue-config": "https://www.schemastore.org/github-issue-config.json",
    "github-issue-forms": "https://www.schemastore.org/github-issue-forms.json",
    "github-workflows": "https://json.schemastore.org/github-workflow",
    "gitlab-ci": "https://gitlab.com/api/v4/projects/gitlab-org%2Fgitlab/repository/files/app%2Fassets%2Fjavascripts%2Feditor%2Fschema%2Fci.json/raw?ref=master",
    "meltano": "https://raw.githubusercontent.com/meltano/meltano/main/src/meltano/schemas/meltano.schema.json",
    "mergify": "https://docs.mergify.com/mergify-configuration-schema.json",
    "readthedocs": "https://raw.githubusercontent.com/readthedocs/readthedocs.org/main/readthedocs/rtd_tests/fixtures/spec/v2/schema.json",
    "renovate": "https://docs.renovatebot.com/renovate-schema.json",
    "snapcraft": "https://raw.githubusercontent.com/canonical/snapcraft/main/schema/snapcraft.json",
    "taskfile": "https://taskfile.dev/schema.json",
    "travis": "https://json.schemastore.org/travis",
    "woodpecker-ci": "https://raw.githubusercontent.com/woodpecker-ci/woodpecker/main/pipeline/frontend/yaml/linter/schema/schema.json",
}


def run(cmd: list[str], *, capture: bool = False) -> str:
    result = subprocess.run(
        cmd,
        check=True,
        text=True,
        capture_output=capture,
    )
    if capture:
        return result.stdout.strip()
    return ""


def repo_root() -> Path:
    root = run(["git", "rev-parse", "--show-toplevel"], capture=True)
    return Path(root)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalize_schema_contents(data: bytes) -> bytes:
    """Strips trailing spaces like upstream, and applies what upstream's `pre-commit run`
    step did for prek's own hooks: LF line endings and a single final newline."""
    lines = data.replace(b"\r\n", b"\n").split(b"\n")
    content = b"\n".join(line.rstrip(b" \t") for line in lines)
    return content.rstrip(b"\n") + b"\n"


def download(url: str) -> bytes:
    request = urllib.request.Request(
        url, headers={"User-Agent": "prek-sync-check-jsonschema"}
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def update_schemas(vendor_dir: Path, names: list[str]) -> list[str]:
    print("downloading schemas to check for updates")
    updated = []
    for name in names:
        url = SCHEMA_URLS[name]
        print(f"  {name} ({url})")
        new_content = normalize_schema_contents(download(url))
        path = vendor_dir / f"{name}.json"
        # Skip schemas whose downloaded content matches the embedded one.
        if path.exists() and digest(path.read_bytes()) == digest(new_content):
            continue
        path.write_bytes(new_content)
        updated.append(name)
    return updated


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "names", nargs="*", metavar="NAME", help="schemas to update (default: all)"
    )
    args = parser.parse_args()
    if unknown := sorted(set(args.names) - set(SCHEMA_URLS)):
        parser.error(f"unknown schemas: {', '.join(unknown)}")

    vendor_dir = repo_root() / VENDOR_DIR
    updated = update_schemas(vendor_dir, args.names or list(SCHEMA_URLS))
    if updated:
        print(f"updated schemas: {', '.join(updated)}")
    else:
        print("no changes detected")


if __name__ == "__main__":
    main()
