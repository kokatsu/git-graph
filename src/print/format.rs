//! Formatting of commits.

use chrono::{FixedOffset, Local, TimeZone};
use git2::{Commit, Time};
use lazy_static::lazy_static;
use std::fmt::Write;
use std::str::FromStr;
use textwrap::Options;

/// Style options for formatting commit hashes.
#[derive(Clone, Default)]
pub struct HashStyle {
    /// Foreground color (ANSI 256-color code)
    pub color: Option<u8>,
    /// Apply bold formatting
    pub bold: bool,
    /// Background color (ANSI 256-color code)
    pub background: Option<u8>,
}

/// Commit formatting options.
#[derive(Ord, PartialOrd, Eq, PartialEq)]
pub enum CommitFormat {
    OneLine,
    Short,
    Medium,
    Full,
    Format(String),
}

impl FromStr for CommitFormat {
    type Err = String;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "oneline" | "o" => Ok(CommitFormat::OneLine),
            "short" | "s" => Ok(CommitFormat::Short),
            "medium" | "m" => Ok(CommitFormat::Medium),
            "full" | "f" => Ok(CommitFormat::Full),
            str => Ok(CommitFormat::Format(str.to_string())),
        }
    }
}

const NEW_LINE: usize = 0;
const HASH: usize = 1;
const HASH_ABBREV: usize = 2;
const PARENT_HASHES: usize = 3;
const PARENT_HASHES_ABBREV: usize = 4;
const REFS: usize = 5;
const SUBJECT: usize = 6;
const AUTHOR: usize = 7;
const AUTHOR_EMAIL: usize = 8;
const AUTHOR_DATE: usize = 9;
const AUTHOR_DATE_SHORT: usize = 10;
const AUTHOR_DATE_RELATIVE: usize = 11;
const COMMITTER: usize = 12;
const COMMITTER_EMAIL: usize = 13;
const COMMITTER_DATE: usize = 14;
const COMMITTER_DATE_SHORT: usize = 15;
const COMMITTER_DATE_RELATIVE: usize = 16;
const BODY: usize = 17;
const BODY_RAW: usize = 18;

const MODE_SPACE: usize = 1;
const MODE_PLUS: usize = 2;
const MODE_MINUS: usize = 3;

lazy_static! {
    pub static ref PLACEHOLDERS: Vec<[String; 4]> = {
        let base = vec![
            "n", "H", "h", "P", "p", "d", "s", "an", "ae", "ad", "as", "ar", "cn", "ce", "cd",
            "cs", "cr", "b", "B",
        ];
        base.iter()
            .map(|b| {
                [
                    format!("%{}", b),
                    format!("% {}", b),
                    format!("%+{}", b),
                    format!("%-{}", b),
                ]
            })
            .collect()
    };
}

/// Format a commit for `CommitFormat::Format(String)`.
pub fn format_commit(
    format: &str,
    commit: &Commit,
    branches: String,
    wrapping: &Option<Options>,
    hash_style: Option<&HashStyle>,
) -> Result<Vec<String>, String> {
    // Find replacements
    let mut replacements = vec![];
    for (idx, arr) in PLACEHOLDERS.iter().enumerate() {
        let mut curr = 0;
        loop {
            let mut found = false;
            for (mode, str) in arr.iter().enumerate() {
                if let Some(start) = &format[curr..].find(str) {
                    replacements.push((curr + start, str.len(), idx, mode));
                    curr += start + str.len();
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
        }
    }
    replacements.sort_by_key(|p| p.0);

    // Generate formatted lines
    let mut lines = vec![];
    let mut out = String::new();
    let mut formatter = CommitFieldFormatter::new(
        commit, &branches, hash_style, wrapping, &mut out, &mut lines,
    );

    let mut curr = 0;
    for &(start, len, idx, mode) in &replacements {
        formatter.out.push_str(&format[curr..start]);
        formatter.format_field(idx, mode)?;
        curr = start + len;
    }
    formatter.out.push_str(&format[curr..]);
    formatter.finalize_tail();
    Ok(lines)
}

struct CommitFieldFormatter<'a> {
    commit: &'a Commit<'a>,
    branches: &'a str,
    hash_style: Option<&'a HashStyle>,
    wrapping: &'a Option<Options<'a>>,
    out: &'a mut String,
    lines: &'a mut Vec<String>,
}

impl<'a> CommitFieldFormatter<'a> {
    fn new(
        commit: &'a Commit,
        branches: &'a str,
        hash_style: Option<&'a HashStyle>,
        wrapping: &'a Option<Options>,
        out: &'a mut String,
        lines: &'a mut Vec<String>,
    ) -> Self {
        Self {
            commit,
            branches,
            hash_style,
            wrapping,
            out,
            lines,
        }
    }

    fn handle_mode(&mut self, mode: usize, field_has_content: bool) {
        match mode {
            MODE_SPACE => {
                if field_has_content {
                    self.out.push(' ');
                }
            }
            MODE_PLUS => {
                if field_has_content {
                    self.add_line();
                }
            }
            MODE_MINUS => {
                if !field_has_content {
                    *self.out = remove_empty_lines(self.lines, self.out.clone());
                }
            }
            _ => {}
        }
    }

    pub fn format_field(&mut self, idx: usize, mode: usize) -> Result<(), String> {
        match idx {
            NEW_LINE => self.add_line(),
            HASH => self.format_hash(mode),
            HASH_ABBREV => self.format_hash_abbrev(mode),
            PARENT_HASHES => self.format_parent_hashes(mode)?,
            PARENT_HASHES_ABBREV => self.format_parent_hashes_abbrev(mode)?,
            REFS => self.format_refs(mode),
            SUBJECT => self.format_subject(mode)?,
            AUTHOR => self.format_author(mode),
            AUTHOR_EMAIL => self.format_author_email(mode),
            AUTHOR_DATE => self.format_author_date(mode),
            AUTHOR_DATE_SHORT => self.format_author_date_short(mode),
            AUTHOR_DATE_RELATIVE => self.format_author_date_relative(mode),
            COMMITTER => self.format_committer(mode),
            COMMITTER_EMAIL => self.format_committer_email(mode),
            COMMITTER_DATE => self.format_committer_date(mode),
            COMMITTER_DATE_SHORT => self.format_committer_date_short(mode),
            COMMITTER_DATE_RELATIVE => self.format_committer_date_relative(mode),
            BODY => self.format_body(mode)?,
            BODY_RAW => self.format_body_raw(mode)?,
            x => return Err(format!("No commit field at index {}", x)),
        }
        Ok(())
    }

    fn add_line(&mut self) {
        if !self.out.is_empty() {
            add_line(self.lines, self.out, self.wrapping);
        }
    }

    pub fn finalize_tail(&mut self) {
        self.add_line();
    }

    fn format_hash(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        let id = self.commit.id().to_string();
        self.out.push_str(&apply_hash_style(&id, self.hash_style));
    }

    fn format_hash_abbrev(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        let id = self.commit.id().to_string();
        let s = &id[..7];
        self.out.push_str(&apply_hash_style(s, self.hash_style));
    }

    fn format_parent_hashes(&mut self, mode: usize) -> Result<(), String> {
        self.handle_mode(mode, true);
        for i in 0..self.commit.parent_count() {
            self.out
                .push_str(&self.commit.parent_id(i).unwrap().to_string());
            if i < self.commit.parent_count() - 1 {
                self.out.push(' ');
            }
        }
        Ok(())
    }

    fn format_parent_hashes_abbrev(&mut self, mode: usize) -> Result<(), String> {
        self.handle_mode(mode, true);
        for i in 0..self.commit.parent_count() {
            let parent_id = self.commit.parent_id(i).map_err(|err| err.to_string())?;
            self.out.push_str(&parent_id.to_string()[..7]);
            if i < self.commit.parent_count() - 1 {
                self.out.push(' ');
            }
        }
        Ok(())
    }

    fn format_refs(&mut self, mode: usize) {
        self.handle_mode(mode, !self.branches.is_empty());
        self.out.push_str(self.branches);
    }

    fn format_subject(&mut self, mode: usize) -> Result<(), String> {
        let summary = self.commit.summary().unwrap_or("");
        self.handle_mode(mode, !summary.is_empty());
        self.out.push_str(summary);
        Ok(())
    }

    fn format_author(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out.push_str(self.commit.author().name().unwrap_or(""));
    }

    fn format_author_email(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(self.commit.author().email().unwrap_or(""));
    }

    fn format_author_date(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out.push_str(&format_date(
            self.commit.author().when(),
            "%a %b %e %H:%M:%S %Y %z",
        ));
    }

    fn format_author_date_short(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(&format_date(self.commit.author().when(), "%F"));
    }

    fn format_author_date_relative(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(&format_relative_time(self.commit.author().when()));
    }

    fn format_committer(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(self.commit.committer().name().unwrap_or(""));
    }

    fn format_committer_email(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(self.commit.committer().email().unwrap_or(""));
    }

    fn format_committer_date(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out.push_str(&format_date(
            self.commit.committer().when(),
            "%a %b %e %H:%M:%S %Y %z",
        ));
    }

    fn format_committer_date_short(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(&format_date(self.commit.committer().when(), "%F"));
    }

    fn format_committer_date_relative(&mut self, mode: usize) {
        self.handle_mode(mode, true);
        self.out
            .push_str(&format_relative_time(self.commit.committer().when()));
    }

    fn format_body(&mut self, mode: usize) -> Result<(), String> {
        let message = self
            .commit
            .message()
            .unwrap_or("")
            .lines()
            .collect::<Vec<&str>>();
        let num_parts = message.len();
        self.handle_mode(mode, num_parts > 2);
        for (cnt, line) in message.iter().enumerate() {
            if cnt > 1 && (cnt < num_parts - 1 || !line.is_empty()) {
                self.out.push_str(line);
                self.add_line();
            }
        }
        Ok(())
    }

    fn format_body_raw(&mut self, mode: usize) -> Result<(), String> {
        let message = self
            .commit
            .message()
            .unwrap_or("")
            .lines()
            .collect::<Vec<&str>>();
        let num_parts = message.len();
        self.handle_mode(mode, !message.is_empty());
        for (cnt, line) in message.iter().enumerate() {
            if cnt < num_parts - 1 || !line.is_empty() {
                self.out.push_str(line);
                self.add_line();
            }
        }
        Ok(())
    }
}

/// Format a commit for `CommitFormat::OneLine`.
pub fn format_oneline(
    commit: &Commit,
    branches: String,
    wrapping: &Option<Options>,
    hash_style: Option<&HashStyle>,
) -> Vec<String> {
    let mut out = String::new();
    let id = commit.id().to_string();
    let hash_str = apply_hash_style(&id[..7], hash_style);
    write!(out, "{}", hash_str).unwrap();

    write!(out, "{} {}", branches, commit.summary().unwrap_or("")).unwrap();

    if let Some(wrap) = wrapping {
        let lines: Vec<String> = textwrap::fill(&out, wrap)
            .lines()
            .map(|s| s.to_string())
            .collect();
        fix_ansi_continuation(lines)
    } else {
        vec![out]
    }
}

/// Format a commit for `CommitFormat::Short`, `CommitFormat::Medium` or `CommitFormat::Full`.
pub fn format(
    commit: &Commit,
    branches: String,
    wrapping: &Option<Options>,
    hash_style: Option<&HashStyle>,
    format: &CommitFormat,
) -> Result<Vec<String>, String> {
    match format {
        CommitFormat::OneLine => return Ok(format_oneline(commit, branches, wrapping, hash_style)),
        CommitFormat::Format(format) => {
            return format_commit(format, commit, branches, wrapping, hash_style)
        }
        _ => {}
    }

    let mut out_vec = vec![];
    let mut out = String::new();

    let id = commit.id().to_string();
    let hash_str = apply_hash_style(&id, hash_style);
    write!(out, "commit {}", hash_str).map_err(|err| err.to_string())?;

    write!(out, "{}", branches).map_err(|err| err.to_string())?;
    append_wrapped(&mut out_vec, out, wrapping);

    if commit.parent_count() > 1 {
        out = String::new();
        write!(
            out,
            "Merge: {} {}",
            &commit.parent_id(0).unwrap().to_string()[..7],
            &commit.parent_id(1).unwrap().to_string()[..7]
        )
        .map_err(|err| err.to_string())?;
        append_wrapped(&mut out_vec, out, wrapping);
    }

    out = String::new();
    write!(
        out,
        "Author: {} <{}>",
        commit.author().name().unwrap_or(""),
        commit.author().email().unwrap_or("")
    )
    .map_err(|err| err.to_string())?;
    append_wrapped(&mut out_vec, out, wrapping);

    if format > &CommitFormat::Medium {
        out = String::new();
        write!(
            out,
            "Commit: {} <{}>",
            commit.committer().name().unwrap_or(""),
            commit.committer().email().unwrap_or("")
        )
        .map_err(|err| err.to_string())?;
        append_wrapped(&mut out_vec, out, wrapping);
    }

    if format > &CommitFormat::Short {
        out = String::new();
        write!(
            out,
            "Date:   {}",
            format_date(commit.author().when(), "%a %b %e %H:%M:%S %Y %z")
        )
        .map_err(|err| err.to_string())?;
        append_wrapped(&mut out_vec, out, wrapping);
    }

    if format == &CommitFormat::Short {
        out_vec.push("".to_string());
        append_wrapped(
            &mut out_vec,
            format!("    {}", commit.summary().unwrap_or("")),
            wrapping,
        );
        out_vec.push("".to_string());
    } else {
        out_vec.push("".to_string());
        let mut add_line = true;
        for line in commit.message().unwrap_or("").lines() {
            if line.is_empty() {
                out_vec.push(line.to_string());
            } else {
                append_wrapped(&mut out_vec, format!("    {}", line), wrapping);
            }
            add_line = !line.trim().is_empty();
        }
        if add_line {
            out_vec.push("".to_string());
        }
    }

    Ok(out_vec)
}

/// Apply styling (color, bold, background) to a hash string.
fn apply_hash_style(hash: &str, style: Option<&HashStyle>) -> String {
    match style {
        None => hash.to_string(),
        Some(s) => {
            let mut styled = yansi::Painted::new(hash);

            if let Some(color) = s.color {
                styled = styled.fixed(color);
            }
            if s.bold {
                styled = styled.bold();
            }
            if let Some(bg) = s.background {
                styled = styled.on_fixed(bg);
            }

            styled.to_string()
        }
    }
}

pub fn format_date(time: Time, format: &str) -> String {
    let offset = FixedOffset::east_opt(time.offset_minutes() * 60).expect("Invalid offset minutes");
    let date = offset
        .timestamp_opt(time.seconds(), 0)
        .single()
        .expect("Invalid timestamp, maybe a fold or gap in local time");
    date.format(format).to_string()
}

/// Format a time as a relative time string (e.g., "21 hours ago", "4 days ago")
pub fn format_relative_time(time: Time) -> String {
    let offset = FixedOffset::east_opt(time.offset_minutes() * 60).expect("Invalid offset minutes");
    let commit_time = Local::from_offset(&offset)
        .timestamp_opt(time.seconds(), 0)
        .single()
        .expect("Invalid timestamp");
    let now = Local::now();
    let duration = now.signed_duration_since(commit_time);

    let seconds = duration.num_seconds();
    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_hours() / 24;
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;

    if seconds < 60 {
        format!("{} seconds ago", seconds)
    } else if minutes < 60 {
        format!("{} minutes ago", minutes)
    } else if hours < 24 {
        format!("{} hours ago", hours)
    } else if days < 7 {
        format!("{} days ago", days)
    } else if weeks < 4 {
        format!("{} weeks ago", weeks)
    } else if months < 12 {
        format!("{} months ago", months)
    } else {
        format!("{} years ago", years)
    }
}

fn append_wrapped(vec: &mut Vec<String>, str: String, wrapping: &Option<Options>) {
    if str.is_empty() {
        vec.push(str);
    } else if let Some(wrap) = wrapping {
        let lines: Vec<String> = textwrap::fill(&str, wrap)
            .lines()
            .map(|s| s.to_string())
            .collect();
        vec.extend(fix_ansi_continuation(lines));
    } else {
        vec.push(str);
    }
}

fn add_line(lines: &mut Vec<String>, line: &mut String, wrapping: &Option<Options>) {
    let mut temp = String::new();
    std::mem::swap(&mut temp, line);
    append_wrapped(lines, temp, wrapping);
}

fn remove_empty_lines(lines: &mut Vec<String>, mut line: String) -> String {
    while !lines.is_empty() && lines.last().unwrap().is_empty() {
        line = lines.remove(lines.len() - 1);
    }
    if !lines.is_empty() {
        line = lines.remove(lines.len() - 1);
    }
    line
}

/// Fixes ANSI color code continuation across wrapped lines.
///
/// When textwrap splits a colored string, the second line loses the color
/// because the ANSI escape sequence only appears at the start of the colored
/// segment. This function tracks active ANSI codes and prepends them to
/// continuation lines.
fn fix_ansi_continuation(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= 1 {
        return lines;
    }

    let mut result = Vec::with_capacity(lines.len());
    let mut active_color: Option<String> = None;

    for (i, line) in lines.into_iter().enumerate() {
        let fixed_line = if i > 0 && active_color.is_some() {
            format!("{}{}", active_color.as_ref().unwrap(), line)
        } else {
            line
        };

        // Track the last active color code in this line
        active_color = find_active_ansi_code(&fixed_line);
        result.push(fixed_line);
    }
    result
}

/// Finds the last active (non-reset) ANSI color code in a string.
///
/// ANSI escape sequences have the format: ESC [ <params> m
/// where ESC is \x1b (0x1b). Reset codes like \x1b[0m or \x1b[m clear the color.
/// Returns None if no color is active at the end of the string.
fn find_active_ansi_code(s: &str) -> Option<String> {
    let mut last_code: Option<String> = None;
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for ESC character (0x1b)
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let start = i;
            i += 2; // Skip ESC and '['

            // Read until 'm' (end of ANSI sequence)
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }

            if i < bytes.len() {
                i += 1; // Include 'm'
                let code = &s[start..i];

                // Check if this is a reset code
                if code == "\x1b[0m" || code == "\x1b[m" {
                    last_code = None;
                } else {
                    last_code = Some(code.to_string());
                }
            }
        } else {
            i += 1;
        }
    }

    last_code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_active_ansi_code_no_codes() {
        assert_eq!(find_active_ansi_code("plain text"), None);
    }

    #[test]
    fn test_find_active_ansi_code_with_color() {
        // Yellow color code (fixed color 11)
        let s = "\x1b[38;5;11mhello";
        assert_eq!(find_active_ansi_code(s), Some("\x1b[38;5;11m".to_string()));
    }

    #[test]
    fn test_find_active_ansi_code_with_reset() {
        // Color followed by reset
        let s = "\x1b[38;5;11mhello\x1b[0m world";
        assert_eq!(find_active_ansi_code(s), None);
    }

    #[test]
    fn test_find_active_ansi_code_multiple_colors() {
        // Multiple colors - should return the last active one
        let s = "\x1b[38;5;11mhello\x1b[0m \x1b[38;5;14mworld";
        assert_eq!(find_active_ansi_code(s), Some("\x1b[38;5;14m".to_string()));
    }

    #[test]
    fn test_fix_ansi_continuation_single_line() {
        let lines = vec!["single line".to_string()];
        assert_eq!(fix_ansi_continuation(lines.clone()), lines);
    }

    #[test]
    fn test_fix_ansi_continuation_no_color() {
        let lines = vec!["line one".to_string(), "line two".to_string()];
        assert_eq!(fix_ansi_continuation(lines.clone()), lines);
    }

    #[test]
    fn test_fix_ansi_continuation_with_color() {
        // Simulates textwrap splitting a colored string
        // Original: "\x1b[38;5;11m[release-3.14.2-hotfix-03]\x1b[0m"
        // After wrap: line1 ends with color still active, line2 has no color
        let lines = vec![
            "\x1b[38;5;11m[release-3.14.2-hotfix-".to_string(),
            "03]\x1b[0m".to_string(),
        ];
        let fixed = fix_ansi_continuation(lines);
        assert_eq!(fixed.len(), 2);
        assert_eq!(fixed[0], "\x1b[38;5;11m[release-3.14.2-hotfix-");
        // Second line should have color prepended
        assert_eq!(fixed[1], "\x1b[38;5;11m03]\x1b[0m");
    }

    #[test]
    fn test_fix_ansi_continuation_color_reset_before_wrap() {
        // Color is reset before the line break - no continuation needed
        let lines = vec![
            "\x1b[38;5;11mhello\x1b[0m world".to_string(),
            "next line".to_string(),
        ];
        let fixed = fix_ansi_continuation(lines);
        assert_eq!(fixed[1], "next line");
    }
}
