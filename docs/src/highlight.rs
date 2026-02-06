use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::LazyLock;

use comrak::adapters::SyntaxHighlighterAdapter;
use hypertext::{Raw, prelude::*};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

// These are the standard capture names used by tree-sitter themes
pub const CAPTURE_NAMES: &[&str] = &[
    "variable",
    "variable.builtin",
    "variable.parameter",
    "variable.parameter.builtin",
    "variable.member",
    "constant",
    "constant.builtin",
    "constant.macro",
    "module",
    "module.builtin",
    "label",
    "string",
    "string.documentation",
    "string.regexp",
    "string.escape",
    "string.special",
    "string.special.symbol",
    "string.special.url",
    "string.special.path",
    "character",
    "character.special",
    "boolean",
    "number",
    "number.float",
    "type",
    "type.builtin",
    "type.definition",
    "attribute",
    "attribute.builtin",
    "property",
    "function",
    "function.builtin",
    "function.call",
    "function.macro",
    "function.method",
    "function.method.call",
    "constructor",
    "operator",
    "keyword",
    "keyword.coroutine",
    "keyword.function",
    "keyword.operator",
    "keyword.import",
    "keyword.type",
    "keyword.modifier",
    "keyword.repeat",
    "keyword.return",
    "keyword.debug",
    "keyword.exception",
    "keyword.conditional",
    "keyword.conditional.ternary",
    "keyword.directive",
    "keyword.directive.define",
    "punctuation.delimiter",
    "punctuation.bracket",
    "punctuation.special",
    "comment",
    "comment.documentation",
    "comment.error",
    "comment.warning",
    "comment.todo",
    "comment.note",
    "markup.strong",
    "markup.italic",
    "markup.strikethrough",
    "markup.underline",
    "markup.heading",
    "markup.heading.1",
    "markup.heading.2",
    "markup.heading.3",
    "markup.heading.4",
    "markup.heading.5",
    "markup.heading.6",
    "markup.quote",
    "markup.math",
    "markup.link",
    "markup.link.label",
    "markup.link.url",
    "markup.raw",
    "markup.raw.block",
    "markup.list",
    "markup.list.checked",
    "markup.list.unchecked",
    "diff.plus",
    "diff.minus",
    "diff.delta",
    "tag",
    "tag.builtin",
    "tag.attribute",
    "tag.delimiter",
];

// Helper macro to initialize the configuration
macro_rules! language {
    ($name:expr, $lang:expr, $highlights:expr, $injections:expr, $locals:expr $(,)?) => {
        ($name, {
            let lang: tree_sitter::Language = $lang.into();
            let mut config =
                HighlightConfiguration::new(lang, $name, $highlights, $injections, $locals)
                    .unwrap();
            config.configure(CAPTURE_NAMES);
            config
        })
    };
}

// The configuration map, strictly for Rust
static CONFIGS: LazyLock<HashMap<&'static str, HighlightConfiguration>> = LazyLock::new(|| {
    HashMap::from([
        language!(
            "rust",
            tree_sitter_rust::LANGUAGE,
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ),
        language!(
            "toml",
            tree_sitter_toml_ng::LANGUAGE,
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
    ])
});

pub fn get_config(name: &str) -> Option<&'static HighlightConfiguration> {
    // Simplified extension expansion just for Rust
    let key = match name {
        "rs" => "rust",
        other => other,
    };
    CONFIGS.get(key)
}

pub enum TSEvent {
    Write(String),
    Enter(String),
    Close,
}

// Main entry point to highlight code
pub fn highlight<'a>(lang: &'a str, code: &'a str) -> impl Renderable + 'a {
    maud!(
        figure .listing.atom-one-light data-lang=(lang) {
            pre {
                code {
                    (Raw::dangerously_create(to_html(lang, code)))
                }
            }
        }
    )
}

fn to_html(lang: &str, code: &str) -> String {
    get_events(lang, code)
        .into_iter()
        .map(|event| match event {
            TSEvent::Write(text) => Cow::from(
                text.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            ),
            // Transforms capture names (e.g., "variable.builtin") into CSS classes
            TSEvent::Enter(class) => {
                Cow::from(format!("<span class=\"{}\">", class.replace('.', "-")))
            }
            TSEvent::Close => Cow::from("</span>"),
        })
        .collect()
}

fn get_events(lang: &str, src: &str) -> Vec<TSEvent> {
    let config = match get_config(lang) {
        Some(c) => c,
        None => return vec![TSEvent::Write(src.into())],
    };

    let mut hl = Highlighter::new();
    // highlight returns an iterator of results
    let highlights = hl
        .highlight(config, src.as_bytes(), None, |name| get_config(name))
        .unwrap();

    let mut out = vec![];
    for event in highlights {
        let event = event.unwrap(); // Handle errors in real code
        let obj = map_event(event, src);
        out.push(obj);
    }
    out
}

fn map_event(event: HighlightEvent, src: &str) -> TSEvent {
    match event {
        HighlightEvent::Source { start, end } => TSEvent::Write(src[start..end].into()),
        HighlightEvent::HighlightStart(s) => TSEvent::Enter(CAPTURE_NAMES[s.0].into()),
        HighlightEvent::HighlightEnd => TSEvent::Close,
    }
}

pub struct TreeSitter;

impl SyntaxHighlighterAdapter for TreeSitter {
    fn write_highlighted(
        &self,
        output: &mut dyn Write,
        lang: Option<&str>,
        code: &str,
    ) -> std::fmt::Result {
        let lang = lang.unwrap_or("text");
        let html = highlight(lang, code).render().into_inner();
        write!(output, "{}", html)?;

        Ok(())
    }

    fn write_pre_tag(
        &self,
        _output: &mut dyn std::fmt::Write,
        _attributes: HashMap<&str, Cow<str>>,
    ) -> std::fmt::Result {
        Ok(())
    }

    fn write_code_tag(
        &self,
        _output: &mut dyn std::fmt::Write,
        _attributes: HashMap<&str, Cow<str>>,
    ) -> std::fmt::Result {
        Ok(())
    }
}
