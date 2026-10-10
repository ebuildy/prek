//! Ports of upstream check-jsonschema acceptance tests (`tests/acceptance/*.py`), plus the
//! download cache unit tests (`tests/unit/test_cachedownloader.py`). Each section names the
//! upstream file it mirrors. Remote schemas are served by a small local HTTP server instead
//! of the `responses` mock used upstream.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use clap::Parser;
use serde_json::{Value, json};

use super::download::cache_filename;
use super::{Args, Context, check};
use crate::hooks::HookOutput;

// Test harness

struct Run {
    code: i32,
    output: String,
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn cache_dir(&self) -> PathBuf {
        self.dir.path().join(".cache")
    }

    fn context(&self) -> Context {
        Context {
            base: self.dir.path().to_path_buf(),
            cache_dir: self.cache_dir(),
            client: Some(reqwest::Client::new()),
        }
    }

    fn write(&self, name: &str, content: &str) -> String {
        let path = self.dir.path().join(name);
        fs_err::create_dir_all(path.parent().unwrap()).unwrap();
        fs_err::write(&path, content).unwrap();
        path.display().to_string()
    }

    fn write_json(&self, name: &str, value: &Value) -> String {
        self.write(name, &value.to_string())
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        self.cache_dir().join(cache_filename(url))
    }

    fn inject_cache(&self, url: &str, content: &str) {
        fs_err::create_dir_all(self.cache_dir()).unwrap();
        fs_err::write(self.cache_path(url), content).unwrap();
    }

    async fn run(&self, args: &[&str]) -> Run {
        let args = parse_args(args);
        let HookOutput {
            exit_status,
            output,
            ..
        } = check(args, self.context(), &[]).await.unwrap();
        Run {
            code: exit_status,
            output: String::from_utf8(output).unwrap(),
        }
    }

    async fn code(&self, args: &[&str]) -> i32 {
        self.run(args).await.code
    }
}

fn parse_args(args: &[&str]) -> Args {
    Args::try_parse_from(std::iter::once("check-jsonschema").chain(args.iter().copied())).unwrap()
}

fn title(passes: bool) -> Value {
    if passes {
        json!({"title": "doc one"})
    } else {
        json!({"title": 2})
    }
}

#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: String,
    headers: Vec<(String, String)>,
}

type Routes = Arc<Mutex<HashMap<String, VecDeque<MockResponse>>>>;

/// Serves queued responses per path. Like the `responses` library, responses registered for
/// one path are returned in order and the last one repeats.
struct Server {
    base: String,
    routes: Routes,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Routes = Arc::default();
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        let (thread_routes, thread_calls) = (routes.clone(), calls.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                thread_calls.lock().unwrap().push(path.clone());
                let response = {
                    let mut routes = thread_routes.lock().unwrap();
                    match routes.get_mut(&path) {
                        Some(queue) if queue.len() > 1 => queue.pop_front(),
                        Some(queue) => queue.front().cloned(),
                        None => None,
                    }
                };
                let response = response.unwrap_or(MockResponse {
                    status: 404,
                    body: String::new(),
                    headers: vec![],
                });
                let mut head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response.status,
                    response.body.len()
                );
                for (name, value) in &response.headers {
                    let _ = write!(head, "{name}: {value}\r\n");
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(response.body.as_bytes());
            }
        });
        Self {
            base,
            routes,
            calls,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn add_full(&self, path: &str, status: u16, body: &str, headers: &[(&str, &str)]) {
        self.routes
            .lock()
            .unwrap()
            .entry(path.to_string())
            .or_default()
            .push_back(MockResponse {
                status,
                body: body.to_string(),
                headers: headers
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                    .collect(),
            });
    }

    fn add(&self, path: &str, body: &Value) {
        self.add_full(path, 200, &body.to_string(), &[]);
    }

    fn calls(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| *call == path)
            .count()
    }

    fn total_calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

// Cases shared by test_local_relative_ref.py and test_remote_ref_resolution.py

struct RefCase {
    main: Value,
    others: Vec<(&'static str, Value)>,
    passing: Value,
    failing: Value,
}

fn ref_cases() -> Vec<RefCase> {
    vec![
        RefCase {
            main: json!({
                "$schema": "http://json-schema.org/draft-07/schema",
                "properties": {"title": {"$ref": "./title_schema.json"}},
                "additionalProperties": false
            }),
            others: vec![("title_schema.json", json!({"type": "string"}))],
            passing: title(true),
            failing: title(false),
        },
        RefCase {
            main: json!({
                "$schema": "http://json-schema.org/draft-07/schema",
                "type": "object",
                "required": ["test"],
                "properties": {"test": {"$ref": "./values.json#/$defs/test"}}
            }),
            others: vec![(
                "values.json",
                json!({
                    "$schema": "http://json-schema.org/draft-07/schema",
                    "$defs": {"test": {"type": "string"}}
                }),
            )],
            passing: json!({"test": "some data"}),
            failing: json!({"test": {"foo": "bar"}}),
        },
    ]
}

impl RefCase {
    fn document(&self, passes: bool) -> &Value {
        if passes { &self.passing } else { &self.failing }
    }
}

// test_local_relative_ref.py

#[tokio::test]
async fn local_ref_schema() {
    for case in ref_cases() {
        for with_file_scheme in [true, false] {
            for passes in [true, false] {
                let env = Env::new();
                let main = env.write_json("main_schema.json", &case.main);
                for (name, schema) in &case.others {
                    env.write_json(name, schema);
                }
                let doc = env.write_json("doc.json", case.document(passes));
                let schemafile = if with_file_scheme {
                    url::Url::from_file_path(fs_err::canonicalize(&main).unwrap())
                        .unwrap()
                        .to_string()
                } else {
                    main
                };
                let run = env.run(&["--schemafile", &schemafile, &doc]).await;
                assert_eq!(run.code, i32::from(!passes), "{}", run.output);
            }
        }
    }
}

#[tokio::test]
async fn local_ref_schema_failure_message() {
    let case = ref_cases().remove(1);
    let env = Env::new();
    let main = env.write_json("main_schema.json", &case.main);
    env.write_json("values.json", &case.others[0].1);
    let doc = env.write_json("doc.json", &case.failing);
    let run = env.run(&["--schemafile", &main, &doc]).await;
    assert_eq!(run.code, 1);
    assert!(
        run.output
            .contains(r#"{"foo":"bar"} is not of type "string""#),
        "{}",
        run.output
    );
}

// test_remote_ref_resolution.py

fn serve_case(server: &Server, case: &RefCase) -> String {
    server.add("/main.json", &case.main);
    for (name, schema) in &case.others {
        server.add(&format!("/{name}"), schema);
    }
    server.url("/main.json")
}

#[tokio::test]
async fn remote_ref_resolution_simple_case() {
    for case in ref_cases() {
        for passes in [true, false] {
            let server = Server::start();
            let env = Env::new();
            let main = serve_case(&server, &case);
            let doc = env.write_json("instance.json", case.document(passes));
            let run = env.run(&["--schemafile", &main, &doc]).await;
            assert_eq!(run.code, i32::from(!passes), "{}", run.output);
        }
    }
}

#[tokio::test]
async fn remote_ref_resolution_cache_control() {
    for case in ref_cases() {
        for disable_cache in [true, false] {
            let server = Server::start();
            let env = Env::new();
            let main = serve_case(&server, &case);
            let doc = env.write_json("instance.json", &case.passing);
            let mut args = vec!["--schemafile", main.as_str(), doc.as_str()];
            if disable_cache {
                args.push("--no-cache");
            }
            assert_eq!(env.code(&args).await, 0);
            for (name, _) in &case.others {
                let cached = env.cache_path(&server.url(&format!("/{name}")));
                assert_eq!(cached.exists(), !disable_cache, "{name}");
            }
        }
    }
}

#[tokio::test]
async fn remote_ref_resolution_loads_from_cache() {
    for case in ref_cases() {
        for passes in [true, false] {
            let server = Server::start();
            let env = Env::new();
            server.add("/main.json", &case.main);
            for (name, schema) in &case.others {
                // The server returns bad data and the cache holds the good schema.
                server.add(&format!("/{name}"), &json!("{}"));
                env.inject_cache(&server.url(&format!("/{name}")), &schema.to_string());
            }
            let doc = env.write_json("instance.json", case.document(passes));
            let run = env
                .run(&["--schemafile", &server.url("/main.json"), &doc])
                .await;
            assert_eq!(run.code, i32::from(!passes), "{}", run.output);
        }
    }
}

#[tokio::test]
async fn ref_resolution_prefers_id_over_retrieval_uri() {
    for passes in [true, false] {
        let server = Server::start();
        let env = Env::new();
        server.add(
            "/alternate-path-retrieval-only/schemas/main",
            &json!({
                "$id": server.url("/schemas/main.json"),
                "$schema": "http://json-schema.org/draft-07/schema",
                "properties": {"title": {"$ref": "./title_schema.json"}},
                "additionalProperties": false
            }),
        );
        server.add("/schemas/title_schema.json", &json!({"type": "string"}));
        let doc = env.write_json("instance.json", &title(passes));
        let schemafile = server.url("/alternate-path-retrieval-only/schemas/main");
        let run = env.run(&["--schemafile", &schemafile, &doc]).await;
        assert_eq!(run.code, i32::from(!passes), "{}", run.output);
    }
}

#[tokio::test]
async fn ref_resolution_does_not_callout_for_absolute_ref_to_retrieval_uri() {
    for passes in [true, false] {
        let server = Server::start();
        let env = Env::new();
        let retrieval_uri = server.url("/schemas/main");
        server.add(
            "/schemas/main",
            &json!({
                "$id": server.url("/schemas/some-uri-which-will-never-be-used/main.json"),
                "$schema": "http://json-schema.org/draft-07/schema",
                "$defs": {"title": {"type": "string"}},
                "properties": {"title": {"$ref": format!("{retrieval_uri}#/$defs/title")}},
                "additionalProperties": false
            }),
        );
        // Exactly one GET to the retrieval URI works.
        server.add_full(
            "/schemas/main",
            500,
            r#"{"error": "permafrost melted"}"#,
            &[],
        );
        let doc = env.write_json("instance.json", &title(passes));
        let run = env.run(&["--schemafile", &retrieval_uri, &doc]).await;
        assert_eq!(run.code, i32::from(!passes), "{}", run.output);
        assert_eq!(server.calls("/schemas/main"), 1);
    }
}

#[tokio::test]
async fn ref_resolution_with_custom_base_uri() {
    for passes in [true, false] {
        let server = Server::start();
        let env = Env::new();
        let retrieval_uri = server.url("/retrieval-and-in-schema-only/schemas/main");
        server.add(
            "/retrieval-and-in-schema-only/schemas/main",
            &json!({
                "$id": retrieval_uri,
                "$schema": "http://json-schema.org/draft-07/schema",
                "properties": {"title": {"$ref": "./title_schema.json"}},
                "additionalProperties": false
            }),
        );
        server.add("/schemas/title_schema.json", &json!({"type": "string"}));
        let doc = env.write_json("instance.json", &title(passes));
        let base_uri = server.url("/schemas/main");
        let run = env
            .run(&[
                "--schemafile",
                &retrieval_uri,
                "--base-uri",
                &base_uri,
                &doc,
            ])
            .await;
        assert_eq!(run.code, i32::from(!passes), "{}", run.output);
    }
}

#[tokio::test]
async fn remote_ref_resolution_callout_count_is_scale_free_in_instancefiles() {
    for num_instances in [1, 2, 10] {
        for passes in [true, false] {
            let server = Server::start();
            let env = Env::new();
            let schema_uri = server.url("/schemas/main.json");
            server.add(
                "/schemas/main.json",
                &json!({
                    "$id": schema_uri,
                    "$schema": "http://json-schema.org/draft-07/schema",
                    "properties": {"title": {"$ref": "./title_schema.json"}},
                    "additionalProperties": false
                }),
            );
            server.add("/schemas/title_schema.json", &json!({"type": "string"}));
            let paths: Vec<String> = (0..num_instances)
                .map(|i| env.write_json(&format!("instance{i}.json"), &title(passes)))
                .collect();
            let mut args = vec!["--schemafile", schema_uri.as_str()];
            args.extend(paths.iter().map(String::as_str));
            assert_eq!(env.code(&args).await, i32::from(!passes));
            assert_eq!(server.total_calls(), 2);
            assert_eq!(server.calls("/schemas/main.json"), 1);
            assert_eq!(server.calls("/schemas/title_schema.json"), 1);
        }
    }
}

// test_nonjson_schema_handling.py

fn ref_main(target: &str) -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema",
        "properties": {"title": {"$ref": target}},
        "additionalProperties": false
    })
}

#[tokio::test]
async fn yaml_and_json5_references() {
    for (target, ref_file) in [
        ("./title_schema.yaml", "title_schema.yaml"),
        ("./title_schema.json5", "title_schema.json5"),
    ] {
        for passes in [true, false] {
            let env = Env::new();
            let main = env.write_json("main_schema.json", &ref_main(target));
            env.write_json(ref_file, &json!({"type": "string"}));
            let doc = env.write_json("doc.json", &title(passes));
            assert_eq!(
                env.code(&["--schemafile", &main, &doc]).await,
                i32::from(!passes)
            );
        }
    }
}

#[tokio::test]
async fn can_load_json5_schema() {
    for passes in [true, false] {
        let env = Env::new();
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema",
            "properties": {"title": {"type": "string"}},
            "additionalProperties": false
        });
        let main = env.write_json("main_schema.json5", &schema);
        let doc = env.write_json("doc.json", &title(passes));
        assert_eq!(
            env.code(&["--schemafile", &main, &doc]).await,
            i32::from(!passes)
        );
    }
}

const REMOTE_YAML_MAIN: &str = r#""$schema": "http://json-schema.org/draft-07/schema"
properties:
  title: {"type": "string"}
additionalProperties: false
"#;

const REMOTE_YAML_REF_MAIN: &str = r#""$schema": "http://json-schema.org/draft-07/schema"
properties:
  "title": {"$ref": "./title_schema.yaml"}
additionalProperties: false
"#;

#[tokio::test]
async fn can_load_remote_yaml_schema_and_ref() {
    for main in [REMOTE_YAML_MAIN, REMOTE_YAML_REF_MAIN] {
        for passes in [true, false] {
            let server = Server::start();
            let env = Env::new();
            server.add_full("/retrieval/schemas/main.yaml", 200, main, &[]);
            server.add_full(
                "/retrieval/schemas/title_schema.yaml",
                200,
                "type: string",
                &[],
            );
            let doc = env.write_json("doc.json", &title(passes));
            let schemafile = server.url("/retrieval/schemas/main.yaml");
            let run = env.run(&["--schemafile", &schemafile, &doc]).await;
            assert_eq!(run.code, i32::from(!passes), "{}", run.output);
        }
    }
}

#[tokio::test]
async fn can_load_remote_yaml_schema_ref_from_cache() {
    let server = Server::start();
    let env = Env::new();
    server.add_full(
        "/retrieval/schemas/main.yaml",
        200,
        REMOTE_YAML_REF_MAIN,
        &[],
    );
    server.add_full("/retrieval/schemas/title_schema.yaml", 200, "false", &[]);
    env.inject_cache(
        &server.url("/retrieval/schemas/title_schema.yaml"),
        "type: string",
    );
    let doc = env.write_json("doc.json", &title(true));
    let schemafile = server.url("/retrieval/schemas/main.yaml");
    let run = env.run(&["--schemafile", &schemafile, &doc]).await;
    assert_eq!(run.code, 0, "{}", run.output);
}

// test_invalid_schema_files.py

async fn run_with_schema(schema: &str, extra: &[&str]) -> Run {
    let env = Env::new();
    let foo = env.write("foo.json", schema);
    let bar = env.write("bar.json", "{}");
    let mut args = vec!["--schemafile", foo.as_str(), bar.as_str()];
    args.extend(extra.iter().copied());
    env.run(&args).await
}

#[tokio::test]
async fn checker_non_json_schemafile() {
    for schema in ["{", "true"] {
        let run = run_with_schema(schema, &[]).await;
        assert_eq!(run.code, 1);
        assert!(
            run.output.contains("schemafile could not be parsed"),
            "{}",
            run.output
        );
    }
}

#[tokio::test]
async fn checker_invalid_schemafile() {
    let run = run_with_schema(r#"{"title": {"foo": "bar"}}"#, &[]).await;
    assert_eq!(run.code, 1);
    assert!(
        run.output.contains("schemafile was not valid"),
        "{}",
        run.output
    );
}

#[tokio::test]
async fn checker_invalid_schemafile_scheme() {
    let env = Env::new();
    let foo = env.write("foo.json", r#"{"title": "foo"}"#);
    let bar = env.write("bar.json", "{}");
    let run = env
        .run(&["--schemafile", &format!("ftp://{foo}"), &bar])
        .await;
    assert_eq!(run.code, 1);
    assert!(
        run.output.contains("only supports http, https"),
        "{}",
        run.output
    );
}

#[tokio::test]
async fn checker_invalid_schemafile_due_to_bad_regex() {
    for extra in [
        &[][..],
        &["--disable-formats", "*"],
        &["--disable-formats", "regex"],
    ] {
        // Too many backslash escapes: not a valid Unicode-mode regex.
        let run =
            run_with_schema(r#"{"properties": {"foo": {"pattern": "\\\\p{N}"}}}"#, extra).await;
        assert_eq!(run.code, 1, "{extra:?}");
        assert!(
            run.output.contains("schemafile was not valid"),
            "{}",
            run.output
        );
    }
}

// test_format_failure.py

#[tokio::test]
async fn format_checks() {
    let env = Env::new();
    let schema = env.write_json(
        "schema.json",
        &json!({
            "$schema": "http://json-schema.org/draft-07/schema",
            "properties": {
                "title": {"type": "string"},
                "date": {"type": "string", "format": "date"}
            }
        }),
    );
    let good = env.write_json(
        "doc2.json",
        &json!({"title": "doc one", "date": "2021-10-28"}),
    );
    let bad = env.write_json("doc1.json", &json!({"title": "doc one", "date": "foo"}));
    assert_eq!(env.code(&["--schemafile", &schema, &good]).await, 0);
    assert_eq!(env.code(&["--schemafile", &schema, &bad]).await, 1);
    let disabled = ["--disable-formats", "*", "--schemafile", &schema];
    assert_eq!(env.code(&[&disabled[..], &[&bad]].concat()).await, 0);
    assert_eq!(env.code(&[&disabled[..], &[&bad, &good]].concat()).await, 0);
}

/// Upstream only enforces the formats its default dependencies can check.
#[tokio::test]
async fn formats_python_does_not_check_are_ignored() {
    let env = Env::new();
    let schema = env.write_json(
        "schema.json",
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "uri": {"format": "uri"},
                "host": {"format": "hostname"},
                "duration": {"format": "duration"},
                "pointer": {"format": "json-pointer"},
                "email": {"format": "email"},
                "uuid": {"format": "uuid"}
            }
        }),
    );
    let doc = env.write_json(
        "doc.json",
        &json!({"uri": "not a uri", "host": "-bad-", "duration": "P1", "pointer": "x", "email": "a@"}),
    );
    assert_eq!(env.code(&["--schemafile", &schema, &doc]).await, 0);
    for bad in [json!({"email": "nope"}), json!({"uuid": "x"})] {
        let bad = env.write_json("bad.json", &bad);
        assert_eq!(env.code(&["--schemafile", &schema, &bad]).await, 1);
    }
}

// test_format_regex_opts.py

const REGEX_OPTS: &[&[&str]] = &[
    &["--disable-formats", "regex"],
    &["--format-regex", "default"],
    &["--format-regex", "python"],
    &["--regex-variant", "python"],
    &["--regex-variant", "default"],
    &["--regex-variant", "default", "--format-regex", "python"],
    &["--regex-variant", "python", "--format-regex", "default"],
];

async fn run_regex(opts: &[&str], schema: &Value, doc: &Value) -> Run {
    let env = Env::new();
    let schema = env.write_json("schema.json", schema);
    let doc = env.write_json("doc.json", doc);
    let mut args = opts.to_vec();
    args.extend(["--schemafile", schema.as_str(), doc.as_str()]);
    env.run(&args).await
}

fn regex_schema(kind: &str) -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema",
        "properties": {"pattern": {"type": kind, "format": "regex"}}
    })
}

#[tokio::test]
async fn regex_format_good_bad_and_non_str() {
    for opts in REGEX_OPTS {
        let good = run_regex(opts, &regex_schema("string"), &json!({"pattern": "ab*c"})).await;
        assert_eq!(good.code, 0, "{opts:?}: {}", good.output);

        let non_str = run_regex(opts, &regex_schema("integer"), &json!({"pattern": 0})).await;
        assert_eq!(non_str.code, 0, "{opts:?}: {}", non_str.output);

        let bad = run_regex(opts, &regex_schema("string"), &json!({"pattern": "a(b*c"})).await;
        let expect_ok = *opts == ["--disable-formats", "regex"];
        assert_eq!(bad.code, i32::from(!expect_ok), "{opts:?}: {}", bad.output);
    }
}

#[tokio::test]
async fn regex_format_js_specific() {
    for opts in REGEX_OPTS {
        let run = run_regex(
            opts,
            &regex_schema("string"),
            &json!({"pattern": "a(?<captured>)bc"}),
        )
        .await;
        let expect_ok = !matches!(opts[..2], ["--format-regex" | "--regex-variant", "python"]);
        assert_eq!(run.code, i32::from(!expect_ok), "{opts:?}: {}", run.output);
    }
}

#[tokio::test]
async fn regex_format_in_renovate_config() {
    let env = Env::new();
    let doc = env.write_json(
        "doc.json",
        &json!({
            "regexManagers": [{
                "fileMatch": ["^Dockerfile$"],
                "matchStrings": ["ENV YARN_VERSION=(?<currentValue>.*?)\n"],
                "depNameTemplate": "yarn",
                "datasourceTemplate": "npm"
            }]
        }),
    );
    let run = env
        .run(&["--builtin-schema", "vendor.renovate", &doc])
        .await;
    assert_eq!(run.code, 0, "{}", run.output);
}

// test_fill_defaults.py

#[tokio::test]
async fn fill_defaults() {
    let env = Env::new();
    let schema = env.write_json(
        "schema.json",
        &json!({
            "$schema": "http://json-schema.org/draft-07/schema",
            "properties": {"title": {"type": "string", "default": "Untitled"}},
            "required": ["title"]
        }),
    );
    let valid = env.write_json("valid.json", &json!({"title": "doc one"}));
    let invalid = env.write_json("invalid.json", &json!({"title": {"foo": "bar"}}));
    let missing = env.write_json("missing.json", &json!({}));
    let fill = ["--fill-defaults", "--schemafile", &schema];
    assert_eq!(env.code(&[&fill[..], &[&valid]].concat()).await, 0);
    assert_eq!(env.code(&[&fill[..], &[&invalid]].concat()).await, 1);
    assert_eq!(env.code(&["--schemafile", &schema, &missing]).await, 1);
    assert_eq!(env.code(&[&fill[..], &[&missing]].concat()).await, 0);
}

// test_malformed_instances.py

#[tokio::test]
async fn non_json_instance() {
    let env = Env::new();
    let schema = env.write("schema.json", "{}");
    let instance = env.write("instance.json", "{");
    let run = env.run(&["--schemafile", &schema, &instance]).await;
    assert_eq!(run.code, 1);
    assert!(
        run.output.contains(&format!("{instance}: Failed to parse")),
        "{}",
        run.output
    );
}

#[tokio::test]
async fn non_json_instance_mixed_with_valid_and_invalid_data() {
    for with_bad in [false, true] {
        for format in ["TEXT", "JSON"] {
            let env = Env::new();
            let schema = env.write_json(
                "schema.json",
                &json!({
                    "$schema": "http://json-schema.org/draft-07/schema",
                    "properties": {"title": {"type": "string"}},
                    "required": ["title"]
                }),
            );
            let malformed = env.write("malformed_instance.json", "{");
            let good = env.write("good_instance.json", r#"{"title": "ohai"}"#);
            let bad = env.write("bad_instance.json", r#"{"title": false}"#);
            let mut args = vec!["-o", format, "--schemafile", &schema, &good, &malformed];
            if with_bad {
                args.push(&bad);
            }
            let run = env.run(&args).await;
            assert_eq!(run.code, 1);
            if format == "TEXT" {
                assert!(
                    run.output
                        .contains(&format!("{malformed}: Failed to parse")),
                    "{}",
                    run.output
                );
                if with_bad {
                    let expected = format!(r#"{bad}:1: /title: false is not of type "string""#);
                    assert!(run.output.contains(&expected), "{}", run.output);
                }
                continue;
            }
            let report: Value = serde_json::from_str(&run.output).unwrap();
            assert_eq!(report["status"], "fail");
            assert_eq!(
                report["errors"].as_array().unwrap().len(),
                usize::from(with_bad)
            );
            let parse_errors = report["parse_errors"].as_array().unwrap();
            assert_eq!(parse_errors.len(), 1);
            assert_eq!(parse_errors[0]["filename"], malformed.as_str());
            let message = parse_errors[0]["message"].as_str().unwrap();
            assert!(message.contains(&format!("Failed to parse {malformed}")));
            if with_bad {
                assert_eq!(report["errors"][0]["path"], "$.title");
            }
        }
    }
}

// test_nonjson_instance_files.py

#[tokio::test]
async fn json5_filetype_forced_on_json_suffixed_instance() {
    for passes in [true, false] {
        let env = Env::new();
        let schema = env.write_json(
            "schema.json",
            &json!({
                "$schema": "http://json-schema.org/draft-07/schema",
                "properties": {"title": {"type": "string"}},
                "additionalProperties": false
            }),
        );
        let doc = env.write("doc.json", &format!("// a comment\n{}\n", title(passes)));
        let forced = ["--force-filetype", "json5", "--schemafile", &schema, &doc];
        assert_eq!(env.code(&forced).await, i32::from(!passes));
        if passes {
            assert_eq!(env.code(&["--schemafile", &schema, &doc]).await, 1);
        }
    }
}

// test_special_filetypes.py. The memfd, fifo and stdin cases do not apply to prek hooks.

#[tokio::test]
async fn remote_schema_requiring_retry() {
    for passes in [true, false] {
        let server = Server::start();
        let env = Env::new();
        server.add_full("/schema1.json", 200, "", &[]);
        server.add_full(
            "/schema1.json",
            200,
            r#"{"type": "integer"}"#,
            &[("Last-Modified", "Sun, 01 Jan 2000 00:00:01 GMT")],
        );
        let instance = env.write("instance.json", if passes { "42" } else { r#""foo""# });
        let schemafile = server.url("/schema1.json");
        let run = env.run(&["--schemafile", &schemafile, &instance]).await;
        assert_eq!(run.code, i32::from(!passes), "{}", run.output);
    }
}

// test_gitlab_reference_handling.py

#[tokio::test]
async fn gitlab_reference_handling() {
    for (passes, script) in [
        (
            false,
            "    # !reference not a list, error\n    - !reference .setup\n",
        ),
        (true, "    - !reference [.setup, script]\n"),
    ] {
        let env = Env::new();
        let doc = env.write(
            "data.yml",
            &format!(
                ".setup:\n  script:\n    - echo setup\n\ntest:\n  script:\n{script}    - echo running my own command\n"
            ),
        );
        let args = [
            "--builtin-schema",
            "gitlab-ci",
            "--data-transform",
            "gitlab-ci",
            &doc,
        ];
        let run = env.run(&args).await;
        assert_eq!(run.code, i32::from(!passes), "{}", run.output);
    }
}

// tests/unit/cli/test_parse.py

#[tokio::test]
async fn schema_options_are_required_and_mutually_exclusive() {
    let env = Env::new();
    let doc = env.write("doc.json", "{}");
    let schema = env.write("schema.json", "{}");
    for args in [
        vec![doc.as_str()],
        vec![
            "--schemafile",
            &schema,
            "--builtin-schema",
            "renovate",
            &doc,
        ],
        vec!["--schemafile", &schema, "--check-metaschema", &doc],
        vec!["--builtin-schema", "renovate", "--check-metaschema", &doc],
        vec!["--check-metaschema", "--base-uri", "https://x", &doc],
        vec![
            "--validator-class",
            "foo:Bar",
            "--schemafile",
            &schema,
            &doc,
        ],
    ] {
        assert!(
            check(parse_args(&args), env.context(), &[]).await.is_err(),
            "{args:?}"
        );
    }
}

#[test]
fn accepts_python_only_flags() {
    let args = parse_args(&[
        "--schemafile",
        "s.json",
        "--traceback-mode",
        "full",
        "--cache-filename",
        "x",
        "--color",
        "never",
        "-vv",
        "-q",
        "-o",
        "JSON",
    ]);
    assert_eq!(args.verbosity(), 2);
    assert!(Args::try_parse_from(["check-jsonschema", "--color", "sometimes"]).is_err());
}

#[tokio::test]
async fn quiet_prints_nothing() {
    let env = Env::new();
    let schema = env.write("schema.json", r#"{"type": "integer"}"#);
    let doc = env.write("doc.json", r#""x""#);
    let run = env.run(&["-q", "--schemafile", &schema, &doc]).await;
    assert_eq!(run.code, 1);
    assert_eq!(run.output, "");
}

// tests/unit/test_cachedownloader.py

#[tokio::test]
async fn cachedownloader_succeeds_after_few_errors() {
    for failures in [1, 2] {
        for disable_cache in [true, false] {
            let server = Server::start();
            let env = Env::new();
            for _ in 0..failures {
                server.add_full("/schema1.json", 500, "{}", &[]);
            }
            server.add_full("/schema1.json", 200, r#"{"type": "integer"}"#, &[]);
            let url = server.url("/schema1.json");
            let instance = env.write("instance.json", "42");
            let mut args = vec!["--schemafile", url.as_str(), instance.as_str()];
            if disable_cache {
                args.push("--no-cache");
            }
            assert_eq!(env.code(&args).await, 0);
            assert_eq!(env.cache_path(&url).exists(), !disable_cache);
        }
    }
}

#[tokio::test]
async fn cachedownloader_fails_after_many_errors() {
    let server = Server::start();
    let env = Env::new();
    server.add_full("/schema1.json", 500, "{}", &[]);
    let url = server.url("/schema1.json");
    let instance = env.write("instance.json", "42");
    let run = env.run(&["--schemafile", &url, &instance]).await;
    assert_eq!(run.code, 1);
    assert_eq!(server.calls("/schema1.json"), 3);
    assert!(!env.cache_path(&url).exists());
}

#[tokio::test]
async fn cachedownloader_retries_on_bad_data() {
    let server = Server::start();
    let env = Env::new();
    server.add_full("/schema1.json", 200, "{", &[]);
    server.add_full("/schema1.json", 200, r#"{"type": "integer"}"#, &[]);
    let url = server.url("/schema1.json");
    let instance = env.write("instance.json", "42");
    assert_eq!(env.code(&["--schemafile", &url, &instance]).await, 0);
    let cached = fs_err::read_to_string(env.cache_path(&url)).unwrap();
    assert_eq!(cached, r#"{"type": "integer"}"#);
}

#[tokio::test]
async fn cachedownloader_uses_cache_unless_remote_is_newer() {
    for (last_modified, expect_cached) in [
        // A missing or malformed header counts as the epoch, so the cache wins.
        (None, true),
        (Some("Jan 2000 00:00:01"), true),
        (Some("Sun, 01 Jan 2000 00:00:01 GMT"), true),
        // A remote file newer than the cache replaces it.
        (Some("Sun, 01 Jan 2090 00:00:01 GMT"), false),
    ] {
        let server = Server::start();
        let env = Env::new();
        let headers: Vec<(&str, &str)> = last_modified
            .map(|value| ("Last-Modified", value))
            .into_iter()
            .collect();
        server.add_full("/schema1.json", 200, r#"{"type": "string"}"#, &headers);
        let url = server.url("/schema1.json");
        env.inject_cache(&url, r#"{"type": "integer"}"#);
        let instance = env.write("instance.json", "42");
        let run = env.run(&["--schemafile", &url, &instance]).await;
        assert_eq!(
            run.code,
            i32::from(!expect_cached),
            "{last_modified:?}: {}",
            run.output
        );
    }
}

#[tokio::test]
async fn cache_hit_skips_validation_of_remote_body() {
    let server = Server::start();
    let env = Env::new();
    // The remote body is garbage, but the cache is newer than `Last-Modified`.
    server.add_full(
        "/schema1.json",
        200,
        "{",
        &[("Last-Modified", "Sun, 01 Jan 2000 00:00:01 GMT")],
    );
    let url = server.url("/schema1.json");
    env.inject_cache(&url, r#"{"type": "integer"}"#);
    let instance = env.write("instance.json", "42");
    assert_eq!(env.code(&["--schemafile", &url, &instance]).await, 0);
    assert_eq!(server.calls("/schema1.json"), 1);
}

#[tokio::test]
async fn stale_cache_is_replaced() {
    let server = Server::start();
    let env = Env::new();
    let url = server.url("/schema1.json");
    server.add_full(
        "/schema1.json",
        200,
        r#"{"type": "string"}"#,
        &[("Last-Modified", "Sun, 01 Jan 2000 00:00:01 GMT")],
    );
    env.inject_cache(&url, r#"{"type": "integer"}"#);
    let old = SystemTime::UNIX_EPOCH + Duration::from_hours(250_000);
    fs_err::File::options()
        .write(true)
        .open(env.cache_path(&url))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let instance = env.write("instance.json", r#""text""#);
    assert_eq!(env.code(&["--schemafile", &url, &instance]).await, 0);
    let cached = fs_err::read_to_string(env.cache_path(&url)).unwrap();
    assert_eq!(cached, r#"{"type": "string"}"#);
}

// --check-metaschema

#[tokio::test]
async fn check_metaschema() {
    let env = Env::new();
    let draft7 = "http://json-schema.org/draft-07/schema#";
    let good = env.write_json("good.json", &json!({"$schema": draft7, "type": "string"}));
    let bad = env.write_json("bad.json", &json!({"$schema": draft7, "type": "nope"}));
    let bad_regex = env.write_json("bad_regex.json", &json!({"pattern": "a(b"}));
    assert_eq!(env.code(&["--check-metaschema", &good]).await, 0);
    assert_eq!(env.code(&["--check-metaschema", &bad]).await, 1);
    assert_eq!(env.code(&["--check-metaschema", &bad_regex]).await, 1);
}

// test_example_files.py: every upstream hook entry against upstream example files.

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check-jsonschema")
}

/// Upstream hooks from `.pre-commit-hooks.yaml`: (id, entry arguments, manifest entry).
fn upstream_hooks() -> Vec<(String, Vec<String>, Value)> {
    let manifest = fs_err::read_to_string(fixtures().join("pre-commit-hooks.yaml")).unwrap();
    let Value::Array(hooks) = super::filetype::FileType::Yaml.parse(&manifest).unwrap() else {
        panic!("manifest is not a list");
    };
    hooks
        .into_iter()
        .map(|hook| {
            let id = hook["id"].as_str().unwrap().to_string();
            let mut entry: Vec<String> = hook["entry"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            assert_eq!(entry.remove(0), "check-jsonschema");
            (id, entry, hook)
        })
        .collect()
}

/// `add_args` and `requires_packages` from a case directory's `_config.yaml`.
fn case_config(dir: &std::path::Path, name: &str) -> Value {
    let Ok(config) = fs_err::read_to_string(dir.join("_config.yaml")) else {
        return Value::Null;
    };
    let config = super::filetype::FileType::Yaml.parse(&config).unwrap();
    config["files"][name].clone()
}

#[tokio::test]
async fn hook_examples() {
    let mut checked = 0;
    for (id, entry, _) in upstream_hooks() {
        let hook_name = id.trim_start_matches("check-");
        // `example-files/hooks` is upstream's corpus. `parity` adds a passing and a failing
        // file for hooks upstream does not cover, each verified against Python
        // check-jsonschema (see the fixtures README).
        for (root, category, expected) in [
            ("example-files/hooks", "positive", 0),
            ("example-files/hooks", "negative", 1),
            ("parity", "positive", 0),
            ("parity", "negative", 1),
        ] {
            let dir = fixtures().join(root).join(category).join(hook_name);
            let Ok(cases) = fs_err::read_dir(&dir) else {
                continue;
            };
            for case in cases {
                let path = case.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                if name == "_config.yaml" {
                    continue;
                }
                let config = case_config(&dir, &name);
                // Upstream skips cases that need optional Python packages, such as `rfc3987`
                // for the `uri` format, which upstream does not check without them.
                if config.get("requires_packages").is_some() {
                    continue;
                }
                let add_args: Vec<String> = config["add_args"]
                    .as_array()
                    .map(|args| {
                        args.iter()
                            .map(|arg| arg.as_str().unwrap().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let path_arg = path.display().to_string();
                let mut args: Vec<&str> = entry.iter().map(String::as_str).collect();
                args.push(&path_arg);
                args.extend(add_args.iter().map(String::as_str));
                let run = Env::new().run(&args).await;
                assert_eq!(run.code, expected, "{id} {root}/{category}/{name}: {}", run.output);
                checked += 1;
            }
        }
    }
    assert!(checked >= 70, "only {checked} cases ran");
}

#[tokio::test]
async fn explicit_schema_examples() {
    for (category, expected) in [("positive", 0), ("negative", 1)] {
        let dir = fixtures()
            .join("example-files/explicit-schema")
            .join(category);
        for case in fs_err::read_dir(&dir).unwrap() {
            let case = case.unwrap().path();
            let find = |names: &[&str]| {
                names
                    .iter()
                    .map(|name| case.join(name))
                    .find(|path| path.exists())
                    .unwrap()
                    .display()
                    .to_string()
            };
            let instance = find(&["instance.json", "instance.yaml", "instance.toml"]);
            let schema = find(&["schema.json", "schema.yaml"]);
            let run = Env::new().run(&["--schemafile", &schema, &instance]).await;
            assert_eq!(run.code, expected, "{}: {}", case.display(), run.output);
        }
    }
}

// test_hook_file_matches.py, evaluated with prek's `files` and `types` matching.

const HOOK_PATHS: &[(&str, &[&str], &[&str])] = &[
    (
        "check-azure-pipelines",
        &[
            "azure-pipelines.yml",
            "azure-pipelines.yaml",
            ".azure-pipelines.yml",
            ".azure-pipelines.yaml",
        ],
        &["foo.yml", "foo/azure-pipelines.yaml"],
    ),
    (
        "check-bamboo-spec",
        &["bamboo-specs/foo.yml", "bamboo-specs/foo.yaml"],
        &[
            "bamboo-specs.yaml",
            "bamboo-spec/foo.yml",
            "bamboo-specs/README.md",
        ],
    ),
    (
        "check-changie",
        &[".changie.yml", ".changie.yaml"],
        &["changie.yml", ".changie.json"],
    ),
    (
        "check-citation-file-format",
        &["CITATION.cff"],
        &["CITATION.yml"],
    ),
    (
        "check-codecov",
        &[
            "codecov.yml",
            "codecov.yaml",
            ".codecov.yml",
            ".codecov.yaml",
            ".github/codecov.yml",
            ".github/codecov.yaml",
            ".github/.codecov.yml",
            ".github/.codecov.yaml",
            "dev/codecov.yml",
            "dev/codecov.yaml",
            "dev/.codecov.yml",
            "dev/.codecov.yaml",
        ],
        &[".gitlab/codecov.yml"],
    ),
    (
        "check-compose-spec",
        &[
            "compose.yml",
            "compose.yaml",
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.override.yml",
            "docker-compose.override.yml",
            "path/to/compose.yml",
        ],
        &["docker.compose.yml", "docker.md", "Dockerfile"],
    ),
    (
        "check-meltano",
        &[
            "meltano.yml",
            "data/meltano.yml",
            "extractors.meltano.yml",
            "meltano-manifest.json",
            "meltano-manifest.prod.json",
        ],
        &["meltano.yaml", "meltano.yml.md", "meltano-manifest.yml"],
    ),
    (
        "check-dependabot",
        &[".github/dependabot.yml", ".github/dependabot.yaml"],
        &[".dependabot.yaml", ".dependabot.yml"],
    ),
    (
        "check-github-actions",
        &[
            "action.yaml",
            ".github/actions/action.yml",
            ".github/actions/foo/bar/action.yaml",
            ".github/actions/path with spaces/action.yml",
        ],
        &[".github/actions/foo/other.yaml"],
    ),
    (
        "check-github-discussion",
        &[
            ".github/DISCUSSION_TEMPLATE/announcements.yaml",
            ".github/DISCUSSION_TEMPLATE/office-hours.yml",
        ],
        &[
            ".github/discussion.yml",
            ".github/DISCUSSION_TEMPLATE/not-a-discussion.txt",
        ],
    ),
    (
        "check-github-issue-config",
        &[".github/ISSUE_TEMPLATE/config.yml"],
        &[
            ".github/ISSUE_TEMPLATE/config.yaml",
            ".github/ISSUE_TEMPLATE/bug.yml",
        ],
    ),
    (
        "check-github-issue-forms",
        &[
            ".github/ISSUE_TEMPLATE/feature.yaml",
            ".github/ISSUE_TEMPLATE/bug.yml",
        ],
        &[
            ".github/ISSUE_TEMPLATE/config.yaml",
            ".github/ISSUE_TEMPLATE/config.yml",
        ],
    ),
    (
        "check-github-workflows",
        &[
            ".github/workflows/build.yml",
            ".github/workflows/build.yaml",
        ],
        &[".github/workflows.yaml", ".github/workflows/foo/bar.yaml"],
    ),
    (
        "check-gitlab-ci",
        &[
            ".gitlab-ci.yml",
            ".gitlab/.gitlab-ci.yml",
            "gitlab/.gitlab-ci.yml",
            ".gitlab-ci.yaml",
        ],
        &[
            "gitlab-ci.yml",
            ".gitlab/gitlab-ci.yml",
            "gitlab/gitlab-ci.yml",
        ],
    ),
    (
        "check-readthedocs",
        &[".readthedocs.yml", ".readthedocs.yaml"],
        &["readthedocs.yml", "readthedocs.yaml"],
    ),
    (
        "check-renovate",
        &[
            "renovate.json",
            "renovate.json5",
            ".github/renovate.json",
            ".gitlab/renovate.json5",
            ".renovaterc",
            ".renovaterc.json",
        ],
        &[".github/renovaterc", ".renovate", ".renovate.json"],
    ),
    (
        "check-snapcraft",
        &[
            "snapcraft.yaml",
            "snap/snapcraft.yaml",
            "foo/bar/snapcraft.yaml",
        ],
        &["snapcraft.yml", "snap.yaml", "snapcraft"],
    ),
    (
        "check-travis",
        &[".travis.yml", ".travis.yaml"],
        &["travis.yml", ".travis"],
    ),
];

#[test]
fn hook_file_patterns_match_like_upstream() {
    use crate::config::FilePattern;
    use prek_identify::{TagSet, tags_from_filename};

    let hooks = upstream_hooks();
    for (id, good, bad) in HOOK_PATHS {
        let (_, _, manifest) = hooks.iter().find(|(hook_id, _, _)| hook_id == id).unwrap();
        let files = FilePattern::regex(manifest["files"].as_str().unwrap()).unwrap();
        let tag_list = |key: &str| -> Vec<String> {
            manifest[key]
                .as_array()
                .map(|tags| {
                    tags.iter()
                        .map(|tag| tag.as_str().unwrap().to_string())
                        .collect()
                })
                .unwrap_or_default()
        };
        let (types, types_or) = (tag_list("types"), tag_list("types_or"));
        for path in *good {
            assert!(
                files.is_match(std::path::Path::new(path)),
                "{id} should match {path}"
            );
            let tags = tags_from_filename(std::path::Path::new(path));
            if !types.is_empty() {
                assert!(
                    TagSet::from_tags(&types).is_subset(&tags),
                    "{id} types {types:?} vs {path}"
                );
            }
            if !types_or.is_empty() {
                assert!(
                    !TagSet::from_tags(&types_or).is_disjoint(&tags),
                    "{id} types_or vs {path}"
                );
            }
        }
        for path in *bad {
            assert!(
                !files.is_match(std::path::Path::new(path)),
                "{id} should not match {path}"
            );
        }
    }
}

// Upstream issues

/// Issue #640: a `$ref` inside a referenced local file resolves against that file.
#[tokio::test]
async fn nested_local_refs() {
    for passes in [true, false] {
        let env = Env::new();
        let main = env.write_json(
            "schemas/main.json",
            &json!({"properties": {"title": {"$ref": "./sub/a.json"}}}),
        );
        env.write_json("schemas/sub/a.json", &json!({"$ref": "./b.json"}));
        env.write_json("schemas/sub/b.json", &json!({"type": "string"}));
        let doc = env.write_json("doc.json", &title(passes));
        let run = env.run(&["--schemafile", &main, &doc]).await;
        assert_eq!(run.code, i32::from(!passes), "{}", run.output);
    }
}

/// Issue #549: in Draft 7 an `$id` next to `$ref` is ignored, so the `urn:` indirection does
/// not resolve (upstream fails the same way). The 2019-09 form works.
#[tokio::test]
async fn urn_ref_indirection() {
    let schema = |draft: &str, defs: &str| {
        json!({
            "$schema": draft,
            "$id": "urn:chart",
            "type": "object",
            "properties": {"someVal": {"$ref": "urn:chart/indirection"}},
            defs: {
                "indirection": {"$id": "urn:chart/indirection", "$ref": "urn:chart/string-type"},
                "string": {"$id": "urn:chart/string-type", "type": "string"}
            }
        })
    };
    let env = Env::new();
    let draft7 = env.write_json(
        "draft7.json",
        &schema("http://json-schema.org/draft-07/schema", "definitions"),
    );
    let draft2019 = env.write_json(
        "draft2019.json",
        &schema("https://json-schema.org/draft/2019-09/schema", "$defs"),
    );
    let good = env.write("good.yaml", "someVal: \"a string\"\n");
    let bad = env.write("bad.yaml", "someVal: 3\n");

    let run = env.run(&["--schemafile", &draft7, &good]).await;
    assert_eq!(run.code, 1);
    assert!(
        run.output.contains("Failure resolving $ref"),
        "{}",
        run.output
    );
    assert_eq!(env.code(&["--schemafile", &draft2019, &good]).await, 0);
    assert_eq!(env.code(&["--schemafile", &draft2019, &bad]).await, 1);
}

/// Issue #222: every document of a multi-document YAML file is validated.
#[tokio::test]
async fn multi_document_yaml() {
    let env = Env::new();
    let schema = env.write_json(
        "schema.json",
        &json!({"type": "object", "required": ["kind"]}),
    );
    let good = env.write("good.yaml", "---\nkind: System\n---\nkind: Component\n");
    let bad = env.write("bad.yaml", "kind: System\n---\nname: no-kind\n");
    let top_level_list = env.write("list.yaml", "- a\n- b\n");
    assert_eq!(env.code(&["--schemafile", &schema, &good]).await, 0);
    let run = env.run(&["--schemafile", &schema, &bad]).await;
    assert_eq!(run.code, 1);
    assert!(
        run.output.contains(&format!(
            "{bad} (document 2):3: /: \"kind\" is a required property"
        )),
        "{}",
        run.output
    );
    assert!(!run.output.contains("document 1"), "{}", run.output);
    let list_schema = env.write_json("list_schema.json", &json!({"type": "array"}));
    assert_eq!(
        env.code(&["--schemafile", &list_schema, &top_level_list])
            .await,
        0
    );
}

/// Issue #561: a GitLab CI file with a `spec:inputs` header document.
#[tokio::test]
async fn gitlab_ci_spec_inputs_header() {
    let env = Env::new();
    let doc = env.write(
        ".gitlab-ci.yml",
        "spec:\n  inputs:\n    job-stage:\n      default: test\n---\nscan-website:\n  stage: $[[ inputs.job-stage ]]\n  script: ./scan-website\n",
    );
    let run = env
        .run(&[
            "--builtin-schema",
            "vendor.gitlab-ci",
            "--data-transform",
            "gitlab-ci",
            &doc,
        ])
        .await;
    assert_eq!(run.code, 0, "{}", run.output);
}

/// Issues #310, #340 and #644: `--schema-from-instances`.
#[tokio::test]
async fn schema_from_instances() {
    let env = Env::new();
    env.write_json(
        "schemas/title.json",
        &json!({"properties": {"title": {"type": "string"}}}),
    );
    env.write_json(
        "schemas/count.json",
        &json!({"properties": {"count": {"type": "integer"}}}),
    );
    let by_key = env.write_json(
        "data/a.json",
        &json!({"$schema": "../schemas/title.json", "title": 2}),
    );
    let by_modeline = env.write(
        "data/b.yaml",
        "# yaml-language-server: $schema=../schemas/count.json\ncount: nope\n",
    );
    let valid = env.write(
        "data/c.yaml",
        "# yaml-language-server: $schema=../schemas/count.json\ncount: 3\n",
    );
    let undeclared = env.write_json("data/d.json", &json!({"title": 2}));
    let missing_schema = env.write_json("data/e.json", &json!({"$schema": "../schemas/nope.json"}));

    assert_eq!(env.code(&["--schema-from-instances", &valid]).await, 0);
    let run = env
        .run(&[
            "--schema-from-instances",
            &by_key,
            &by_modeline,
            &undeclared,
            &missing_schema,
        ])
        .await;
    assert_eq!(run.code, 1);
    for expected in [
        format!("{by_key}:1: /title: 2 is not of type \"string\""),
        format!("{by_modeline}:2: /count: \"nope\" is not of type \"integer\""),
        format!("{undeclared}: no schema declared"),
        format!("{missing_schema}: Error: schemafile could not be parsed"),
    ] {
        assert!(
            run.output.contains(&expected),
            "missing `{expected}` in:\n{}",
            run.output
        );
    }

    // `--schemafile` applies to documents that declare no schema.
    let fallback = env.write_json(
        "fallback.json",
        &json!({"properties": {"title": {"type": "string"}}}),
    );
    let run = env
        .run(&[
            "--schema-from-instances",
            "--schemafile",
            &fallback,
            &undeclared,
            &valid,
        ])
        .await;
    assert_eq!(run.code, 1);
    assert!(
        run.output.contains(&format!("{undeclared}:1: /title")),
        "{}",
        run.output
    );
    assert!(!run.output.contains("c.yaml"), "{}", run.output);
}

/// Issue #668: without `Last-Modified`, the `ETag` decides whether the cache is fresh.
#[tokio::test]
async fn etag_cache_validation() {
    for (cached_etag, expect_cached) in [
        (Some("\"v2\""), true),
        (Some("\"v1\""), false),
        (None, false),
    ] {
        let server = Server::start();
        let env = Env::new();
        let url = server.url("/schema1.json");
        server.add_full(
            "/schema1.json",
            200,
            r#"{"type": "string"}"#,
            &[("ETag", "\"v2\"")],
        );
        env.inject_cache(&url, r#"{"type": "integer"}"#);
        if let Some(etag) = cached_etag {
            fs_err::write(format!("{}.etag", env.cache_path(&url).display()), etag).unwrap();
        }
        let instance = env.write("instance.json", "42");
        let run = env.run(&["--schemafile", &url, &instance]).await;
        assert_eq!(
            run.code,
            i32::from(!expect_cached),
            "{cached_etag:?}: {}",
            run.output
        );
        let saved =
            fs_err::read_to_string(format!("{}.etag", env.cache_path(&url).display())).unwrap();
        assert_eq!(saved, "\"v2\"");
    }
}

#[tokio::test]
async fn not_modified_reuses_cache() {
    let server = Server::start();
    let env = Env::new();
    let url = server.url("/schema1.json");
    server.add_full("/schema1.json", 304, "", &[("ETag", "\"v1\"")]);
    env.inject_cache(&url, r#"{"type": "integer"}"#);
    fs_err::write(format!("{}.etag", env.cache_path(&url).display()), "\"v1\"").unwrap();
    let instance = env.write("instance.json", "42");
    assert_eq!(env.code(&["--schemafile", &url, &instance]).await, 0);
}

/// Issue #359: errors report the line of the failing value.
#[tokio::test]
async fn errors_report_line_numbers() {
    let env = Env::new();
    let schema = env.write_json(
        "schema.json",
        &json!({"properties": {"jobs": {"items": {"properties": {"name": {"type": "string"}}}}}}),
    );
    let yaml = env.write("ci.yaml", "# header\njobs:\n  - name: a\n  - name: 3\n");
    let json = env.write(
        "ci.json",
        "{\n  \"jobs\": [\n    {\"name\": \"a\"},\n    {\"name\": 3}\n  ]\n}\n",
    );
    let toml = env.write("ci.toml", "[[jobs]]\nname = 3\n");
    let run = env
        .run(&["-o", "json", "--schemafile", &schema, &yaml, &json, &toml])
        .await;
    let report: Value = serde_json::from_str(&run.output).unwrap();
    let lines: Vec<&Value> = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|error| &error["line"])
        .collect();
    assert_eq!(lines, [&json!(4), &json!(4), &Value::Null]);
    let text = env.run(&["--schemafile", &schema, &yaml]).await.output;
    assert!(
        text.starts_with(&format!("{yaml}:4: /jobs/1/name: 3 is not of type")),
        "{text}"
    );
}

/// Every bundled schema loads and compiles, so every upstream hook can run.
#[tokio::test]
async fn every_builtin_schema_compiles() {
    let env = Env::new();
    let doc = env.write("doc.json", "{}");
    for name in super::catalog::builtin_schema_names() {
        let run = env.run(&["--builtin-schema", &name, &doc]).await;
        assert!(
            !run.output.contains("Error:") && !run.output.contains("Failure resolving"),
            "{name}: {}",
            run.output
        );
    }
}
