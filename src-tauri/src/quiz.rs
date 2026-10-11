//! zhuxian_question.data: the questions of gdeliveryd's daily quiz contest (gamesys.conf `contest_file`;
//! znet/gdeliveryd/questionlib.cpp `CQuestionLib::Load`, contestmanager.cpp).
//!
//! ```text
//! i32 count; count × { u8 correct answers (bit 0 = A … bit 5 = F); i32 n, n × u16 question;
//!                      i32 answers (≤ 6); answers × { i32 n, n × u16 answer } }
//! ```
//!
//! The server cuts the questions into sets of 30 in file order and drops the rest; with no full set gdeliveryd
//! exits ("Initialize contest data failed"). A contest (19:00 every day) asks the 30 questions of set
//! `day of the month % sets` (day when smaller than the set count), one every 25 s, 20 s to answer. An empty
//! question or answer is a null string when the server sends it (`GetLen(NULL)`); more than 6 answers overflow
//! `pAnswer[6]`. A player's answer must equal the correct-answer byte. Only gdeliveryd reads the file.

use std::collections::HashMap;
use std::path::Path;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

pub const SET: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// Correct answers: bit 0 = A … bit 5 = F.
    pub key: u8,
    pub text: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedQuiz {
    pub path: String,
    pub size: usize,
    pub token: String,
    pub questions: Vec<Question>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize, what: &str) -> Result<&[u8], String> {
        let end = self.at.checked_add(len).filter(|end| *end <= self.data.len()).ok_or_else(|| format!("{what} runs past the end of the file (offset {})", self.at))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn i32(&mut self, what: &str) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn text(&mut self, what: &str) -> Result<String, String> {
        let units = self.i32(what)?;
        if units < 0 || units as usize > (self.data.len() - self.at) / 2 {
            return Err(format!("{what}: length {units} does not fit the file"));
        }
        let bytes = self.take(units as usize * 2, what)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16(&units).map_err(|_| format!("{what} is not valid UTF-16"))
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Question>, String> {
    let mut reader = Reader { data, at: 0 };
    let count = reader.i32("the question count")?;
    if count < 0 || count as usize > data.len() / 9 {
        return Err(format!("{count} questions do not fit the file"));
    }
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let what = format!("question {}", index + 1);
        let key = reader.take(1, &what)?[0];
        let text = reader.text(&what)?;
        let answers = reader.i32(&what)?;
        if !(0..=1000).contains(&answers) {
            return Err(format!("{what}: {answers} answers"));
        }
        let answers = (0..answers).map(|_| reader.text(&what)).collect::<Result<Vec<_>, String>>()?;
        out.push(Question { key, text, answers });
    }
    if reader.at != data.len() {
        return Err(format!("{} bytes follow the last question", data.len() - reader.at));
    }
    Ok(out)
}

pub fn encode(questions: &[Question]) -> Vec<u8> {
    let mut out = Vec::new();
    let text = |out: &mut Vec<u8>, value: &str| {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as i32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    };
    out.extend_from_slice(&(questions.len() as i32).to_le_bytes());
    for question in questions {
        out.push(question.key);
        text(&mut out, &question.text);
        out.extend_from_slice(&(question.answers.len() as i32).to_le_bytes());
        for answer in &question.answers {
            text(&mut out, answer);
        }
    }
    out
}

fn token(data: &[u8]) -> String {
    format!("{:x}", Md5::digest(data))
}

pub fn open(path: &Path) -> Result<OpenedQuiz, String> {
    let data = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let questions = parse(&data).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(OpenedQuiz { path: path.display().to_string(), size: data.len(), token: token(&data), questions })
}

/// The set a contest uses on a day of the month (1–31), as `BroadcastQuestion` picks it.
pub fn set_of_day(day: usize, sets: usize) -> Option<usize> {
    (sets > 0).then(|| if day >= sets { day % sets } else { day })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `error`: gdeliveryd does not start or may crash; `warning`: a question cannot be answered right; `note`.
    pub severity: &'static str,
    pub question: Option<usize>,
    pub message: String,
}

pub fn problems(questions: &[Question]) -> Vec<Problem> {
    let mut out = Vec::new();
    let sets = questions.len() / SET;
    if sets == 0 {
        out.push(Problem { severity: "error", question: None, message: format!("{} question{}: the contest needs at least {SET} (one set); gdeliveryd does not start", questions.len(), if questions.len() == 1 { "" } else { "s" }) });
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (index, question) in questions.iter().enumerate() {
        let mut add = |severity, message: String| out.push(Problem { severity, question: Some(index), message });
        if question.text.trim().is_empty() {
            add(if question.text.is_empty() { "error" } else { "warning" }, if question.text.is_empty() { "The question has no text: the server sends a null string (it may crash)".into() } else { "The question is only spaces".into() });
        }
        if question.answers.len() > 6 {
            add("error", format!("{} answers: the server holds 6 (it writes past its list)", question.answers.len()));
        }
        if question.answers.len() < 2 {
            add("warning", format!("{} answer{}: a question needs at least 2", question.answers.len(), if question.answers.len() == 1 { "" } else { "s" }));
        }
        if question.answers.iter().any(String::is_empty) {
            add("error", "An answer is empty: the server sends a null string (it may crash)".into());
        }
        let valid = if question.answers.len() >= 6 { 0x3f } else { (1u8 << question.answers.len()) - 1 };
        if question.key & valid == 0 {
            add("warning", "No answer is marked correct: nobody can answer it right".into());
        } else if question.key & !valid != 0 {
            add("warning", "An answer that does not exist is marked correct: nobody can answer it right".into());
        }
        if question.text.contains('\0') || question.answers.iter().any(|answer| answer.contains('\0')) {
            add("warning", "A text contains a zero character: the server cuts it there".into());
        }
        let key = question.text.trim();
        if !key.is_empty() {
            if let Some(&first) = seen.get(key) {
                add("note", format!("The same question as #{}", first + 1));
            } else {
                seen.insert(key, index);
            }
        }
    }
    let left = questions.len() % SET;
    if sets > 0 && left > 0 {
        out.push(Problem { severity: "note", question: Some(sets * SET), message: format!("The last {left} question{} make{} no full set of {SET}: the server never uses {}", if left == 1 { "" } else { "s" }, if left == 1 { "s" } else { "" }, if left == 1 { "it" } else { "them" }) });
    }
    if sets > 0 {
        let used: std::collections::HashSet<usize> = (1..=31).filter_map(|day| set_of_day(day, sets)).collect();
        let unused: Vec<String> = (0..sets).filter(|set| !used.contains(set)).map(|set| (set + 1).to_string()).collect();
        if !unused.is_empty() {
            out.push(Problem { severity: "note", question: None, message: format!("Set{} {} never run{} (the day of the month picks the set)", if unused.len() == 1 { "" } else { "s" }, unused.join(", "), if unused.len() == 1 { "s" } else { "" }) });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub token: String,
    pub backup: Option<String>,
}

pub fn save(path: &Path, token_now: Option<&str>, questions: &[Question], backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
    let data = encode(questions);
    if parse(&data)? != questions {
        return Err("The written file would not read back as these questions; nothing was saved".into());
    }
    if !replace_changed {
        if let (Some(expected), Ok(current)) = (token_now, std::fs::read(path)) {
            if token(&current) != expected {
                return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", path.display()));
            }
        }
    }
    let backup = if backup && path.is_file() { Some(crate::backup::archive(path, &[path.to_path_buf()])?.display().to_string()) } else { None };
    crate::path_data::write_replacing(path, &data)?;
    Ok(SaveReport { path: path.display().to_string(), size: data.len(), token: token(&data), backup })
}

/// The correct answers' letters ("B", "AC").
pub fn letters(key: u8) -> String {
    (0..8).filter(|bit| key & (1 << bit) != 0).map(|bit| (b'A' + bit) as char).collect()
}

/// Letters A–F in any case, with or without separators; empty = not given.
pub fn parse_letters(text: &str) -> Result<Option<u8>, String> {
    let mut key = 0u8;
    let mut any = false;
    for c in text.chars() {
        match c.to_ascii_uppercase() {
            letter @ 'A'..='F' => {
                key |= 1 << (letter as u8 - b'A');
                any = true;
            }
            ',' | ';' | '/' | '+' | '&' | ' ' | '\t' => {}
            _ => return Err(format!("“{text}” is not a list of answer letters A–F")),
        }
    }
    Ok(any.then_some(key))
}

/// A question of another file (zhuxian_question.data, JSON or CSV) by its number (1 = the first).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceQuestion {
    pub number: usize,
    /// None when the file does not say (an empty Correct column): the open file's answers stay.
    pub key: Option<u8>,
    pub text: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuizSource {
    pub path: String,
    /// `data`, `json` or `csv`.
    pub kind: &'static str,
    pub questions: Vec<SourceQuestion>,
    pub notes: Vec<String>,
}

pub const JSON_FORMAT: &str = "jdide-quiz";
const JSON_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonQuestion {
    #[serde(default)]
    number: Option<usize>,
    /// Letters, e.g. "B".
    #[serde(default)]
    correct: Option<String>,
    text: String,
    #[serde(default)]
    answers: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonExport<'a> {
    format: &'static str,
    format_version: u32,
    source_path: &'a str,
    exported_at: String,
    questions: Vec<JsonQuestion>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonEnvelope {
    format: String,
    format_version: u32,
    questions: Vec<JsonQuestion>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonInput {
    Envelope(JsonEnvelope),
    /// A plain list, for hand-made files.
    List(Vec<JsonQuestion>),
}

pub fn to_json(questions: &[Question], source_path: &str) -> String {
    let export = JsonExport {
        format: JSON_FORMAT,
        format_version: JSON_FORMAT_VERSION,
        source_path,
        exported_at: chrono::Local::now().to_rfc3339(),
        questions: questions.iter().enumerate().map(|(index, question)| JsonQuestion { number: Some(index + 1), correct: Some(letters(question.key)), text: question.text.clone(), answers: question.answers.clone() }).collect(),
    };
    serde_json::to_string_pretty(&export).expect("questions serialize")
}

pub fn from_json(text: &str) -> Result<Vec<SourceQuestion>, String> {
    let input: JsonInput = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|error| format!("Invalid quiz JSON: {error}"))?;
    let questions = match input {
        JsonInput::Envelope(envelope) => {
            if envelope.format != JSON_FORMAT {
                return Err(format!("This JSON is “{}”, not a JD IDE quiz export", envelope.format));
            }
            if envelope.format_version != JSON_FORMAT_VERSION {
                return Err(format!("Quiz JSON format version {} is not supported", envelope.format_version));
            }
            envelope.questions
        }
        JsonInput::List(questions) => questions,
    };
    let out = questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| {
            let key = match &question.correct {
                Some(correct) => parse_letters(correct).map_err(|error| format!("Question {}: {error}", question.number.unwrap_or(index + 1)))?,
                None => None,
            };
            Ok(SourceQuestion { number: question.number.unwrap_or(index + 1), key, text: question.text, answers: question.answers })
        })
        .collect::<Result<Vec<_>, String>>()?;
    check_numbers(&out)?;
    Ok(out)
}

fn check_numbers(questions: &[SourceQuestion]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for question in questions {
        if question.number == 0 {
            return Err("Question numbers start at 1".into());
        }
        if !seen.insert(question.number) {
            return Err(format!("Question {} is in the file twice", question.number));
        }
    }
    Ok(())
}

/// CSV for spreadsheets and translators: UTF-8 with a BOM (Excel), `#, Correct, Question, A … F`, texts quoted.
pub fn to_csv(questions: &[Question]) -> Vec<u8> {
    let columns = questions.iter().map(|question| question.answers.len()).max().unwrap_or(0).max(6);
    let quote = |text: &str| format!("\"{}\"", text.replace('"', "\"\""));
    let mut out = String::from("\u{feff}#,Correct,Question");
    for column in 0..columns {
        out.push(',');
        out.push_str(&answer_header(column));
    }
    out.push_str("\r\n");
    for (index, question) in questions.iter().enumerate() {
        out.push_str(&format!("{},{},{}", index + 1, letters(question.key), quote(&question.text)));
        for column in 0..columns {
            out.push(',');
            if let Some(answer) = question.answers.get(column) {
                out.push_str(&quote(answer));
            }
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

fn answer_header(column: usize) -> String {
    if column < 26 { ((b'A' + column as u8) as char).to_string() } else { format!("Answer {}", column + 1) }
}

/// Text of a CSV file: UTF-8 (with or without a BOM), UTF-16 with a BOM (Excel's "Unicode Text"), else GBK.
fn decode_text(bytes: &[u8], notes: &mut Vec<String>) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return encoding_rs::UTF_16LE.decode_without_bom_handling(rest).0.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return encoding_rs::UTF_16BE.decode_without_bom_handling(rest).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            notes.push("The file is not UTF-8: it was read as GBK (Chinese ANSI). Check the texts; saving the sheet as “CSV UTF-8” avoids this.".into());
            encoding_rs::GBK.decode_without_bom_handling(bytes).0.into_owned()
        }
    }
}

/// RFC 4180 records (quoted cells may hold delimiters, quotes and line breaks), with the line each starts on.
fn csv_records(text: &str, delimiter: char) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut line = 1;
    let mut start = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                if c == '\n' {
                    line += 1;
                }
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            c if c == delimiter => record.push(std::mem::take(&mut cell)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut cell));
                records.push((start, std::mem::take(&mut record)));
                line += 1;
                start = line;
            }
            c => cell.push(c),
        }
    }
    if quoted {
        return Err(format!("Line {start}: a quoted cell is not closed"));
    }
    if !cell.is_empty() || !record.is_empty() {
        record.push(cell);
        records.push((start, record));
    }
    Ok(records)
}

/// Reads a CSV made by `to_csv` or edited in a spreadsheet: the header names the columns (`#` optional, Correct
/// optional, Question, then one column per answer); comma, semicolon or tab separated.
pub fn from_csv(bytes: &[u8]) -> Result<(Vec<SourceQuestion>, Vec<String>), String> {
    let mut notes = Vec::new();
    let text = decode_text(bytes, &mut notes);
    let first = text.lines().next().unwrap_or("");
    let delimiter = [',', ';', '\t'].into_iter().max_by_key(|delimiter| first.matches(*delimiter).count()).filter(|delimiter| first.contains(*delimiter)).unwrap_or(',');
    let mut records = csv_records(&text, delimiter)?.into_iter().filter(|(_, cells)| cells.iter().any(|cell| !cell.trim().is_empty()));
    let (_, header) = records.next().ok_or("The file is empty")?;
    let (mut number, mut correct, mut question, mut answers) = (None, None, None, Vec::new());
    for (index, name) in header.iter().enumerate() {
        let name = name.trim().trim_start_matches('\u{feff}').to_lowercase();
        match name.as_str() {
            "#" | "no" | "no." | "nr" | "number" => number = Some(index),
            "correct" | "key" | "correct answer" | "correct answers" => correct = Some(index),
            "question" | "text" => question = Some(index),
            _ if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) || name.starts_with("answer") => answers.push(index),
            _ => {}
        }
    }
    let question = question.ok_or("The first row must name the columns: #, Correct, Question, A, B, … (as the CSV export writes them)")?;
    if answers.is_empty() {
        return Err("The header has no answer columns (A, B, …)".into());
    }
    let cell = |cells: &[String], index: usize| cells.get(index).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for (row, (line, cells)) in records.enumerate() {
        let number = match number.map(|index| cell(&cells, index)).filter(|value| !value.trim().is_empty()) {
            Some(value) => value.trim().parse::<usize>().map_err(|_| format!("Line {line}: “{value}” is not a question number"))?,
            None => row + 1,
        };
        let key = match correct {
            Some(index) => parse_letters(&cell(&cells, index)).map_err(|error| format!("Line {line}: {error}"))?,
            None => None,
        };
        let mut texts: Vec<String> = answers.iter().map(|index| cell(&cells, *index)).collect();
        while texts.last().is_some_and(|text| text.is_empty()) {
            texts.pop();
        }
        out.push(SourceQuestion { number, key, text: cell(&cells, question), answers: texts });
    }
    check_numbers(&out)?;
    if correct.is_none() {
        notes.push("No Correct column: the correct answers stay as they are.".into());
    }
    Ok((out, notes))
}

/// Another question file for comparing or importing: by extension JSON, CSV (also .tsv/.txt), else zhuxian_question.data.
pub fn read_source(path: &Path) -> Result<QuizSource, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    let fail = |error: String| format!("{}: {error}", path.display());
    let (kind, questions, notes) = match extension.as_str() {
        "json" => ("json", from_json(&String::from_utf8_lossy(&bytes)).map_err(fail)?, Vec::new()),
        "csv" | "tsv" | "txt" => {
            let (questions, notes) = from_csv(&bytes).map_err(fail)?;
            ("csv", questions, notes)
        }
        _ => {
            let questions = parse(&bytes).map_err(fail)?;
            ("data", questions.into_iter().enumerate().map(|(index, question)| SourceQuestion { number: index + 1, key: Some(question.key), text: question.text, answers: question.answers }).collect(), Vec::new())
        }
    };
    Ok(QuizSource { path: path.display().to_string(), kind, questions, notes })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub path: String,
    pub questions: usize,
}

/// Writes the questions as `json` or `csv`.
pub fn export(path: &Path, source_path: &str, questions: &[Question], format: &str) -> Result<ExportReport, String> {
    let data = match format {
        "json" => to_json(questions, source_path).into_bytes(),
        "csv" => to_csv(questions),
        other => return Err(format!("Unknown export format “{other}”")),
    };
    crate::path_data::write_replacing(path, &data)?;
    Ok(ExportReport { path: path.display().to_string(), questions: questions.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sourced(questions: &[Question]) -> Vec<SourceQuestion> {
        questions.iter().enumerate().map(|(index, question)| SourceQuestion { number: index + 1, key: Some(question.key), text: question.text.clone(), answers: question.answers.clone() }).collect()
    }

    #[test]
    fn samples_survive_json_and_csv() {
        for path in samples() {
            let Ok(data) = std::fs::read(path) else { continue };
            let questions = parse(&data).unwrap();
            assert_eq!(from_json(&to_json(&questions, path)).unwrap(), sourced(&questions), "{path} JSON");
            let (back, notes) = from_csv(&to_csv(&questions)).unwrap();
            assert_eq!(back, sourced(&questions), "{path} CSV");
            assert!(notes.is_empty());
        }
    }

    #[test]
    fn csv_from_spreadsheets() {
        // Semicolons, no # column, a quoted line break and quotes, lower-case letters, an empty Correct cell.
        let text = "Question;correct;A;B;C\r\n\"Say \"\"hi\"\"\nplease\";b;x;y;\r\nSecond;;1;2;3\r\n;;;;\r\n";
        let (questions, notes) = from_csv(text.as_bytes()).unwrap();
        assert!(notes.is_empty());
        assert_eq!(questions[0], SourceQuestion { number: 1, key: Some(2), text: "Say \"hi\"\nplease".into(), answers: vec!["x".into(), "y".into()] });
        assert_eq!(questions[1].key, None);
        assert_eq!(questions[1].answers.len(), 3);
        assert_eq!(questions.len(), 2);
        // Excel's "Unicode Text": UTF-16LE with a BOM, tab separated.
        let mut utf16 = vec![0xff, 0xfe];
        for unit in "#\tCorrect\tQuestion\tA\tB\r\n7\tA, C\t问题\t甲\t乙\r\n".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let (questions, _) = from_csv(&utf16).unwrap();
        assert_eq!(questions[0], SourceQuestion { number: 7, key: Some(5), text: "问题".into(), answers: vec!["甲".into(), "乙".into()] });
        // ANSI (GBK) from a Chinese Excel.
        let (gbk, _, _) = encoding_rs::GBK.encode("Question,A,B\r\n问题,甲,乙\r\n");
        let (questions, notes) = from_csv(&gbk).unwrap();
        assert_eq!(questions[0].text, "问题");
        assert_eq!(notes.len(), 2);
        assert!(from_csv(b"#,Question,A\r\n1,x,y\r\n1,z,w\r\n").unwrap_err().contains("twice"));
        assert!(from_csv(b"Question,Correct,A\r\nx,G,y\r\n").unwrap_err().starts_with("Line 2"));
        assert!(from_csv(b"Foo,Bar\r\n1,2\r\n").is_err());
    }

    fn samples() -> Vec<&'static str> {
        vec![
            "E:/Game Dev/JD/zxserver/znet/gdeliveryd/zhuxian_question.data",
            "E:/Game Dev/JD/1559/gdeliveryd/zhuxian_question.data",
            "E:/Games/Elite Jade Dynasty - HDN/element/data/zhuxian_question.data",
            "E:/Game Dev/JD/zxserver/zgame/gs/config/zhuxian_questions.data",
        ]
    }

    #[test]
    fn samples_read_back_byte_for_byte() {
        for path in samples() {
            let Ok(data) = std::fs::read(path) else { continue };
            let questions = parse(&data).unwrap();
            assert_eq!(encode(&questions), data, "{path}");
            let found = problems(&questions);
            let errors = found.iter().filter(|problem| problem.severity == "error").count();
            println!("QUIZ {path}: {} questions, {} sets, {} problems, {errors} errors", questions.len(), questions.len() / SET, found.len());
            if path.ends_with("zhuxian_question.data") {
                assert_eq!(errors, 0, "the contest files load");
            }
        }
    }

    #[test]
    fn days_pick_sets_like_the_server() {
        assert_eq!(set_of_day(5, 31), Some(5));
        assert_eq!(set_of_day(31, 31), Some(0));
        assert_eq!(set_of_day(25, 24), Some(1));
        assert_eq!(set_of_day(3, 0), None);
        let question = Question { key: 1, text: "Q".into(), answers: vec!["a".into(), "b".into()] };
        let found = problems(&vec![question.clone(); 29]);
        assert!(found.iter().any(|problem| problem.severity == "error" && problem.question.is_none()));
        let mut broken = vec![question; 30];
        broken[1].answers[0].clear();
        broken[2].key = 4;
        let found = problems(&broken);
        assert!(found.iter().any(|problem| problem.question == Some(1) && problem.severity == "error"));
        assert!(found.iter().any(|problem| problem.question == Some(2) && problem.severity == "warning"));
    }
}
