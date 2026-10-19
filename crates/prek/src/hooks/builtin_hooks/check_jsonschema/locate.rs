//! Line numbers for validation errors (upstream issue #359).
//!
//! The validated values carry no positions, so the source is parsed again, only when a file
//! has errors, into a tree of node lines. JSON is YAML flow syntax, so one YAML event parser
//! covers both formats. TOML and JSON5 files report no lines.

use granit_parser::{Event, Parser};

enum Node {
    Map(Vec<(String, usize, Node)>),
    Seq(Vec<(usize, Node)>),
    Leaf,
}

/// The line of every top-level value and nested node of the documents in `text`.
pub(super) struct Lines {
    documents: Vec<(usize, Node)>,
}

impl Lines {
    pub(super) fn parse(text: &str) -> Option<Self> {
        let mut events = Parser::new_from_str(text)
            .filter_map(Result::ok)
            .filter(|(event, _)| !matches!(event, Event::Comment(..)));
        let mut documents = Vec::new();
        while let Some((event, _)) = events.next() {
            if matches!(event, Event::DocumentStart(..)) {
                let (event, span) = events.next()?;
                let line = span.start.line();
                documents.push((line, build(&event, &mut events)?));
            }
        }
        Some(Self { documents })
    }

    /// The line of the deepest node along `path` in document `index`.
    pub(super) fn line(&self, index: usize, path: &[String]) -> Option<usize> {
        let (mut line, mut node) = self
            .documents
            .get(index)
            .map(|(line, node)| (*line, node))?;
        for segment in path {
            let child = match node {
                Node::Map(entries) => entries
                    .iter()
                    .rev()
                    .find(|(key, _, _)| key == segment)
                    .map(|(_, line, child)| (*line, child)),
                Node::Seq(items) => segment
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| items.get(index))
                    .map(|(line, child)| (*line, child)),
                Node::Leaf => None,
            };
            let Some((child_line, child)) = child else {
                break;
            };
            line = child_line;
            node = child;
        }
        Some(line)
    }
}

fn build<'a>(
    event: &Event<'a>,
    events: &mut impl Iterator<Item = (Event<'a>, granit_parser::Span)>,
) -> Option<Node> {
    Some(match event {
        Event::MappingStart(..) => {
            let mut entries = Vec::new();
            loop {
                let (key_event, key_span) = events.next()?;
                let key = match key_event {
                    Event::MappingEnd => break,
                    Event::Scalar(value, ..) => value.to_string(),
                    other => {
                        // Complex keys are skipped along with their value.
                        build(&other, events)?;
                        String::new()
                    }
                };
                let (value_event, _) = events.next()?;
                entries.push((key, key_span.start.line(), build(&value_event, events)?));
            }
            Node::Map(entries)
        }
        Event::SequenceStart(..) => {
            let mut items = Vec::new();
            loop {
                let (event, span) = events.next()?;
                if matches!(event, Event::SequenceEnd) {
                    break;
                }
                items.push((span.start.line(), build(&event, events)?));
            }
            Node::Seq(items)
        }
        _ => Node::Leaf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn yaml_lines() {
        let text =
            "# comment\nname: x\njobs:\n  build:\n    steps:\n      - run: a\n      - run: b\n";
        let lines = Lines::parse(text).unwrap();
        assert_eq!(lines.line(0, &path(&[])), Some(2));
        assert_eq!(lines.line(0, &path(&["name"])), Some(2));
        assert_eq!(
            lines.line(0, &path(&["jobs", "build", "steps", "1"])),
            Some(7)
        );
        assert_eq!(
            lines.line(0, &path(&["jobs", "build", "steps", "1", "run"])),
            Some(7)
        );
        // Missing keys report the closest existing parent.
        assert_eq!(lines.line(0, &path(&["jobs", "nope", "x"])), Some(3));
    }

    #[test]
    fn json_and_documents() {
        let lines = Lines::parse("{\n  \"a\": [\n    1,\n    {\"b\": 2}\n  ]\n}\n").unwrap();
        assert_eq!(lines.line(0, &path(&["a", "1", "b"])), Some(4));
        let lines = Lines::parse("a: 1\n---\nb:\n  c: 2\n").unwrap();
        assert_eq!(lines.line(1, &path(&["b", "c"])), Some(4));
        assert_eq!(lines.line(2, &path(&[])), None);
    }
}
