# check-jsonschema fixtures

Copied from [python-jsonschema/check-jsonschema](https://github.com/python-jsonschema/check-jsonschema)
at `56b62a433c1aaffa83522aa3667127959008f55f` (Apache-2.0):

- `example-files/` is upstream `tests/example-files/`.
- `pre-commit-hooks.yaml` is upstream `.pre-commit-hooks.yaml`.

The builtin `check-jsonschema` tests run every upstream hook entry against these files and
expect the same exit codes as upstream `tests/acceptance/test_example_files.py`.

`parity/{positive,negative}/<hook>/` adds a passing and a failing file for every upstream hook
that upstream's corpus does not cover. Each file was checked against Python check-jsonschema at
the commit above with `verify_parity.py`, which runs the upstream hook entries and expects exit 0
for `positive` and 1 for `negative`:

```sh
uv venv /tmp/cj && uv pip install --python /tmp/cj/bin/python check-jsonschema
/tmp/cj/bin/python verify_parity.py . /tmp/cj/bin/check-jsonschema
```

The Rust test `hook_examples` runs the same files and expects the same exit codes.
