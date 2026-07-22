//! Minimal unified-diff engine replacing `patch -pN --batch -N`.
//!
//! Hunks are located like GNU patch does: at the expected line first, then at growing
//! offsets around it, and finally ignoring up to [`MAX_FUZZ`] lines of leading and
//! trailing context. A hunk whose post-image is already present is reported as
//! already applied instead of failing, which makes re-running idempotent.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, IoContext, Result};

pub const MAX_FUZZ: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Context(String),
    Remove(String),
    Add(String),
}

#[derive(Debug, Clone)]
pub struct Hunk {
    old_start: usize,
    lines: Vec<Line>,
}

#[derive(Debug, Clone)]
pub struct FilePatch {
    pub old_path: String,
    pub new_path: String,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parses every file section of a unified diff. Text outside sections (mail headers,
/// `diff --git` lines) is ignored.
pub fn parse(text: &str) -> Result<Vec<FilePatch>, ParseError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (Some(old), Some(new)) =
            (lines[i].strip_prefix("--- "), lines.get(i + 1).and_then(|l| l.strip_prefix("+++ ")))
        else {
            i += 1;
            continue;
        };
        let mut fp = FilePatch { old_path: path_field(old), new_path: path_field(new), hunks: Vec::new() };
        i += 2;
        while let Some(header) = lines.get(i).and_then(|l| l.strip_prefix("@@ ")) {
            let (old_start, mut old_len, mut new_len) =
                parse_range(header).ok_or_else(|| ParseError(format!("bad hunk header at line {}", i + 1)))?;
            i += 1;
            let mut hunk = Hunk { old_start, lines: Vec::new() };
            while old_len > 0 || new_len > 0 {
                let Some(&raw) = lines.get(i) else {
                    return Err(ParseError(format!("truncated hunk in {}", fp.new_path)));
                };
                i += 1;
                let (tag, body) = raw.split_at(raw.len().min(1));
                let line = match tag {
                    " " | "" => Line::Context(body.to_string()),
                    "-" => Line::Remove(body.to_string()),
                    "+" => Line::Add(body.to_string()),
                    "\\" => continue, // "\ No newline at end of file"
                    _ => return Err(ParseError(format!("unexpected line {i} in hunk: {raw:?}"))),
                };
                match line {
                    Line::Context(_) => (old_len, new_len) = (old_len.saturating_sub(1), new_len.saturating_sub(1)),
                    Line::Remove(_) => old_len = old_len.saturating_sub(1),
                    Line::Add(_) => new_len = new_len.saturating_sub(1),
                }
                hunk.lines.push(line);
            }
            if lines.get(i).is_some_and(|l| l.starts_with('\\')) {
                i += 1;
            }
            fp.hunks.push(hunk);
        }
        if fp.hunks.is_empty() {
            return Err(ParseError(format!("no hunks for {}", fp.new_path)));
        }
        files.push(fp);
    }
    if files.is_empty() {
        return Err(ParseError("no file sections found".into()));
    }
    Ok(files)
}

fn path_field(s: &str) -> String {
    s.split('\t').next().unwrap_or(s).trim().to_string()
}

/// `-a,b +c,d @@` -> (a, b, d); a missing length means 1.
fn parse_range(header: &str) -> Option<(usize, usize, usize)> {
    let mut parts = header.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let split = |r: &str| -> Option<(usize, usize)> {
        match r.split_once(',') {
            Some((s, l)) => Some((s.parse().ok()?, l.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let (old_start, old_len) = split(old)?;
    let (_, new_len) = split(new)?;
    Some((old_start, old_len, new_len))
}

impl Hunk {
    fn image(&self, reverse: bool) -> (Vec<&str>, Vec<&str>) {
        let (mut before, mut after) = (Vec::new(), Vec::new());
        for l in &self.lines {
            match l {
                Line::Context(s) => {
                    before.push(s.as_str());
                    after.push(s.as_str());
                }
                Line::Remove(s) => before.push(s.as_str()),
                Line::Add(s) => after.push(s.as_str()),
            }
        }
        if reverse { (after, before) } else { (before, after) }
    }

    fn leading_context(&self) -> usize {
        self.lines.iter().take_while(|l| matches!(l, Line::Context(_))).count()
    }

    fn trailing_context(&self) -> usize {
        self.lines.iter().rev().take_while(|l| matches!(l, Line::Context(_))).count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkResult {
    Applied { offset: isize, fuzz: usize },
    AlreadyApplied,
    Failed,
}

#[derive(Debug)]
pub struct Outcome {
    pub content: String,
    pub hunks: Vec<HunkResult>,
}

impl Outcome {
    pub fn failed(&self) -> bool {
        self.hunks.contains(&HunkResult::Failed)
    }

    pub fn fully_already_applied(&self) -> bool {
        self.hunks.iter().all(|h| *h == HunkResult::AlreadyApplied)
    }
}

/// Applies the hunks of one file to `original`. Never fails outright: each hunk gets
/// a [`HunkResult`] and the caller decides what a partial result means.
pub fn apply(fp: &FilePatch, original: &str) -> Outcome {
    let trailing_newline = original.ends_with('\n') || original.is_empty();
    let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
    // Maps line numbers of the original file to the current buffer.
    let mut offset: isize = 0;
    // Hunks must land in order and not overlap previously applied ones.
    let mut floor = 0usize;
    let mut results = Vec::with_capacity(fp.hunks.len());

    for hunk in &fp.hunks {
        let (lead, trail) = (hunk.leading_context(), hunk.trailing_context());
        let forward = |fuzz: usize| {
            let (pre, post) = (fuzz.min(lead), fuzz.min(trail));
            locate(&lines, hunk, false, pre, post, offset, floor).map(|pos| (pos, fuzz, pre, post))
        };
        // Exact forward match first, then "already applied" (exact post-image), and
        // only then fuzz: otherwise fuzz could re-apply a hunk that is already there.
        let exact = forward(0);
        if exact.is_none() {
            if let Some(pos) = locate(&lines, hunk, true, 0, 0, offset, floor) {
                let (_, after) = hunk.image(false);
                results.push(HunkResult::AlreadyApplied);
                offset = pos as isize - hunk.old_start.saturating_sub(1) as isize;
                floor = pos + after.len();
                continue;
            }
        }
        if let Some((pos, fuzz, pre, post)) = exact.or_else(|| (1..=MAX_FUZZ).find_map(forward)) {
            let (before, after) = hunk.image(false);
            let removed = before.len() - pre - post;
            let inserted: Vec<String> = after[pre..after.len() - post].iter().map(|s| s.to_string()).collect();
            let added = inserted.len();
            let expected = hunk.old_start.saturating_sub(1) + pre;
            lines.splice(pos..pos + removed, inserted);
            results.push(HunkResult::Applied { offset: pos as isize - (expected as isize + offset), fuzz });
            offset = pos as isize - expected as isize + added as isize - removed as isize;
            floor = pos + added;
            continue;
        }
        results.push(HunkResult::Failed);
    }

    let mut content = lines.join("\n");
    if trailing_newline && !content.is_empty() {
        content.push('\n');
    }
    Outcome { content, hunks: results }
}

/// Finds where the (trimmed) pre-image of `hunk` matches, searching outward from
/// the expected position. Returns the index of the first matched line.
fn locate(
    lines: &[String],
    hunk: &Hunk,
    reverse: bool,
    pre: usize,
    post: usize,
    offset: isize,
    floor: usize,
) -> Option<usize> {
    let (before, _) = hunk.image(reverse);
    let pattern = &before[pre..before.len() - post];
    if pattern.len() > lines.len() {
        return None;
    }
    let last = lines.len() - pattern.len();
    if floor > last {
        return None;
    }
    let expected = (hunk.old_start.saturating_sub(1) + pre) as isize + offset;
    let expected = expected.clamp(floor as isize, last as isize) as usize;
    let matches = |pos: usize| pattern.iter().zip(&lines[pos..]).all(|(p, l)| *p == l.as_str());
    let max_dist = (expected - floor).max(last - expected);
    (0..=max_dist).find_map(|d| {
        let after = expected + d;
        if after <= last && matches(after) {
            return Some(after);
        }
        let before = expected.checked_sub(d).filter(|&b| b >= floor && d > 0)?;
        matches(before).then_some(before)
    })
}

/// Picks the path of `fp` relative to `dir` by stripping leading components, trying
/// the strip levels the shell script used (3, 1, 0) before the remaining ones.
pub fn resolve_target(fp: &FilePatch, dir: &Path) -> Option<PathBuf> {
    let candidates = [&fp.new_path, &fp.old_path];
    let depth = candidates.iter().map(|p| p.split('/').count()).max().unwrap_or(0);
    let order = [3, 1, 0].into_iter().chain(2..depth);
    for strip in order {
        for path in candidates {
            if path == "/dev/null" {
                continue;
            }
            let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if strip >= parts.len() || parts[strip..].iter().any(|p| *p == ".." || *p == ".") {
                continue;
            }
            let rel: PathBuf = parts[strip..].iter().collect();
            if dir.join(&rel).is_file() {
                return Some(rel);
            }
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
pub enum FileStatus {
    Applied,
    AlreadyApplied,
}

/// Applies every file section of `patch_text` inside `dir`. Files are only written
/// when all their hunks either applied or were already present.
pub fn apply_in_dir(patch_text: &str, dir: &Path, name: &str) -> Result<FileStatus> {
    let files = parse(patch_text).map_err(|e| Error::Patch(format!("{name}: {e}")))?;
    let mut all_present = true;
    let mut pending = Vec::new();
    for fp in &files {
        let rel =
            resolve_target(fp, dir).ok_or_else(|| Error::Patch(format!("{name}: target {} not found", fp.new_path)))?;
        let path = dir.join(&rel);
        let original = fs::read_to_string(&path).ctx(|| format!("reading {}", path.display()))?;
        let outcome = apply(fp, &original);
        if outcome.failed() {
            let failed: Vec<String> = outcome
                .hunks
                .iter()
                .enumerate()
                .filter(|(_, h)| **h == HunkResult::Failed)
                .map(|(i, _)| (i + 1).to_string())
                .collect();
            return Err(Error::Patch(format!("{name}: hunk(s) #{} FAILED on {}", failed.join(", #"), rel.display())));
        }
        for (i, h) in outcome.hunks.iter().enumerate() {
            match h {
                HunkResult::Applied { offset: 0, fuzz: 0 } => {}
                HunkResult::Applied { offset, fuzz } => {
                    println!("Hunk #{} succeeded on {} (offset {offset} lines, fuzz {fuzz}).", i + 1, rel.display())
                }
                HunkResult::AlreadyApplied => println!("Hunk #{} already present in {}.", i + 1, rel.display()),
                HunkResult::Failed => unreachable!(),
            }
        }
        all_present &= outcome.fully_already_applied();
        pending.push((path, outcome.content));
    }
    for (path, content) in pending {
        write_atomic(&path, &content)?;
    }
    Ok(if all_present { FileStatus::AlreadyApplied } else { FileStatus::Applied })
}

pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().map(|e| e.to_string_lossy()).unwrap_or_default()));
    fs::write(&tmp, content).ctx(|| format!("writing {}", tmp.display()))?;
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    fs::rename(&tmp, path).ctx(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
--- a/dir/f.c
+++ b/dir/f.c
@@ -2,3 +2,4 @@ header
 b
 c
+X
 d
@@ -8,3 +9,3 @@
 h
-i
+I
 j
";

    fn file(n: usize) -> String {
        (0..n).map(|i| format!("{}\n", (b'a' + i as u8) as char)).collect()
    }

    #[test]
    fn parses_sections_and_hunks() {
        let files = parse(DIFF).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "b/dir/f.c");
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[0].hunks[0].old_start, 2);
    }

    #[test]
    fn applies_exactly() {
        let out = apply(&parse(DIFF).unwrap()[0], &file(12));
        assert!(!out.failed(), "{:?}", out.hunks);
        assert_eq!(out.content, "a\nb\nc\nX\nd\ne\nf\ng\nh\nI\nj\nk\nl\n");
        assert!(matches!(out.hunks[0], HunkResult::Applied { offset: 0, fuzz: 0 }));
    }

    #[test]
    fn applies_with_offset() {
        let shifted = format!("0\n1\n2\n{}", file(12));
        let out = apply(&parse(DIFF).unwrap()[0], &shifted);
        assert!(!out.failed());
        assert!(matches!(out.hunks[0], HunkResult::Applied { offset: 3, fuzz: 0 }));
        assert!(matches!(out.hunks[1], HunkResult::Applied { offset: 0, fuzz: 0 }), "{:?}", out.hunks);
        assert!(out.content.contains("c\nX\nd\n") && out.content.contains("h\nI\nj\n"));
    }

    #[test]
    fn applies_with_fuzz_when_outer_context_differs() {
        let changed = file(12).replace("b\n", "B\n");
        let out = apply(&parse(DIFF).unwrap()[0], &changed);
        assert!(matches!(out.hunks[0], HunkResult::Applied { fuzz: 1, .. }), "{:?}", out.hunks);
        assert!(out.content.contains("B\nc\nX\nd\n"));
    }

    #[test]
    fn detects_already_applied_and_is_idempotent() {
        let fp = &parse(DIFF).unwrap()[0];
        let once = apply(fp, &file(12));
        let twice = apply(fp, &once.content);
        assert!(twice.fully_already_applied(), "{:?}", twice.hunks);
        assert_eq!(twice.content, once.content);
    }

    #[test]
    fn partially_present_patch_applies_the_rest() {
        let fp = &parse(DIFF).unwrap()[0];
        let half = file(12).replace("c\nd\n", "c\nX\nd\n");
        let out = apply(fp, &half);
        assert_eq!(out.hunks[0], HunkResult::AlreadyApplied);
        assert!(matches!(out.hunks[1], HunkResult::Applied { .. }));
        assert!(out.content.contains("h\nI\nj\n"));
    }

    #[test]
    fn reports_failure_when_context_is_missing() {
        let out = apply(&parse(DIFF).unwrap()[0], "x\ny\nz\n");
        assert!(out.failed());
    }

    #[test]
    fn resolves_strip_level_and_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.c"), "").unwrap();
        let fp = &parse(DIFF).unwrap()[0];
        // b/dir/f.c needs -p2 here.
        assert_eq!(resolve_target(fp, dir.path()), Some(PathBuf::from("f.c")));
        let evil =
            FilePatch { old_path: "a/../../etc/passwd".into(), new_path: "b/../../etc/passwd".into(), hunks: vec![] };
        assert_eq!(resolve_target(&evil, dir.path()), None);
    }

    #[test]
    fn embedded_patch_parses() {
        let files = parse(crate::context::PATCH).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "b/drivers/bluetooth/btusb.c");
        assert_eq!(files[0].hunks.len(), 3);
    }
}
