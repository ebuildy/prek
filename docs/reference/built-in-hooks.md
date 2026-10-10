# Built-in Hooks

This page lists the built-in Rust hooks, their supported arguments, and behavior
notes. For setup examples and how to choose between the automatic fast path and
`repo: builtin`, see [Built-in Hooks](../built-in-hooks.md).

## Supported Hooks

For `repo: builtin`, the following hooks are supported:

- [`trailing-whitespace`](#trailing-whitespace) (Trims trailing whitespace.)
- [`check-added-large-files`](#check-added-large-files) (Prevents giant files from being committed.)
- [`check-case-conflict`](#check-case-conflict) (Checks for files that would conflict in case-insensitive filesystems.)
- [`check-illegal-windows-names`](#check-illegal-windows-names) (Checks for filenames which cannot be created on Windows.)
- [`end-of-file-fixer`](#end-of-file-fixer) (Ensures that a file is either empty, or ends with one newline.)
- [`file-contents-sorter`](#file-contents-sorter) (Sorts the lines in specified files (defaults to alphabetical).)
- [`requirements-txt-fixer`](#requirements-txt-fixer) (Sorts entries in requirements.txt.)
- [`fix-byte-order-marker`](#fix-byte-order-marker) (Removes UTF-8 byte order marker.)
- [`forbid-new-submodules`](#forbid-new-submodules) (Prevents the addition of new Git submodules.)
- [`check-json`](#check-json) (Checks JSON files for parseable syntax.)
- [`check-json5`](#check-json5) (Checks JSON5 files for parseable syntax.)
- [`check-jsonc`](#check-jsonc) (Checks JSONC files for parseable syntax.)
- [`check-jsonschema`](#check-jsonschema) (Validates JSON, YAML and TOML files against a JSON Schema.)
- [`pretty-format-json`](#pretty-format-json) (Checks that JSON files are pretty-formatted.)
- [`check-toml`](#check-toml) (Checks TOML files for parseable syntax.)
- [`check-vcs-permalinks`](#check-vcs-permalinks) (Ensures that links to VCS websites are permalinks.)
- [`check-yaml`](#check-yaml) (Checks YAML files for parseable syntax.)
- [`check-xml`](#check-xml) (Checks XML files for parseable syntax.)
- [`deny-filename-pattern`](#deny-filename-pattern) (Fails if any selected filename matches a regular expression.)
- [`deny-pattern`](#deny-pattern) (Fails if any file contains a matching regular expression.)
- [`require-filename-pattern`](#require-filename-pattern) (Fails if any selected filename does not match a regular expression.)
- [`require-pattern`](#require-pattern) (Fails if any file does not contain a matching regular expression.)
- [`mixed-line-ending`](#mixed-line-ending) (Replaces or checks mixed line endings.)
- [`check-symlinks`](#check-symlinks) (Checks for symlinks which do not point to anything.)
- [`destroyed-symlinks`](#destroyed-symlinks) (Detects symlinks that were replaced with regular files whose contents are the original symlink target path.)
- [`check-merge-conflict`](#check-merge-conflict) (Checks for files that contain merge conflict strings.)
- [`detect-private-key`](#detect-private-key) (Detects the presence of private keys.)
- [`no-commit-to-branch`](#no-commit-to-branch) (Protects specific branches from direct commits.)
- [`check-signed-commit`](#check-signed-commit) (Ensures commits are signed with a valid GPG/SSH signature before they're pushed.)
- [`check-shebang-scripts-are-executable`](#check-shebang-scripts-are-executable) (Ensures that (non-binary) files with a shebang are executable.)
- [`check-executables-have-shebangs`](#check-executables-have-shebangs) (Ensures that (non-binary) executables have a shebang.)

## Automatic Fast Path

Currently, part of the hooks from `https://github.com/pre-commit/pre-commit-hooks` and all hooks from `https://github.com/python-jsonschema/check-jsonschema` are supported. More popular repositories may be added over time.

- [`trailing-whitespace`](https://github.com/pre-commit/pre-commit-hooks#trailing-whitespace) (Trims trailing whitespace.)
- [`check-added-large-files`](https://github.com/pre-commit/pre-commit-hooks#check-added-large-files) (Prevents giant files from being committed.)
- [`check-case-conflict`](https://github.com/pre-commit/pre-commit-hooks#check-case-conflict) (Checks for files that would conflict in case-insensitive filesystems.)
- [`check-illegal-windows-names`](https://github.com/pre-commit/pre-commit-hooks#check-illegal-windows-names) (Checks for filenames which cannot be created on Windows.)
- [`end-of-file-fixer`](https://github.com/pre-commit/pre-commit-hooks#end-of-file-fixer) (Ensures that a file is either empty, or ends with one newline.)
- [`file-contents-sorter`](https://github.com/pre-commit/pre-commit-hooks#file-contents-sorter) (Sorts the lines in specified files (defaults to alphabetical).)
- [`requirements-txt-fixer`](https://github.com/pre-commit/pre-commit-hooks#requirements-txt-fixer) (Sorts entries in requirements.txt.)
- [`fix-byte-order-marker`](https://github.com/pre-commit/pre-commit-hooks#fix-byte-order-marker) (Removes UTF-8 byte order marker.)
- [`forbid-new-submodules`](https://github.com/pre-commit/pre-commit-hooks#forbid-new-submodules) (Prevents the addition of new Git submodules.)
- [`check-json`](https://github.com/pre-commit/pre-commit-hooks#check-json) (Checks JSON files for parseable syntax.)
- [`check-toml`](https://github.com/pre-commit/pre-commit-hooks#check-toml) (Checks TOML files for parseable syntax.)
- [`check-vcs-permalinks`](https://github.com/pre-commit/pre-commit-hooks#check-vcs-permalinks) (Ensures that links to VCS websites are permalinks.)
- [`check-yaml`](https://github.com/pre-commit/pre-commit-hooks#check-yaml) (Checks YAML files for parseable syntax.)
- [`check-xml`](https://github.com/pre-commit/pre-commit-hooks#check-xml) (Checks XML files for parseable syntax.)
- [`mixed-line-ending`](https://github.com/pre-commit/pre-commit-hooks#mixed-line-ending) (Replaces or checks mixed line endings.)
- [`check-symlinks`](https://github.com/pre-commit/pre-commit-hooks#check-symlinks) (Checks for symlinks which do not point to anything.)
- [`destroyed-symlinks`](https://github.com/pre-commit/pre-commit-hooks#destroyed-symlinks) (Detects symlinks that were replaced with regular files whose contents are the original symlink target path.)
- [`check-merge-conflict`](https://github.com/pre-commit/pre-commit-hooks#check-merge-conflict) (Checks for files that contain merge conflict strings.)
- [`detect-private-key`](https://github.com/pre-commit/pre-commit-hooks#detect-private-key) (Detects the presence of private keys.)
- [`no-commit-to-branch`](https://github.com/pre-commit/pre-commit-hooks#no-commit-to-branch) (Protects specific branches from direct commits.)
- [`check-shebang-scripts-are-executable`](https://github.com/pre-commit/pre-commit-hooks#check-shebang-scripts-are-executable) (Ensures that (non-binary) files with a shebang are executable.)
- [`check-executables-have-shebangs`](https://github.com/pre-commit/pre-commit-hooks#check-executables-have-shebangs) (Ensures that (non-binary) executables have a shebang.)

All 29 hooks from `https://github.com/python-jsonschema/check-jsonschema` (`check-jsonschema`,
`check-metaschema`, `check-github-workflows`, `check-renovate`, `check-gitlab-ci`, ...) run with
the [`check-jsonschema`](#check-jsonschema) implementation. Their `entry`, `files` and `types`
still come from the pinned upstream revision, and no Python environment is installed for them.

### Notes

- `pretty-format-json` is currently available only via `repo: builtin` while parity coverage against upstream Python behavior is still being expanded.
- Other hooks from the repository which have no fast path implementation will run via the standard method.

## Hook Reference

### Configuration notes

- Configure arguments via `args: [...]` just like `pre-commit`.
- For `repo: builtin`, `entry` is not allowed and `language` must be `system` (it is fine to omit `language`).
- Some hooks are **fixers** (they modify files). Like `pre-commit-hooks`, they typically exit non-zero after making changes so you can re-run the commit.

Example:

```yaml
repos:
  - repo: builtin
    hooks:
      - id: trailing-whitespace
        args: [--markdown-linebreak-ext=md]
      - id: check-added-large-files
        args: [--maxkb=1024]
```

---

### `trailing-whitespace`

Trims trailing whitespace from each line.

**Supported arguments**:

- `--check` (prek only)
    - Report files that would change and exit nonzero without modifying them.
- `--markdown-linebreak-ext=<ext>` (repeatable / comma-separated)
    - Preserves Markdown hard line breaks (two trailing spaces) for files with the given extension(s).
    - Use `--markdown-linebreak-ext=*` to treat **all** files as Markdown.
- `--chars=<chars>`
    - Trim only the specified set of characters instead of “all trailing whitespace”.
    - Example: `args: [--chars, " \t"]` (space + tab).

**Caveats**

- `--markdown-linebreak-ext` values must be extensions only (no path separators).

---

### `check-added-large-files`

Prevents giant files from being committed.

**Supported arguments** (compatible with `pre-commit-hooks`):

- `--maxkb=<N>` (default: `500`)
    - Maximum allowed file size, in kibibytes. File sizes are rounded up, so `--maxkb=0` accepts only empty files.
- `--enforce-all`
    - Check all matched files, not just those staged for addition.

**Caveats**

- By default, only files staged for **addition** are checked.
- Files configured with `filter=lfs` (via git attributes) are skipped.

---

### `check-case-conflict`

Checks for paths that would conflict on a case-insensitive filesystem (for example macOS / Windows).

**Supported arguments**

- None.

**Caveats**

- The check includes parent directories as well as file paths, to catch directory-level case conflicts.

---

### `check-illegal-windows-names`

Checks for filenames that cannot be created on Windows.

**Supported arguments**

- None.

**Behavior / caveats**

- Reports filenames containing Windows-reserved device names such as `CON`, `PRN`, `AUX`, `NUL`, `COM1`, and `LPT1`.
- Reports filenames containing characters forbidden by Windows, including `<`, `>`, `:`, `"`, `\`, `|`, `?`, `*`, and control characters.
- Reports path segments ending with a trailing `.` or space.

---

### `end-of-file-fixer`

Ensures files end in a newline and only a newline.

**Supported arguments**

- `--check` (prek only)
    - Report files that would change and exit nonzero without modifying them.

**Behavior / caveats**

- Empty files are left unchanged.
- Files containing only newlines are truncated to empty.
- If a file has no trailing newline, a single `\n` is appended (even if the file otherwise uses CRLF).
- If a file has trailing newlines, they are reduced to exactly one trailing line ending.

---

### `file-contents-sorter`

Sorts the non-empty lines in each matched file and rewrites the file when the normalized order changes.

**Supported arguments**:

- `--check` (prek only)
    - Report files that would change and exit nonzero without modifying them.
- `--ignore-case`
    - Sort using ASCII case-folded ordering.
    - Mutually exclusive with `--unique`.
- `--unique`
    - Sort and deduplicate lines.
    - Mutually exclusive with `--ignore-case`.

**Behavior / caveats**

- Blank lines and whitespace-only lines are removed before sorting.
- Line endings are normalized to `\n` in the rewritten file.
- Like upstream, the builtin hook defaults to `files: '^$'`, so you must configure `files:` explicitly to target specific files.

Example:

```yaml
repos:
  - repo: builtin
    hooks:
      - id: file-contents-sorter
        files: ^requirements(-dev)?\.txt$
```

---

### `requirements-txt-fixer`

Sorts entries in Python `requirements*.txt` and `constraints*.txt` files by their case-insensitive requirement name.

**Supported arguments**

- `--check` (prek only)
    - Report files that would change and exit nonzero without modifying them.

**Behavior / caveats**

- The default file pattern is `(requirements|constraints).*\.txt$`.
- Leading comments and continuation lines stay attached to their requirement while sorting. Top-of-file and trailing comment blocks are preserved.
- Exact duplicate entries are collapsed, preferring the copy with an attached comment.
- Exact `pkg-resources==0.0.0` and `pkg_resources==0.0.0` entries are removed, matching upstream.
- This is a sorter, not a full PEP 508 validator. It uses the same lightweight name extraction as `pre-commit-hooks`.

---

### `fix-byte-order-marker`

Removes a UTF-8 byte order marker (BOM) from the beginning of a file.

**Supported arguments**

- `--check` (prek only)
    - Report files that would change and exit nonzero without modifying them.

**Caveats**

- Only removes the UTF-8 BOM (`EF BB BF`).

---

### `forbid-new-submodules`

Prevents the addition of new Git submodules.

**Supported arguments**

- None.

**Behavior / caveats**

- Existing submodules are allowed; only submodules newly added by the checked changes are reported.
- Staged changes are checked by default. When `PRE_COMMIT_FROM_REF` and `PRE_COMMIT_TO_REF` are both set, their revision range is checked instead.

---

### `check-json`

Attempts to load all JSON files to verify syntax. Empty files are rejected.

**Supported arguments**

- None.

**Caveats / differences**

- This implementation rejects **duplicate object keys** (errors with `duplicate key ...`).
- The parser disables the default recursion limit and uses a stack-friendly drop strategy for deeply nested JSON.

---

### `check-json5`

Attempts to load all JSON5 files to verify syntax.

**Supported arguments**

- None.

**Caveats / differences**

- This implementation rejects **duplicate object keys** (errors with `duplicate key ...`).

---

### `check-jsonc`

Checks `.jsonc` files for parseable syntax.

To check JSONC stored in `.json` files, such as `tsconfig.json`, override the default
file-type filter:

```yaml
repos:
  - repo: builtin
    hooks:
      - id: check-jsonc
        types: [json]
        files: '(^|/)tsconfig\.json$'
        args: [--allow-trailing-commas]
```

**Supported arguments**

- `--allow-trailing-commas`
    - Allow trailing commas in objects and arrays (rejected by default).

**Caveats / differences**

- This implementation rejects **duplicate object keys** (errors with `duplicate key ...`).

---

### `check-jsonschema`

A Rust port of [check-jsonschema](https://github.com/python-jsonschema/check-jsonschema).
It validates JSON, YAML, TOML and JSON5 files against a JSON Schema and accepts the upstream
command-line options, so upstream hooks run unchanged through the [fast path](#automatic-fast-path).
The schema is compiled once and every file is checked against it.

The hook does not select any files by default. Set `files` (or `types`) to choose what to validate.

```yaml
repos:
  - repo: builtin
    hooks:
      - id: check-jsonschema
        files: '^config/.*\.ya?ml$'
        args: [--schemafile, schemas/config.schema.json]
      - id: check-jsonschema
        name: Validate GitHub workflows
        files: '^\.github/workflows/[^/]+$'
        args: [--builtin-schema, vendor.github-workflows]
```

**Behavior shared with upstream**

- File types come from the extension, case-sensitively: `.json`, `.jsonld`, `.geojson` (JSON),
  `.yaml`, `.yml`, `.ymlld`, `.eyaml`, `.cff` (YAML), `.json5` and `.toml`. Other files use
  `--default-filetype`.
- YAML is read as YAML 1.2 (`yes` and `on` are strings), unknown tags are errors, and
  timestamps stay strings. TOML datetimes become strings, with `Z` added when there is no offset.
- Only the formats upstream checks with its default dependencies are enforced: `date`,
  `date-time`, `time`, `email` and `idn-email` (an `@` check), `idn-hostname`, `ipv4`, `ipv6`,
  `regex` and `uuid`, in every draft. Other formats such as `uri` or `hostname` are not checked.
- `$ref` to local files and HTTP(S) URLs is resolved, in JSON, YAML, TOML or JSON5.
- Downloads are retried twice and cached in prek's cache directory, keyed by URL. A cached file
  is reused unless the server's `Last-Modified` is newer.
- A schema that cannot be loaded fails this hook only.

**Additions over upstream** (from upstream feature requests)

- Every document of a multi-document YAML file is validated, and errors name the document,
  such as `ci.yml (document 2)` (upstream #222, #561).
- Errors in JSON and YAML files include the line of the failing value, such as
  `ci.yml:12: /jobs/build: ...` (upstream #359). `-o json` adds a `line` field.
- `--schema-from-instances` validates each document against the schema it names, with a
  top-level `$schema` key or a `# yaml-language-server: $schema=...` comment. Relative locations
  resolve against the file's directory. `--schemafile` or `--builtin-schema`, if also given,
  applies to documents that name no schema (upstream #310, #340, #644).
- Without `Last-Modified`, the cache is validated with the server's `ETag` (upstream #668).

**Supported arguments**

- `--schemafile <PATH|URI>`: local path (relative to the project root, `~` and `file://` work)
  or HTTP(S) URL of the schema.
- `--builtin-schema <NAME>`: a schema bundled with upstream, such as `vendor.github-workflows`
  or `custom.github-workflows-require-timeout`.
- `--check-metaschema`: validate each file as a schema against the metaschema of its `$schema`.
- `--base-uri <URI>`: override the schema `$id`.
- `--no-cache`: always download remote schemas.
- `--disable-formats <FORMAT,...>`: format checks to turn off, comma separated and repeatable.
  `*` turns off all of them.
- `--regex-variant {default,nonunicode,python}` (and the legacy `--format-regex`).
- `--default-filetype {json,yaml,toml,json5}` (default `json`) and `--force-filetype`.
- `--data-transform {azure-pipelines,gitlab-ci}`.
- `--fill-defaults`: fill `default` values of `properties` before validating.
- `--schema-from-instances`: see above.
- `-o/--output-format {text,json}`, `-v/--verbose`, `-q/--quiet`.
- `--traceback-mode`, `--cache-filename` and `--color` are accepted and ignored.

**Caveats / differences**

- Errors are printed one per line as `path:line: pointer: message`, and error messages come from
  the Rust `jsonschema` crate, so their wording differs from upstream.
- `--validator-class` and reading from stdin (`-`) are not supported.
- `--regex-variant nonunicode` behaves like `default`, and `python` uses Rust regex syntax that
  rejects JavaScript-only named groups.
- `--check-metaschema` does not check Draft 3 documents.
- YAML numbers with a leading zero such as `017` are read as floats. `.inf` and `.nan` are
  validated as the largest finite number and `0`, which only matters for range keywords.
  JSON files with `NaN` or `Infinity` are rejected.
- `--fill-defaults` follows `properties`, `items` and `allOf`/`anyOf`/`oneOf`, not `$ref`.

---

### `pretty-format-json`

Checks that JSON files are pretty-formatted and can optionally rewrite them in place.

**Supported arguments** (compatible with `pre-commit-hooks`):

- `--autofix`
    - Rewrite files in place when formatting changes are needed.
- `--indent=<indent>` (default: `2`)
    - Use `<indent>` for each indentation level.
    - Numeric values mean that many spaces.
    - Non-numeric values are used literally, so `--indent=\t` uses tabs.
- `--no-ensure-ascii`
    - Keep non-ASCII characters as UTF-8 instead of escaping them as `\uXXXX`.
- `--no-sort-keys`
    - Preserve the original key order instead of sorting object keys.
- `--top-keys=<k1,k2,...>`
    - In every JSON object, move matching keys to the front in the given order.
    - Duplicate names after the first one are ignored.
    - Remaining keys come after that prefix and are sorted unless `--no-sort-keys` is set.
    - This applies recursively to nested objects too, not just the root object.

**Caveats**

- This hook is currently available only via `repo: builtin`; automatic fast-path replacement of the upstream Python hook remains disabled until parity coverage is broader.
- Rewritten files always use LF (`\n`) line endings and end with exactly one trailing newline.

---

### `check-toml`

Attempts to load all TOML files to verify syntax.

**Supported arguments**

- None.

**Caveats**

- Files must be valid UTF-8; invalid UTF-8 is reported as an error.
- May report multiple parse errors for a single file.

---

### `check-vcs-permalinks`

Ensures that links to VCS websites are permalinks.

**Supported arguments** (compatible with `pre-commit-hooks`):

- `--additional-github-domain=<domain>` (repeatable)
    - Adds extra GitHub-style domains to check in addition to the default `github.com`.

**Behavior / caveats**

- Flags links of the form `https://<domain>/<owner>/<repo>/blob/<branch>/...#L...`.
- Does not flag commit-hash permalinks where `<branch>` is already a 4-64 character hexadecimal revision.
- The builtin and fast-path implementations currently follow the upstream hook's GitHub-family matching behavior.

---

### `check-yaml`

Attempts to load all YAML files to verify syntax.

**Supported arguments** (partially compatible with `pre-commit-hooks`):

- `-m`, `--allow-multiple-documents` (alias: `--multi`)
    - Allow YAML multi-document syntax (`---`).
- `--disallow-unknown-tags`
    - Reject unrecognized YAML tags.
- `--unsafe`
    - Parse YAML syntax without loading it. Implies `--allow-multiple-documents`.

**Behavior / caveats**

- Unrecognized YAML tags are allowed by default. This differs from the pinned `pre-commit-hooks` implementation, which rejects them while loading. Use `--disallow-unknown-tags` to match that behavior.

---

### `check-xml`

Attempts to load all XML files to verify syntax.

**Supported arguments**

- None.

**Caveats**

- Empty files are treated as invalid XML.
- Fails if there is “junk after the document element” (multiple top-level roots).

---

### `deny-filename-pattern`

Fails when the final path component (the basename) of any selected file matches a configured regular expression. When multiple patterns are provided, the hook fails when a basename matches any one of them.

The standard `files`, `exclude`, and type filters select which project-relative paths are checked. The patterns passed to this hook are then matched only against each selected basename.

**Supported arguments**

- `PATTERN...` (required)
    - Positional regular expressions to deny.
    - Use `--` before a pattern that begins with `-`.
- `-i`, `--ignore-case`
    - Match all patterns case-insensitively.

Each matching file is reported once as `path: filename matches a denied pattern`.

```yaml
repos:
  - repo: builtin
    hooks:
      - id: deny-filename-pattern
        name: disallow spaces in filenames
        args: ['\s']
```

---

### `deny-pattern`

Fails when any selected text file matches a configured regular expression.
When multiple patterns are provided, matching any one of them is sufficient.

**Supported arguments**

- `PATTERN...` (required)
    - Positional regular expressions to deny.
    - Use `--` before a pattern that begins with `-`.
- `-i`, `--ignore-case`
    - Match all patterns case-insensitively.
- `-m`, `--multiline`
    - Search each file as a whole, with `^` and `$` matching line boundaries and `.` matching newlines.
    - Reads each selected file into memory.

By default, each matching line is reported as `path:line:contents`. A line matching more than one pattern is reported only once. With `--multiline`, the earliest match in each file is reported as `path:start-line:matched-block`.

```yaml
repos:
  - repo: builtin
    hooks:
      - id: deny-pattern
        name: disallow wildcard imports
        args: ['^\s*#import\s+.+:\s*\*']
        files: \.typ$
```

---

### `require-filename-pattern`

Fails when the final path component (the basename) of any selected file does not match at least one configured regular expression. This is a per-file requirement: every selected basename must match, while different basenames may match different patterns.

`require-filename-pattern` supports the same positional `PATTERN...` and `-i` / `--ignore-case` arguments as [`deny-filename-pattern`](#deny-filename-pattern). Matching uses search semantics; use `^` and `$` when the pattern must match the entire basename. Files without a match are reported as `path: filename does not match any required pattern`.

```yaml
repos:
  - repo: builtin
    hooks:
      - id: require-filename-pattern
        name: python tests naming
        args:
          - '^test_.*\.py$'
          - '^__init__\.py$'
          - '^conftest\.py$'
        files: '(^|/)tests/.+\.py$'
```

---

### `require-pattern`

Fails when any selected text file does not match at least one configured regular expression. This is a per-file requirement: every file must match, while different files may match different patterns.

`require-pattern` supports the same positional `PATTERN...`, `-i` / `--ignore-case`, and `--multiline` arguments as [`deny-pattern`](#deny-pattern). Files without a match are reported as `path: file does not match any required pattern`.

```yaml
repos:
  - repo: builtin
    hooks:
      - id: require-pattern
        name: require a copyright notice
        args: [--ignore-case, copyright]
        files: '\.(rs|py)$'
```

---

### `mixed-line-ending`

Replaces or checks mixed line endings.

**Supported arguments** (compatible with `pre-commit-hooks`, plus one extra mode):

- `--fix=<mode>` (default: `auto`)
    - `auto`: replace with the most frequent line ending in the file.
    - `no`: check only (do not modify files).
    - `lf`: convert to LF (`\n`).
    - `crlf`: convert to CRLF (`\r\n`).
    - `cr`: convert to CR (`\r`) (extra mode in `prek`).

**Caveats**

- Empty and binary files (containing NUL) are skipped.
- Upstream note: forcing `lf` / `crlf` may not behave as expected with git CRLF conversion settings (for example `core.autocrlf`).

---

### `check-symlinks`

Checks for symlinks which do not point to anything.

**Supported arguments**

- None.

**Caveats**

- Relies on filesystem symlink support. On Windows, symlink creation and detection can be permission-dependent.

---

### `destroyed-symlinks`

Detects files staged as regular files whose `HEAD` version is a symlink, which usually happens when a repository is checked out in an environment without symlink support.

**Supported arguments**

- None.

**Caveats**

- This matches upstream `pre-commit-hooks` behavior: it only checks tracked entries reported by `git status --porcelain=v2`.
- It intentionally ignores differences consisting only of trailing ASCII whitespace (including spaces, tabs, and newline/CRLF conversions) when comparing the staged file against the original symlink target path, because those differences are commonly introduced by formatting hooks.

---

### `check-merge-conflict`

Checks for merge conflict markers.

**Supported arguments** (compatible with `pre-commit-hooks`):

- `--assume-in-merge`
    - Allow running the hook even when there is no merge/rebase state detected.

**Caveats**

- By default, this hook exits successfully when not in a merge/rebase state.
- Detects conflict markers only when they appear at the start of a line.
- Detects standard conflict blocks (`<<<<<<<`, `=======`, `>>>>>>>`) and diff3 ancestor markers (`|||||||`).
- `=======` is only reported after a preceding `<<<<<<<`, which avoids false positives for content such as reStructuredText headings.

---

### `detect-private-key`

Detects the presence of private keys.

**Supported arguments**

- None.

**Caveats**

- This is a heuristic substring scan for common PEM/key headers (e.g. `BEGIN RSA PRIVATE KEY`, `BEGIN OPENSSH PRIVATE KEY`, `BEGIN PGP PRIVATE KEY BLOCK`, etc.).
  It can produce false positives/negatives.

---

### `no-commit-to-branch`

Protects specific branches from direct commits.

**Supported arguments** (compatible with `pre-commit-hooks`):

- `-b`, `--branch <branch>` (repeatable, default: `main`, `master`)
- `-p`, `--pattern <regex>` (repeatable)

**Caveats**

- This hook is configured as `always_run: true` by default, and does not take filenames.
  As a result, `files`, `exclude`, `types`, etc. are ignored unless you explicitly set `always_run: false`.
- If HEAD is detached (no current branch), the hook does nothing.

---

### `check-signed-commit`

Ensures commits are signed with a valid GPG/SSH signature before they're pushed.

**Supported arguments**

- None.

**Behavior / caveats**

- Defaults to the `pre-push` and `manual` stages. This hook is configured as
  `always_run: true` and does not take filenames.
- Checks the commits being pushed. Root/orphan pushes check the entire branch history.
- Manual runs check only `HEAD`.
- Merge commits are skipped.
- Commits must pass `git verify-commit` using your local Git GPG/SSH configuration.
  Unsigned commits and verification failures, including missing tools or public keys,
  fail the hook.

---

### `check-executables-have-shebangs`

Checks that non-binary executables have a proper shebang.

**Supported arguments**

- None.

**Caveats**

- The check is intentionally lightweight: it only verifies that the file starts with `#!`.
- On systems where the executable bit is not tracked by the filesystem, `prek` consults git’s staged mode bits.

---

### `check-shebang-scripts-are-executable`

Checks that non-binary files with a shebang are marked executable.

**Supported arguments**

- None.

**Caveats**

- The check is intentionally lightweight: it only verifies that the file starts with `#!`.
- To work on filesystems which do not track the executable bit, `prek` consults git’s staged mode bits.
