//! NPC dialogs: the `talk_proc` block at the end of `elements.data`.
//!
//! ```text
//! u32 count, then per dialog:
//!   u32 id_talk, namechar text[64], i32 num_window, then per window:
//!     u32 id, u32 id_parent (-1 for a root), i32 talk_text_len, namechar talk_text[len],
//!     i32 num_option, then per option: u32 id, namechar text[64], u32 param
//! ```
//!
//! An option's `id` is a child window, or a predefined function when its top
//! bit is set (`SERVICE_TYPE` in ExpTypes.h: sell, give task, exit, …). The
//! function's `param` is e.g. the task ID for task functions.

use serde::Serialize;

use super::decode::read_wstr;

/// `id_parent` of a root window.
pub const NO_PARENT: u32 = u32::MAX;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkOption {
    pub id: u32,
    pub text: String,
    pub param: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkWindow {
    pub id: u32,
    pub parent: u32,
    pub text: String,
    pub options: Vec<TalkOption>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Talk {
    pub id: u32,
    /// The dialog's prompt: "RootNode" in most, sometimes a title.
    pub text: String,
    pub windows: Vec<TalkWindow>,
    pub offset: usize,
    pub size: usize,
}

impl Talk {
    /// The dialog's first words: its prompt, else the root window's text.
    /// The official editor names most prompts "RootNode".
    pub fn title(&self) -> String {
        let prompt = self.text.trim();
        let text = if prompt.is_empty() || prompt == "RootNode" {
            self.windows.iter().find(|w| w.parent == NO_PARENT).or(self.windows.first()).map(|w| w.text.as_str()).unwrap_or("")
        } else {
            &self.text
        };
        text.split(['\r', '\n']).map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(80).collect()
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    p: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(|| format!("dialog data ends early at byte {}", self.p))?;
        let bytes = &self.data[self.p..end];
        self.p = end;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn count(&mut self, what: &str) -> Result<usize, String> {
        let at = self.p;
        let n = self.u32()? as i32;
        if !(0..=1_000_000).contains(&n) {
            return Err(format!("bad {what} count {n} at byte {at}"));
        }
        Ok(n as usize)
    }
    fn wstr(&mut self, chars: usize) -> Result<String, String> {
        Ok(read_wstr(self.take(chars.checked_mul(2).ok_or("text too long")?)?))
    }
}

/// Parses the dialog block starting at `offset` (its count).
pub fn parse(data: &[u8], offset: usize) -> Result<Vec<Talk>, String> {
    let mut c = Cursor { data, p: offset };
    let count = c.count("dialog")?;
    let mut talks = Vec::with_capacity(count);
    for _ in 0..count {
        let start = c.p;
        let id = c.u32()?;
        let text = c.wstr(64)?;
        let num_window = c.count("window")?;
        let mut windows = Vec::with_capacity(num_window);
        for _ in 0..num_window {
            let id = c.u32()?;
            let parent = c.u32()?;
            let len = c.count("text length")?;
            let text = c.wstr(len)?;
            let num_option = c.count("option")?;
            let options = (0..num_option)
                .map(|_| Ok(TalkOption { id: c.u32()?, text: c.wstr(64)?, param: c.u32()? }))
                .collect::<Result<_, String>>()?;
            windows.push(TalkWindow { id, parent, text, options });
        }
        talks.push(Talk { id, text, windows, offset: start, size: c.p - start });
    }
    Ok(talks)
}
