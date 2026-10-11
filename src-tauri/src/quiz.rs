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

#[cfg(test)]
mod tests {
    use super::*;

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
