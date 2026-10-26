//! RevuCraft's Markdown notes: the Markdown they are written in (a small, Obsidian-flavoured
//! subset: headings, bold, italic, strikethrough, `code`, ==highlight==, lists, `- [ ]`
//! checklists, quotes, code blocks, rules, web links, `[[links]]` to markups on the plan and
//! `#tags`), read into blocks and spans for the reading view, and the text edits the editor's
//! toolbar makes. Everything here works on the text alone; `notes_ui` draws it.

/// A note longer than this (characters) is read only this far.
const MAX_CHARS: usize = 200_000;

/// What a list item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Bullet,
    Number(u32),
    /// A checklist item, ticked or not.
    Task(bool),
}

/// One block of a note, with the line (0-based) it starts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, text: String },
    Paragraph(String),
    Item { indent: usize, kind: ItemKind, text: String, line: usize },
    Quote(String),
    Code(String),
    Rule,
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Web(String),
    /// `[[target]]`: a markup on the plan, by its name, label or mark.
    Markup(String),
}

/// A run of text and how it looks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub highlight: bool,
    pub tag: bool,
    pub link: Option<Link>,
}

fn clip(src: &str) -> &str {
    match src.char_indices().nth(MAX_CHARS) {
        Some((i, _)) => src.get(..i).unwrap_or(src),
        None => src,
    }
}

/// `#` to `######` and a space: the heading's level and text.
fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = line.get(hashes..)?;
        if rest.is_empty() || rest.starts_with(' ') {
            return Some((u8::try_from(hashes).ok()?, rest.trim()));
        }
    }
    None
}

/// A list item's indent (in levels), kind and text.
fn item(line: &str) -> Option<(usize, ItemKind, &str)> {
    let spaces: usize = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let trimmed = line.trim_start();
    let indent = spaces / 2;
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            for (mark, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                if let Some(text) = rest.strip_prefix(mark) {
                    return Some((indent, ItemKind::Task(done), text));
                }
            }
            if rest == "[ ]" || rest == "[x]" || rest == "[X]" {
                return Some((indent, ItemKind::Task(rest != "[ ]"), ""));
            }
            return Some((indent, ItemKind::Bullet, rest));
        }
    }
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() && digits.len() <= 9 {
        let rest = trimmed.get(digits.len()..)?;
        if let Some(text) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((indent, ItemKind::Number(digits.parse().unwrap_or(1)), text));
        }
    }
    None
}

fn rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// The note's blocks, in order.
pub fn blocks(src: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let mut quote: Vec<&str> = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let flush = |out: &mut Vec<Block>, para: &mut Vec<&str>, quote: &mut Vec<&str>| {
        if !para.is_empty() {
            out.push(Block::Paragraph(para.join(" ")));
            para.clear();
        }
        if !quote.is_empty() {
            out.push(Block::Quote(quote.join(" ")));
            quote.clear();
        }
    };
    for (n, line) in clip(src).lines().enumerate() {
        if let Some(lines) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                out.push(Block::Code(lines.join("\n")));
                code = None;
            } else {
                lines.push(line);
            }
            continue;
        }
        if line.trim_start().starts_with("```") {
            flush(&mut out, &mut para, &mut quote);
            code = Some(Vec::new());
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut out, &mut para, &mut quote);
            continue;
        }
        if let Some((level, text)) = heading(line) {
            flush(&mut out, &mut para, &mut quote);
            out.push(Block::Heading { level, text: text.to_string() });
        } else if let Some((indent, kind, text)) = item(line) {
            flush(&mut out, &mut para, &mut quote);
            out.push(Block::Item { indent, kind, text: text.to_string(), line: n });
        } else if rule(line) {
            flush(&mut out, &mut para, &mut quote);
            out.push(Block::Rule);
        } else if let Some(q) = line.trim_start().strip_prefix('>') {
            if !para.is_empty() {
                flush(&mut out, &mut para, &mut Vec::new());
            }
            quote.push(q.trim());
        } else {
            if !quote.is_empty() {
                flush(&mut out, &mut Vec::new(), &mut quote);
            }
            para.push(line.trim());
        }
    }
    if let Some(lines) = code {
        out.push(Block::Code(lines.join("\n")));
    }
    flush(&mut out, &mut para, &mut quote);
    out
}

fn tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// A line's text as styled runs.
pub fn spans(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span::default();
    let mut buf = String::new();
    let flush = |out: &mut Vec<Span>, cur: &Span, buf: &mut String| {
        if !buf.is_empty() {
            out.push(Span { text: std::mem::take(buf), ..cur.clone() });
        }
    };
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c));
    let mut i = 0;
    while i < chars.len() {
        let Some(&c) = chars.get(i) else { break };
        // Code: nothing inside it is read.
        if c == '`' {
            if let Some(end) = (i + 1..chars.len()).find(|&k| chars.get(k) == Some(&'`')) {
                flush(&mut out, &cur, &mut buf);
                let inner: String = chars.get(i + 1..end).map(|s| s.iter().collect()).unwrap_or_default();
                out.push(Span { text: inner, code: true, ..cur.clone() });
                i = end + 1;
                continue;
            }
        }
        // [[target]] or [[target|shown]].
        if at(i, "[[") {
            if let Some(end) = (i + 2..chars.len().saturating_sub(1)).find(|&k| at(k, "]]")) {
                flush(&mut out, &cur, &mut buf);
                let inner: String = chars.get(i + 2..end).map(|s| s.iter().collect()).unwrap_or_default();
                let (target, shown) = match inner.split_once('|') {
                    Some((t, s)) => (t.trim().to_string(), s.trim().to_string()),
                    None => (inner.trim().to_string(), inner.trim().to_string()),
                };
                out.push(Span { text: shown, link: Some(Link::Markup(target)), ..cur.clone() });
                i = end + 2;
                continue;
            }
        }
        // [shown](url).
        if c == '[' {
            if let Some(close) = (i + 1..chars.len()).find(|&k| chars.get(k) == Some(&']'))
                && chars.get(close + 1) == Some(&'(')
                && let Some(end) = (close + 2..chars.len()).find(|&k| chars.get(k) == Some(&')'))
            {
                flush(&mut out, &cur, &mut buf);
                let shown: String = chars.get(i + 1..close).map(|s| s.iter().collect()).unwrap_or_default();
                let url: String = chars.get(close + 2..end).map(|s| s.iter().collect()).unwrap_or_default();
                out.push(Span { text: shown, link: Some(Link::Web(url.trim().to_string())), ..cur.clone() });
                i = end + 1;
                continue;
            }
        }
        // #tag, at the start of a word.
        if c == '#' && (i == 0 || chars.get(i - 1).is_some_and(|p| p.is_whitespace() || *p == '(')) && chars.get(i + 1).is_some_and(|n| tag_char(*n))
        {
            let end = (i + 1..chars.len()).find(|&k| !chars.get(k).is_some_and(|c| tag_char(*c))).unwrap_or(chars.len());
            let word: String = chars.get(i..end).map(|s| s.iter().collect()).unwrap_or_default();
            if word.chars().skip(1).any(char::is_alphabetic) {
                flush(&mut out, &cur, &mut buf);
                out.push(Span { text: word, tag: true, ..cur.clone() });
                i = end;
                continue;
            }
        }
        let toggle = |flag: &mut bool, out: &mut Vec<Span>, cur: &Span, buf: &mut String| {
            flush(out, cur, buf);
            *flag = !*flag;
        };
        if at(i, "**") || at(i, "__") {
            let mut next = cur.clone();
            toggle(&mut next.bold, &mut out, &cur, &mut buf);
            cur = next;
            i += 2;
        } else if at(i, "~~") {
            let mut next = cur.clone();
            toggle(&mut next.strike, &mut out, &cur, &mut buf);
            cur = next;
            i += 2;
        } else if at(i, "==") {
            let mut next = cur.clone();
            toggle(&mut next.highlight, &mut out, &cur, &mut buf);
            cur = next;
            i += 2;
        } else if c == '*' || (c == '_' && (i == 0 || chars.get(i - 1).is_some_and(|p| !p.is_alphanumeric()) || cur.italic)) {
            let mut next = cur.clone();
            toggle(&mut next.italic, &mut out, &cur, &mut buf);
            cur = next;
            i += 1;
        } else {
            buf.push(c);
            i += 1;
        }
    }
    flush(&mut out, &cur, &mut buf);
    out
}

/// The note's `#tags`, each once, in order (not in code).
pub fn tags(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in blocks(src) {
        let text = match b {
            Block::Paragraph(t) | Block::Quote(t) => t,
            Block::Item { text, .. } | Block::Heading { text, .. } => text,
            Block::Code(_) | Block::Rule => continue,
        };
        for s in spans(&text) {
            if s.tag && !out.iter().any(|t| t.eq_ignore_ascii_case(&s.text)) {
                out.push(s.text);
            }
        }
    }
    out
}

/// The note's title: its first heading, else its first line, without the Markdown.
pub fn title(src: &str) -> String {
    let first = blocks(src).into_iter().find_map(|b| match b {
        Block::Heading { text, .. } | Block::Paragraph(text) | Block::Quote(text) => Some(text),
        Block::Item { text, .. } => Some(text),
        Block::Code(_) | Block::Rule => None,
    });
    let plain: String = first.map(|t| spans(&t).into_iter().map(|s| s.text).collect()).unwrap_or_default();
    plain.chars().take(80).collect()
}

/// Checklist items done, of all.
pub fn tasks(src: &str) -> (usize, usize) {
    blocks(src).iter().fold((0, 0), |(done, all), b| match b {
        Block::Item { kind: ItemKind::Task(d), .. } => (done + usize::from(*d), all + 1),
        _ => (done, all),
    })
}

/// Tick or untick the checklist item on `line` (0-based).
pub fn toggle_task(src: &str, line: usize) -> String {
    let mut out: Vec<String> = src.split('\n').map(str::to_string).collect();
    if let Some(l) = out.get_mut(line) {
        if let Some(i) = l.find("[ ]") {
            l.replace_range(i..i + 3, "[x]");
        } else if let Some(i) = l.find("[x]").or_else(|| l.find("[X]")) {
            l.replace_range(i..i + 3, "[ ]");
        }
    }
    out.join("\n")
}

// ── The editor's edits: on text and a selection (in characters) ────────────────────────────

/// Text and its selection (character offsets, `start <= end`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

fn chars_of(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn ordered(len: usize, a: usize, b: usize) -> (usize, usize) {
    (a.min(b).min(len), a.max(b).min(len))
}

/// The inline markers the toolbar puts round text.
pub const BOLD: &str = "**";
pub const ITALIC: &str = "*";
pub const STRIKE: &str = "~~";
pub const CODE: &str = "`";
pub const HIGHLIGHT: &str = "==";
const MARKERS: [&str; 5] = [BOLD, STRIKE, HIGHLIGHT, CODE, ITALIC];

fn is_wrapped(c: &[char], start: usize, end: usize, m: &str) -> bool {
    let m: Vec<char> = m.chars().collect();
    let n = m.len();
    start >= n && c.get(start - n..start) == Some(&m[..]) && c.get(end..end + n) == Some(&m[..])
}

/// Put `marker` round the selection, or take it off when it is there. With nothing selected,
/// the pair goes in with the cursor between.
pub fn wrap(text: &str, start: usize, end: usize, marker: &str) -> Edit {
    let mut c = chars_of(text);
    let (s, e) = ordered(c.len(), start, end);
    let n = marker.chars().count();
    // `*` inside `**`: italic only when the bold pair isn't the whole of what's around it.
    if is_wrapped(&c, s, e, marker) && !(marker == ITALIC && is_wrapped(&c, s, e, BOLD) && !is_wrapped(&c, s, e, "***")) {
        c.drain(e..e + n);
        c.drain(s - n..s);
        return Edit { text: c.into_iter().collect(), start: s - n, end: e - n };
    }
    let m: Vec<char> = marker.chars().collect();
    c.splice(e..e, m.iter().copied());
    c.splice(s..s, m.iter().copied());
    Edit { text: c.into_iter().collect(), start: s + n, end: e + n }
}

/// The markers round the selection, outermost first (what the format painter copies).
pub fn marks_around(text: &str, start: usize, end: usize) -> Vec<&'static str> {
    let c = chars_of(text);
    let (mut s, mut e) = ordered(c.len(), start, end);
    let mut out = Vec::new();
    loop {
        let Some(m) = MARKERS.iter().find(|m| is_wrapped(&c, s, e, m)) else { break };
        let n = m.chars().count();
        out.insert(0, *m);
        s -= n;
        e += n;
        if out.len() > 8 {
            break;
        }
    }
    out
}

/// Put `marks` round the selection (the format painter's paste).
pub fn apply_marks(text: &str, start: usize, end: usize, marks: &[&str]) -> Edit {
    let mut edit = Edit { text: text.to_string(), start, end };
    for m in marks.iter().rev() {
        edit = wrap(&edit.text, edit.start, edit.end, m);
    }
    edit
}

/// The selection without its inline Markdown (clear formatting).
pub fn clear(text: &str, start: usize, end: usize) -> Edit {
    let c = chars_of(text);
    let (s, e) = ordered(c.len(), start, end);
    let inner: String = c.get(s..e).map(|x| x.iter().collect()).unwrap_or_default();
    let mut plain = inner.clone();
    for m in ["**", "__", "~~", "==", "`", "*"] {
        plain = plain.replace(m, "");
    }
    let removed = inner.chars().count() - plain.chars().count();
    let mut out: Vec<char> = c.get(..s).map(<[char]>::to_vec).unwrap_or_default();
    out.extend(plain.chars());
    out.extend(c.get(e..).map(<[char]>::to_vec).unwrap_or_default());
    Edit { text: out.into_iter().collect(), start: s, end: e - removed }
}

/// The lines the selection touches: (first line start, last line end) in characters.
fn line_span(c: &[char], s: usize, e: usize) -> (usize, usize) {
    let first = c.get(..s).map_or(0, |b| b.iter().rposition(|x| *x == '\n').map_or(0, |p| p + 1));
    let last = c.get(e..).map_or(c.len(), |a| a.iter().position(|x| *x == '\n').map_or(c.len(), |p| e + p));
    (first, last)
}

/// Change each line the selection touches with `f`, keeping the selection over them.
fn per_line(text: &str, start: usize, end: usize, mut f: impl FnMut(&str) -> String) -> Edit {
    let c = chars_of(text);
    let (s, e) = ordered(c.len(), start, end);
    let (a, b) = line_span(&c, s, e);
    let block: String = c.get(a..b).map(|x| x.iter().collect()).unwrap_or_default();
    let changed: String = block.split('\n').map(&mut f).collect::<Vec<_>>().join("\n");
    let mut out: Vec<char> = c.get(..a).map(<[char]>::to_vec).unwrap_or_default();
    out.extend(changed.chars());
    let new_end = out.len();
    out.extend(c.get(b..).map(<[char]>::to_vec).unwrap_or_default());
    let (ns, ne) = if s == e { (new_end, new_end) } else { (a, new_end) };
    Edit { text: out.into_iter().collect(), start: ns, end: ne }
}

fn without_heading(line: &str) -> &str {
    match heading(line) {
        Some(_) => line.trim_start_matches('#').trim_start(),
        None => line,
    }
}

/// Make the lines headings of `level` (1–6), or plain text again with 0 (or when they are
/// already that level).
pub fn set_heading(text: &str, start: usize, end: usize, level: u8) -> Edit {
    let level = level.min(6);
    per_line(text, start, end, |line| {
        let same = heading(line).is_some_and(|(l, _)| l == level);
        let body = without_heading(line);
        if level == 0 || same { body.to_string() } else { format!("{} {body}", "#".repeat(usize::from(level))) }
    })
}

/// What a list toggle puts at a line's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Number,
    Task,
}

/// Make the lines list items of `kind`, or plain lines when they all already are.
pub fn toggle_list(text: &str, start: usize, end: usize, kind: ListKind) -> Edit {
    let c = chars_of(text);
    let (s, e) = ordered(c.len(), start, end);
    let (a, b) = line_span(&c, s, e);
    let block: String = c.get(a..b).map(|x| x.iter().collect()).unwrap_or_default();
    let is = |line: &str| match (item(line), kind) {
        (Some((_, ItemKind::Bullet, _)), ListKind::Bullet) => true,
        (Some((_, ItemKind::Number(_), _)), ListKind::Number) => true,
        (Some((_, ItemKind::Task(_), _)), ListKind::Task) => true,
        _ => false,
    };
    let all = block.split('\n').all(is);
    let mut n = 0;
    per_line(text, start, end, |line| {
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let body = item(line).map_or(line.trim_start(), |(_, _, t)| t);
        if all {
            return format!("{indent}{body}");
        }
        n += 1;
        match kind {
            ListKind::Bullet => format!("{indent}- {body}"),
            ListKind::Number => format!("{indent}{n}. {body}"),
            ListKind::Task => format!("{indent}- [ ] {body}"),
        }
    })
}

/// Indent the lines one level (two spaces), or take one off with `out`.
pub fn indent(text: &str, start: usize, end: usize, out: bool) -> Edit {
    per_line(text, start, end, |line| {
        if out {
            line.strip_prefix("  ").or_else(|| line.strip_prefix('\t')).or_else(|| line.strip_prefix(' ')).unwrap_or(line).to_string()
        } else {
            format!("  {line}")
        }
    })
}

/// A link round the selection: `[text](https://)` with the address selected, or `[[text]]`.
pub fn link(text: &str, start: usize, end: usize, markup: bool) -> Edit {
    let c = chars_of(text);
    let (s, e) = ordered(c.len(), start, end);
    let inner: String = c.get(s..e).map(|x| x.iter().collect()).unwrap_or_default();
    let (insert, sel) = if markup {
        let ins = format!("[[{inner}]]");
        let at = s + 2;
        (ins, (at, at + inner.chars().count()))
    } else {
        let ins = format!("[{inner}](https://)");
        let at = s + inner.chars().count() + 3;
        (ins, (at, at + 8))
    };
    let mut out: Vec<char> = c.get(..s).map(<[char]>::to_vec).unwrap_or_default();
    out.extend(insert.chars());
    out.extend(c.get(e..).map(<[char]>::to_vec).unwrap_or_default());
    Edit { text: out.into_iter().collect(), start: sel.0, end: sel.1 }
}

/// The selected lines in a fenced code block.
pub fn code_block(text: &str, start: usize, end: usize) -> Edit {
    let c = chars_of(text);
    let (s, e) = ordered(c.len(), start, end);
    let (a, b) = line_span(&c, s, e);
    let block: String = c.get(a..b).map(|x| x.iter().collect()).unwrap_or_default();
    let fenced = format!("```\n{block}\n```");
    let mut out: Vec<char> = c.get(..a).map(<[char]>::to_vec).unwrap_or_default();
    out.extend(fenced.chars());
    out.extend(c.get(b..).map(<[char]>::to_vec).unwrap_or_default());
    let inner = a + 4;
    Edit { text: out.into_iter().collect(), start: inner, end: inner + block.chars().count() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "# Kitchen\n\n*2026-10-26 13:45 · Steve*\n\nLeak under the **sink**, see [[Photo 3]]. #leak #followup\n\n- [ ] check the trap\n- [x] photo taken\n  - nested\n1. first\n\n> quoted\n\n```\ncode #not-a-tag\n```\n---";

    #[test]
    fn notes_read_into_blocks() {
        let b = blocks(NOTE);
        assert_eq!(b.first(), Some(&Block::Heading { level: 1, text: "Kitchen".into() }));
        assert!(b.contains(&Block::Item { indent: 0, kind: ItemKind::Task(false), text: "check the trap".into(), line: 6 }));
        assert!(b.contains(&Block::Item { indent: 1, kind: ItemKind::Bullet, text: "nested".into(), line: 8 }));
        assert!(b.contains(&Block::Item { indent: 0, kind: ItemKind::Number(1), text: "first".into(), line: 9 }));
        assert!(b.contains(&Block::Quote("quoted".into())));
        assert!(b.contains(&Block::Code("code #not-a-tag".into())));
        assert_eq!(b.last(), Some(&Block::Rule));
        assert_eq!(title(NOTE), "Kitchen");
        assert_eq!(tasks(NOTE), (1, 2));
        assert_eq!(tags(NOTE), vec!["#leak".to_string(), "#followup".to_string()]);
        // Not a heading: no space after the hashes; a tag at a line's start isn't one either.
        assert_eq!(blocks("#leak here"), vec![Block::Paragraph("#leak here".into())]);
    }

    #[test]
    fn lines_read_into_styled_runs() {
        let s = spans("a **b** *c* ~~d~~ ==e== `f **g**` [w](https://x.y) [[D1|door]] #tag x#no");
        let find = |t: &str| s.iter().find(|x| x.text == t).cloned().unwrap_or_default();
        assert!(find("b").bold && find("c").italic && find("d").strike && find("e").highlight);
        assert!(find("f **g**").code, "code is read as it is");
        assert_eq!(find("w").link, Some(Link::Web("https://x.y".into())));
        assert_eq!(find("door").link, Some(Link::Markup("D1".into())));
        assert!(find("#tag").tag);
        assert!(s.iter().all(|x| !x.text.contains("#no") || !x.tag));
        // Unclosed markers and stray brackets read as text, without losing any.
        let odd = spans("`open [half](no close **");
        assert!(odd.iter().map(|x| x.text.as_str()).collect::<String>().contains("open [half](no close"));
    }

    #[test]
    fn checklist_items_tick_and_untick() {
        let ticked = toggle_task(NOTE, 6);
        assert_eq!(tasks(&ticked), (2, 2));
        assert_eq!(tasks(&toggle_task(&ticked, 7)), (1, 2));
        assert_eq!(toggle_task("plain", 9), "plain", "a line past the end changes nothing");
    }

    #[test]
    fn the_toolbar_wraps_and_unwraps() {
        let e = wrap("a word here", 2, 6, BOLD);
        assert_eq!((e.text.as_str(), e.start, e.end), ("a **word** here", 4, 8));
        let back = wrap(&e.text, e.start, e.end, BOLD);
        assert_eq!(back.text, "a word here");
        let empty = wrap("ab", 1, 1, HIGHLIGHT);
        assert_eq!((empty.text.as_str(), empty.start), ("a====b", 3));
        // Italic inside bold adds a pair rather than undoing the bold.
        assert_eq!(wrap("**x**", 2, 3, ITALIC).text, "***x***");
        assert_eq!(marks_around("a ***x*** b", 5, 6), vec![ITALIC, BOLD]);
        assert_eq!(apply_marks("y", 0, 1, &[BOLD, ITALIC]).text, "***y***");
        let c = clear("a **b** ~~c~~", 0, 13);
        assert_eq!((c.text.as_str(), c.end), ("a b c", 5));
    }

    #[test]
    fn lines_become_headings_lists_and_code() {
        assert_eq!(set_heading("title\nrest", 0, 0, 2).text, "## title\nrest");
        assert_eq!(set_heading("## title", 3, 3, 2).text, "title", "the same level again takes it off");
        assert_eq!(set_heading("### t", 0, 0, 2).text, "## t");
        assert_eq!(toggle_list("a\nb", 0, 3, ListKind::Task).text, "- [ ] a\n- [ ] b");
        assert_eq!(toggle_list("- [ ] a\n- [ ] b", 0, 15, ListKind::Task).text, "a\nb");
        assert_eq!(toggle_list("a\nb", 0, 3, ListKind::Number).text, "1. a\n2. b");
        assert_eq!(indent("- a", 0, 0, false).text, "  - a");
        assert_eq!(indent("  - a", 0, 0, true).text, "- a");
        let l = link("see door", 4, 8, true);
        assert_eq!((l.text.as_str(), l.start, l.end), ("see [[door]]", 6, 10));
        let w = link("site", 0, 4, false);
        assert_eq!(w.text, "[site](https://)");
        assert_eq!(w.text.chars().skip(w.start).take(w.end - w.start).collect::<String>(), "https://");
        assert_eq!(code_block("x = 1", 0, 0).text, "```\nx = 1\n```");
        // Selections past the end are held to it.
        assert_eq!(wrap("ab", 5, 9, BOLD).text, "ab****");
    }
}
