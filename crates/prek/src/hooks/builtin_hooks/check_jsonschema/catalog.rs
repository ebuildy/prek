//! Schemas for `--builtin-schema`, by name.
//!
//! The `vendor.*` schemas are the ones upstream check-jsonschema vendors from their
//! publishers. They are not copied into prek: each name maps to the publisher's URL, and
//! the schema is downloaded and cached like any other `--schemafile` URL. The
//! `custom.*` schemas are written by upstream, have no publisher, and are small, so they
//! are embedded.
//!
//! The URLs are those of upstream `src/check_jsonschema/catalog.py`.

/// Where a builtin schema comes from.
#[derive(Clone, Copy)]
pub(super) enum BuiltinSchema {
    /// Downloaded from the publisher, through the schema cache.
    Remote(&'static str),
    /// Embedded in the binary.
    Embedded(&'static str),
}

/// Schemas that upstream vendors from their publishers, by catalog name.
const VENDOR: &[(&str, &str)] = &[
    (
        "azure-pipelines",
        "https://raw.githubusercontent.com/microsoft/azure-pipelines-vscode/main/service-schema.json",
    ),
    (
        "bamboo-spec",
        "https://json.schemastore.org/bamboo-spec.json",
    ),
    (
        "bitbucket-pipelines",
        "https://api.bitbucket.org/schemas/pipelines-configuration",
    ),
    (
        "buildkite",
        "https://raw.githubusercontent.com/buildkite/pipeline-schema/main/schema.json",
    ),
    ("changie", "https://changie.dev/schema.json"),
    (
        "circle-ci",
        "https://raw.githubusercontent.com/CircleCI-Public/circleci-yaml-language-server/main/schema.json",
    ),
    (
        "citation-file-format",
        "https://raw.githubusercontent.com/citation-file-format/citation-file-format/main/schema.json",
    ),
    ("cloudbuild", "https://json.schemastore.org/cloudbuild.json"),
    ("codecov", "https://www.schemastore.org/codecov.json"),
    (
        "compose-spec",
        "https://raw.githubusercontent.com/compose-spec/compose-spec/master/schema/compose-spec.json",
    ),
    (
        "dependabot",
        "https://json.schemastore.org/dependabot-2.0.json",
    ),
    ("drone-ci", "https://json.schemastore.org/drone.json"),
    (
        "github-actions",
        "https://json.schemastore.org/github-action",
    ),
    (
        "github-discussion",
        "https://www.schemastore.org/github-discussion.json",
    ),
    (
        "github-issue-config",
        "https://www.schemastore.org/github-issue-config.json",
    ),
    (
        "github-issue-forms",
        "https://www.schemastore.org/github-issue-forms.json",
    ),
    (
        "github-workflows",
        "https://json.schemastore.org/github-workflow",
    ),
    (
        "gitlab-ci",
        "https://gitlab.com/api/v4/projects/gitlab-org%2Fgitlab/repository/files/app%2Fassets%2Fjavascripts%2Feditor%2Fschema%2Fci.json/raw?ref=master",
    ),
    (
        "meltano",
        "https://raw.githubusercontent.com/meltano/meltano/main/src/meltano/schemas/meltano.schema.json",
    ),
    (
        "mergify",
        "https://docs.mergify.com/mergify-configuration-schema.json",
    ),
    (
        "readthedocs",
        "https://raw.githubusercontent.com/readthedocs/readthedocs.org/main/readthedocs/rtd_tests/fixtures/spec/v2/schema.json",
    ),
    (
        "renovate",
        "https://docs.renovatebot.com/renovate-schema.json",
    ),
    (
        "snapcraft",
        "https://raw.githubusercontent.com/canonical/snapcraft/main/schema/snapcraft.json",
    ),
    ("taskfile", "https://taskfile.dev/schema.json"),
    ("travis", "https://json.schemastore.org/travis"),
    (
        "woodpecker-ci",
        "https://raw.githubusercontent.com/woodpecker-ci/woodpecker/main/pipeline/frontend/yaml/linter/schema/schema.json",
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
pub(super) fn builtin_schema(name: &str) -> Option<BuiltinSchema> {
    let name = name.to_ascii_lowercase();
    let find = |table: &[(&str, &'static str)], key: &str| {
        table
            .iter()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, value)| *value)
    };
    let vendor = |key: &str| find(VENDOR, key).map(BuiltinSchema::Remote);
    let custom = |key: &str| find(CUSTOM, key).map(BuiltinSchema::Embedded);
    if let Some(key) = name.strip_prefix("vendor.") {
        return vendor(key);
    }
    if let Some(key) = name.strip_prefix("custom.") {
        return custom(key);
    }
    custom(&name).or_else(|| vendor(&name))
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
    fn embedded_schemas_are_valid_json() {
        for (name, schema) in CUSTOM {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(schema);
            assert!(parsed.is_ok(), "{name}");
        }
    }

    #[test]
    fn remote_schemas_have_https_urls() {
        for (name, url) in VENDOR {
            assert!(url.starts_with("https://"), "{name}: {url}");
        }
    }

    #[test]
    fn lookup_by_prefix_or_bare_name() {
        let is_remote = |name| matches!(builtin_schema(name), Some(BuiltinSchema::Remote(_)));
        let is_embedded = |name| matches!(builtin_schema(name), Some(BuiltinSchema::Embedded(_)));
        assert!(is_remote("vendor.github-workflows"));
        assert!(is_remote("github-workflows"));
        assert!(is_remote("VENDOR.Renovate"));
        assert!(is_embedded("custom.github-workflows-require-timeout"));
        assert!(is_embedded("github-workflows-require-timeout"));
        assert!(builtin_schema("vendor.github-workflows-require-timeout").is_none());
        assert!(builtin_schema("nope").is_none());
        assert_eq!(builtin_schema_names().len(), VENDOR.len() + CUSTOM.len());
    }
}
