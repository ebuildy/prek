# check-jsonschema fixtures

Copied from [python-jsonschema/check-jsonschema](https://github.com/python-jsonschema/check-jsonschema)
at `56b62a433c1aaffa83522aa3667127959008f55f` (Apache-2.0):

- `example-files/` is upstream `tests/example-files/`.
- `pre-commit-hooks.yaml` is upstream `.pre-commit-hooks.yaml`.

The builtin `check-jsonschema` tests run every upstream hook entry against these files and
expect the same exit codes as upstream `tests/acceptance/test_example_files.py`.
