//! Schemas bundled with upstream check-jsonschema, for `--builtin-schema`.
//!
//! `schemas/vendor` and `schemas/custom` are copies of upstream
//! `src/check_jsonschema/builtin_schemas/{vendor,custom}` (licenses in
//! `schemas/vendor/licenses`). To refresh them, copy those directories from the upstream
//! release that prek should match and update the list below.

/// Schemas that upstream vendors from their publishers, by catalog name.
const VENDOR: &[(&str, &str)] = &[
    (
        "azure-pipelines",
        include_str!("schemas/vendor/azure-pipelines.json"),
    ),
    (
        "bamboo-spec",
        include_str!("schemas/vendor/bamboo-spec.json"),
    ),
    (
        "bitbucket-pipelines",
        include_str!("schemas/vendor/bitbucket-pipelines.json"),
    ),
    ("buildkite", include_str!("schemas/vendor/buildkite.json")),
    ("changie", include_str!("schemas/vendor/changie.json")),
    ("circle-ci", include_str!("schemas/vendor/circle-ci.json")),
    (
        "citation-file-format",
        include_str!("schemas/vendor/citation-file-format.json"),
    ),
    ("cloudbuild", include_str!("schemas/vendor/cloudbuild.json")),
    ("codecov", include_str!("schemas/vendor/codecov.json")),
    (
        "compose-spec",
        include_str!("schemas/vendor/compose-spec.json"),
    ),
    ("dependabot", include_str!("schemas/vendor/dependabot.json")),
    ("drone-ci", include_str!("schemas/vendor/drone-ci.json")),
    (
        "github-actions",
        include_str!("schemas/vendor/github-actions.json"),
    ),
    (
        "github-discussion",
        include_str!("schemas/vendor/github-discussion.json"),
    ),
    (
        "github-issue-config",
        include_str!("schemas/vendor/github-issue-config.json"),
    ),
    (
        "github-issue-forms",
        include_str!("schemas/vendor/github-issue-forms.json"),
    ),
    (
        "github-workflows",
        include_str!("schemas/vendor/github-workflows.json"),
    ),
    ("gitlab-ci", include_str!("schemas/vendor/gitlab-ci.json")),
    ("meltano", include_str!("schemas/vendor/meltano.json")),
    ("mergify", include_str!("schemas/vendor/mergify.json")),
    (
        "readthedocs",
        include_str!("schemas/vendor/readthedocs.json"),
    ),
    ("renovate", include_str!("schemas/vendor/renovate.json")),
    ("snapcraft", include_str!("schemas/vendor/snapcraft.json")),
    ("taskfile", include_str!("schemas/vendor/taskfile.json")),
    ("travis", include_str!("schemas/vendor/travis.json")),
    (
        "woodpecker-ci",
        include_str!("schemas/vendor/woodpecker-ci.json"),
    ),
];

/// Schemas written by upstream, by name.
const CUSTOM: &[(&str, &str)] = &[(
    "github-workflows-require-timeout",
    include_str!("schemas/custom/github-workflows-require-timeout.json"),
)];

/// Looks up a `--builtin-schema` name the way upstream does: `vendor.NAME` and
/// `custom.NAME` are explicit, a bare `NAME` tries custom schemas first. Names are
/// case-insensitive.
pub(super) fn builtin_schema(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    let find = |table: &[(&str, &'static str)], key: &str| {
        table
            .iter()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, schema)| *schema)
    };
    if let Some(key) = name.strip_prefix("vendor.") {
        return find(VENDOR, key);
    }
    if let Some(key) = name.strip_prefix("custom.") {
        return find(CUSTOM, key);
    }
    find(CUSTOM, &name).or_else(|| find(VENDOR, &name))
}

/// Every accepted `--builtin-schema` value, for error messages.
pub(super) fn builtin_schema_names() -> Vec<String> {
    VENDOR
        .iter()
        .map(|(name, _)| format!("vendor.{name}"))
        .chain(CUSTOM.iter().map(|(name, _)| format!("custom.{name}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_schema_is_valid_json() {
        for (name, schema) in VENDOR.iter().chain(CUSTOM) {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(schema);
            assert!(parsed.is_ok(), "{name}");
        }
    }

    #[test]
    fn lookup_by_prefix_or_bare_name() {
        assert!(builtin_schema("vendor.github-workflows").is_some());
        assert!(builtin_schema("github-workflows").is_some());
        assert!(builtin_schema("VENDOR.Renovate").is_some());
        assert!(builtin_schema("custom.github-workflows-require-timeout").is_some());
        assert!(builtin_schema("github-workflows-require-timeout").is_some());
        assert!(builtin_schema("vendor.github-workflows-require-timeout").is_none());
        assert!(builtin_schema("nope").is_none());
        assert_eq!(builtin_schema_names().len(), VENDOR.len() + CUSTOM.len());
    }
}
