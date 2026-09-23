//! The export formatter (PLAN.md § Exporting words), held byte for byte to
//! contract-fixtures/export/ with the TypeScript one
//! (frontend/src/lib/export/format.ts). It produces the file in chunks so the
//! endpoint can stream a 300,000-question list.

use std::collections::HashMap;

use serde::Deserialize;

use crate::leave::leave_value_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuizType {
    Anagram,
    Definition,
    LeaveValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    Correct,
    Missed,
}

impl Grade {
    pub fn as_str(self) -> &'static str {
        match self {
            Grade::Correct => "correct",
            Grade::Missed => "missed",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Word {
    pub word: String,
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default)]
    pub front_hooks: Option<String>,
    #[serde(default)]
    pub back_hooks: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Question {
    pub idx: i32,
    pub key: String,
    #[serde(default)]
    pub words: Option<Vec<Word>>,
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub front_hooks: Option<String>,
    #[serde(default)]
    pub back_hooks: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Quiz {
    pub level: i32,
    pub active: bool,
    /// Question indexes in study order.
    pub order: Vec<i32>,
    pub grades: HashMap<String, Grade>,
    /// Which quiz a quiz export names when two share a level.
    #[serde(default)]
    pub pick: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Which {
    All,
    Correct,
    Missed,
    Ungraded,
}

impl Which {
    pub fn as_str(self) -> &'static str {
        match self {
            Which::All => "all",
            Which::Correct => "correct",
            Which::Missed => "missed",
            Which::Ungraded => "ungraded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Question,
    Answer,
    Definition,
    Hooks,
    Grade,
}

impl Column {
    pub fn as_str(self) -> &'static str {
        match self {
            Column::Question => "question",
            Column::Answer => "answer",
            Column::Definition => "definition",
            Column::Hooks => "hooks",
            Column::Grade => "grade",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choices {
    pub scope: Scope,
    #[serde(default)]
    pub level: Option<i32>,
    pub which: Which,
    pub format: Format,
    #[serde(default)]
    pub lines: Option<Lines>,
    #[serde(default)]
    pub columns: Option<Vec<Column>>,
    pub order: Order,
    pub decimals: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Cascade,
    Quiz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Txt,
    Csv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lines {
    Answers,
    Questions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Study,
    Alphabetical,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportInput {
    pub name: String,
    pub quiz_type: QuizType,
    /// The distribution's tiles in tile order.
    pub tiles: Vec<String>,
    /// In search order.
    pub questions: Vec<Question>,
    pub quizzes: Vec<Quiz>,
    pub choices: Choices,
}

/// MAGPIE notation: a multi-character tile is written in brackets.
pub fn split_tiles(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            let end = rest.find(']').map_or(rest.len(), |e| e + 1);
            out.push(&rest[..end]);
            rest = &rest[end..];
        } else {
            out.push(&rest[..c.len_utf8()]);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

fn tile_key(key: &str, tiles: &[String]) -> Vec<usize> {
    split_tiles(key)
        .into_iter()
        .map(|t| {
            let t = t.strip_prefix('[').and_then(|t| t.strip_suffix(']')).unwrap_or(t);
            tiles.iter().position(|x| x == t).unwrap_or(usize::MAX)
        })
        .collect()
}

/// For a quiz, its grades; for a cascade, the union over its active quizzes (missed wins).
fn grades_for(input: &ExportInput) -> (HashMap<i32, Grade>, Option<&Quiz>) {
    let ch = &input.choices;
    let parse = |g: &HashMap<String, Grade>| g.iter().filter_map(|(k, v)| k.parse().ok().map(|k| (k, *v))).collect();
    if ch.scope == Scope::Quiz {
        let quiz = input.quizzes.iter().find(|q| Some(q.level) == ch.level && q.pick != Some(false));
        return (quiz.map(|q| parse(&q.grades)).unwrap_or_default(), quiz);
    }
    let mut out: HashMap<i32, Grade> = HashMap::new();
    for q in input.quizzes.iter().filter(|q| q.active) {
        for (k, g) in parse(&q.grades) {
            let merged = if g == Grade::Missed || out.get(&k) == Some(&Grade::Missed) { Grade::Missed } else { Grade::Correct };
            out.insert(k, merged);
        }
    }
    (out, None)
}

/// The selected questions in file order, with their grades.
pub fn selection(input: &ExportInput) -> Vec<(&Question, Option<Grade>)> {
    let by_idx: HashMap<i32, &Question> = input.questions.iter().map(|q| (q.idx, q)).collect();
    let (grades, quiz) = grades_for(input);
    let idxs: Vec<i32> = match quiz {
        Some(q) => {
            let mut idxs = q.order.clone();
            if input.choices.order == Order::Alphabetical {
                idxs.sort_by_cached_key(|i| by_idx.get(i).map(|q| tile_key(&q.key, &input.tiles)).unwrap_or_default());
            }
            idxs
        }
        None => input.questions.iter().map(|q| q.idx).collect(),
    };
    let which = input.choices.which;
    idxs.into_iter()
        .filter_map(|i| {
            let q = by_idx.get(&i)?;
            let g = grades.get(&i).copied();
            let keep = match which {
                Which::All => true,
                Which::Ungraded => g.is_none(),
                Which::Correct => g == Some(Grade::Correct),
                Which::Missed => g == Some(Grade::Missed),
            };
            keep.then_some((*q, g))
        })
        .collect()
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn hooks_pair(front: Option<&str>, back: Option<&str>) -> String {
    format!(
        "{}|{}",
        split_tiles(front.unwrap_or("")).join(" "),
        split_tiles(back.unwrap_or("")).join(" ")
    )
}

fn cell(input: &ExportInput, q: &Question, g: Option<Grade>, col: Column) -> String {
    let t = input.quiz_type;
    let d = input.choices.decimals;
    let words = || q.words.as_deref().unwrap_or(&[]);
    match col {
        Column::Question => q.key.clone(),
        Column::Grade => g.map(|g| g.as_str().to_owned()).unwrap_or_default(),
        Column::Answer => match t {
            QuizType::Anagram => words().iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" "),
            QuizType::Definition => q.definition.clone().unwrap_or_default(),
            QuizType::LeaveValue => leave_value_text(q.value.unwrap_or(0.0), d, false),
        },
        Column::Definition => match t {
            QuizType::Anagram => words().iter().map(|w| w.definition.as_deref().unwrap_or("")).collect::<Vec<_>>().join(" | "),
            QuizType::Definition => q.definition.clone().unwrap_or_default(),
            QuizType::LeaveValue => String::new(),
        },
        Column::Hooks => match t {
            QuizType::Anagram => words()
                .iter()
                .map(|w| hooks_pair(w.front_hooks.as_deref(), w.back_hooks.as_deref()))
                .collect::<Vec<_>>()
                .join(" / "),
            QuizType::Definition => hooks_pair(q.front_hooks.as_deref(), q.back_hooks.as_deref()),
            QuizType::LeaveValue => String::new(),
        },
    }
}

/// The file in chunks of about `chunk` entries.
pub fn format_chunks(input: &ExportInput, chunk: usize) -> Vec<String> {
    let mut out = Vec::new();
    format_each(input, chunk, &mut |s| out.push(s));
    out
}

/// Collects lines and hands them on `chunk` at a time.
struct Chunker<'a> {
    buf: String,
    n: usize,
    chunk: usize,
    sink: &'a mut dyn FnMut(String),
}

impl Chunker<'_> {
    fn line(&mut self, s: &str, end: &str) {
        self.buf.push_str(s);
        self.buf.push_str(end);
        self.n += 1;
        if self.n >= self.chunk {
            (self.sink)(std::mem::take(&mut self.buf));
            self.n = 0;
        }
    }

    fn finish(self) {
        if !self.buf.is_empty() {
            (self.sink)(self.buf);
        }
    }
}

/// The file in chunks of about `chunk` entries, each handed to `sink` as it is made.
pub fn format_each(input: &ExportInput, chunk: usize, sink: &mut dyn FnMut(String)) {
    let ch = &input.choices;
    let mut out = Chunker { buf: String::new(), n: 0, chunk: chunk.max(1), sink };
    let entries = selection(input);
    match ch.format {
        Format::Txt => {
            for (q, _) in entries {
                if ch.lines == Some(Lines::Questions) {
                    out.line(&q.key, "\n");
                    continue;
                }
                match input.quiz_type {
                    QuizType::Anagram => {
                        for w in q.words.as_deref().unwrap_or(&[]) {
                            out.line(&w.word, "\n");
                        }
                    }
                    QuizType::Definition => out.line(q.definition.as_deref().unwrap_or(""), "\n"),
                    QuizType::LeaveValue => out.line(&leave_value_text(q.value.unwrap_or(0.0), ch.decimals, false), "\n"),
                }
            }
        }
        Format::Csv => {
            let cols = ch.columns.clone().unwrap_or_default();
            out.line(&cols.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(","), "\r\n");
            for (q, g) in entries {
                let row = cols.iter().map(|c| csv_field(&cell(input, q, g, *c))).collect::<Vec<_>>().join(",");
                out.line(&row, "\r\n");
            }
        }
    }
    out.finish();
}

/// `CSW24 7s - L2 missed.txt`: every Unicode scalar value outside A–Z, a–z,
/// 0–9, space, `.`, `_` and `-` becomes one `_`, cut to 100 before the extension.
pub fn export_filename(name: &str, scope: Scope, level: Option<i32>, which: Which, format: Format) -> String {
    let mut base = name.to_owned();
    if scope == Scope::Quiz {
        base.push_str(&format!(" - L{}", level.unwrap_or(1)));
    }
    if which != Which::All {
        base.push(' ');
        base.push_str(which.as_str());
    }
    let safe: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-') { c } else { '_' })
        .take(100)
        .collect();
    format!("{safe}.{}", match format {
        Format::Txt => "txt",
        Format::Csv => "csv",
    })
}

#[cfg(test)]
mod tests {
    //! PLAN.md § Unit tests: the export formatters against
    //! contract-fixtures/export/cases.json (an independent reference wrote it).
    use super::*;

    #[derive(Deserialize)]
    struct Case {
        case: String,
        #[serde(flatten)]
        input: ExportInput,
        expected: Expected,
    }

    #[derive(Deserialize)]
    struct Expected {
        filename: String,
        body: String,
    }

    #[test]
    fn every_fixture_case_byte_for_byte() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../contract-fixtures/export/cases.json");
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
        let cases: Vec<Case> = serde_json::from_value(v["cases"].clone()).unwrap();
        assert!(cases.len() > 300);
        for c in cases {
            let body: String = format_chunks(&c.input, 2).concat();
            assert_eq!(body, c.expected.body, "{}", c.case);
            let ch = &c.input.choices;
            assert_eq!(export_filename(&c.input.name, ch.scope, ch.level, ch.which, ch.format), c.expected.filename, "{}", c.case);
        }
    }
}
