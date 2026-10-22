//! Bounded, non-executing shell literal parsing shared by tool-risk consumers.

use std::fmt;

pub(crate) const MAX_COMMAND_BYTES: usize = 8192;
pub(crate) const MAX_SEGMENTS: usize = 128;
pub(crate) const MAX_REDIRECTIONS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Connector {
    Pipe,
    And,
    Or,
    Sequence,
    Background,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedirectionKind {
    Input,
    Output,
    Append,
    FdDuplicate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsupportedTail {
    VariableExpansion,
    CommandSubstitution,
    UnsupportedSyntax,
    UnterminatedQuote,
    DanglingEscape,
    MissingRedirectionTarget,
    MissingCommand,
    CommandBytesLimit,
    SegmentLimit,
    RedirectionLimit,
}
impl UnsupportedTail {
    pub(crate) fn is_obfuscated(self) -> bool {
        matches!(self, Self::VariableExpansion | Self::CommandSubstitution)
    }
}

pub(crate) struct Redirection {
    pub kind: RedirectionKind,
    pub target: String,
    pub fd: Option<u32>,
    pub both_outputs: bool,
}
pub(crate) struct ShellSegment {
    pub words: Vec<String>,
    pub connector: Option<Connector>,
    pub redirections: Vec<Redirection>,
    /// The entire segment is lexically known, independently of later unknown input.
    pub complete: bool,
    /// Count of leading NAME= words with unquoted name and equals sign.
    pub assignment_prefix: usize,
}
pub(crate) struct ParsedShell {
    pub segments: Vec<ShellSegment>,
    pub complete: bool,
    pub unsupported_tail: Option<UnsupportedTail>,
}
impl fmt::Debug for Redirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redirection")
            .field("kind", &self.kind)
            .field("fd", &self.fd)
            .field("both_outputs", &self.both_outputs)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for ShellSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellSegment")
            .field("word_count", &self.words.len())
            .field("connector", &self.connector)
            .field("redirection_count", &self.redirections.len())
            .field("complete", &self.complete)
            .finish()
    }
}
impl fmt::Debug for ParsedShell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedShell")
            .field("segment_count", &self.segments.len())
            .field("complete", &self.complete)
            .field("unsupported_tail", &self.unsupported_tail)
            .finish()
    }
}

struct Word {
    text: String,
    plain_digits: bool,
    assignment: bool,
}
impl ShellSegment {
    pub(crate) fn executable_words(&self) -> &[String] {
        &self.words[self.assignment_prefix..]
    }
}
struct Cursor<'a> {
    text: &'a str,
    offset: usize,
    truncated: bool,
}
impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.offset..)?.chars().next()
    }
    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }
    fn starts(&self, pattern: &str) -> bool {
        self.text
            .get(self.offset..)
            .is_some_and(|tail| tail.starts_with(pattern))
    }
    fn blanks(&mut self) {
        while self.peek().is_some_and(|ch| matches!(ch, ' ' | '\t')) {
            self.next();
        }
    }
    // Never treat a budget boundary as a real EOF, including a closed quoted token.
    fn word(&mut self, allow_glob: bool) -> Result<Option<Word>, UnsupportedTail> {
        let raw_start = self.offset;
        let mut text = String::new();
        let mut started = false;
        let mut decorated = false;
        let mut quote = None;
        while let Some(ch) = self.peek() {
            if let Some(q) = quote {
                self.next();
                if ch == q {
                    quote = None;
                    continue;
                }
                if q == '"' && matches!(ch, '$' | '`') {
                    return Err(if ch == '`' || self.peek() == Some('(') {
                        UnsupportedTail::CommandSubstitution
                    } else {
                        UnsupportedTail::VariableExpansion
                    });
                }
                if q == '"' && ch == '\\' {
                    if self
                        .peek()
                        .is_some_and(|next| matches!(next, '$' | '`' | '"' | '\\' | '\n'))
                    {
                        if let Some(next) = self.next() {
                            if next != '\n' {
                                text.push(next);
                            }
                        }
                    } else {
                        text.push(ch);
                    }
                } else {
                    text.push(ch);
                }
                continue;
            }
            match ch {
                ' ' | '\t' | '\n' | ';' | '|' | '&' | '<' | '>' => break,
                '\'' | '"' => {
                    self.next();
                    quote = Some(ch);
                    started = true;
                    decorated = true;
                }
                '\\' => {
                    self.next();
                    let Some(next) = self.next() else {
                        return Err(if self.truncated {
                            UnsupportedTail::CommandBytesLimit
                        } else {
                            UnsupportedTail::DanglingEscape
                        });
                    };
                    if next != '\n' {
                        text.push(next);
                        started = true;
                        decorated = true;
                    }
                }
                '$' | '`' => {
                    self.next();
                    return Err(if ch == '`' || self.peek() == Some('(') {
                        UnsupportedTail::CommandSubstitution
                    } else {
                        UnsupportedTail::VariableExpansion
                    });
                }
                '(' | ')' | '{' | '}' | '#' => return Err(UnsupportedTail::UnsupportedSyntax),
                '*' | '?' | '[' if !allow_glob => return Err(UnsupportedTail::UnsupportedSyntax),
                ch if ch.is_control() || ch.is_whitespace() => {
                    return Err(UnsupportedTail::UnsupportedSyntax)
                }
                _ => {
                    self.next();
                    text.push(ch);
                    started = true;
                }
            }
        }
        if self.peek().is_none() && self.truncated {
            return Err(UnsupportedTail::CommandBytesLimit);
        }
        if quote.is_some() {
            return Err(UnsupportedTail::UnterminatedQuote);
        }
        let assignment = self.text.get(raw_start..self.offset).is_some_and(|raw| {
            raw.split_once('=').is_some_and(|(name, _)| {
                // Unquoted line continuations vanish before assignment recognition;
                // quotes or any remaining escapes still invalidate the raw name.
                let name = name.replace("\\\n", "");
                let mut chars = name.chars();
                chars
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                    && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            })
        });
        Ok(started.then(|| Word {
            plain_digits: !decorated
                && !text.is_empty()
                && text.bytes().all(|ch| ch.is_ascii_digit()),
            assignment,
            text,
        }))
    }

    fn redirect(
        &mut self,
        fd: Option<u32>,
        both_outputs: bool,
    ) -> Result<Redirection, UnsupportedTail> {
        let Some(operator) = self.next() else {
            return Err(UnsupportedTail::MissingRedirectionTarget);
        };
        let mut kind = if operator == '<' {
            RedirectionKind::Input
        } else {
            RedirectionKind::Output
        };
        if self.peek() == Some(operator) {
            if operator == '<' {
                return Err(UnsupportedTail::UnsupportedSyntax);
            }
            self.next();
            kind = RedirectionKind::Append;
        }
        if self
            .peek()
            .is_some_and(|ch| matches!(ch, '<' | '>' | '|' | '('))
        {
            return Err(UnsupportedTail::UnsupportedSyntax);
        }
        let duplicate = self.peek() == Some('&');
        if duplicate {
            if kind == RedirectionKind::Append || both_outputs || operator == '<' {
                return Err(UnsupportedTail::UnsupportedSyntax);
            }
            self.next();
            kind = RedirectionKind::FdDuplicate;
        }
        self.blanks();
        let Some(target) = self.word(false)? else {
            return Err(UnsupportedTail::MissingRedirectionTarget);
        };
        if target.text.is_empty()
            || duplicate && (!target.plain_digits || target.text.parse::<u32>().is_err())
        {
            return Err(UnsupportedTail::UnsupportedSyntax);
        }
        Ok(Redirection {
            kind,
            target: target.text,
            fd,
            both_outputs,
        })
    }
}

/// Parse only a bounded prefix. No expansion, execution, filesystem or network access.
pub(crate) fn parse(command: &str) -> ParsedShell {
    let mut end = command.len().min(MAX_COMMAND_BYTES);
    while !command.is_char_boundary(end) {
        end -= 1;
    }
    let mut cursor = Cursor {
        text: &command[..end],
        offset: 0,
        truncated: end < command.len(),
    };
    let mut parsed = ParsedShell {
        segments: Vec::new(),
        complete: true,
        unsupported_tail: None,
    };
    let mut segment = ShellSegment {
        words: Vec::new(),
        connector: None,
        redirections: Vec::new(),
        complete: false,
        assignment_prefix: 0,
    };
    let mut redirections = 0;
    loop {
        cursor.blanks();
        let Some(ch) = cursor.peek() else {
            if cursor.truncated {
                parsed.unsupported_tail = Some(UnsupportedTail::CommandBytesLimit);
            } else if segment.words.is_empty() {
                parsed.unsupported_tail = Some(UnsupportedTail::MissingCommand);
            } else {
                segment.complete = true;
            }
            break;
        };
        if parsed.segments.len() >= MAX_SEGMENTS {
            parsed.unsupported_tail = Some(UnsupportedTail::SegmentLimit);
            break;
        }
        if matches!(ch, ';' | '|' | '&' | '\n') && !cursor.starts("&>") {
            if segment.words.is_empty() {
                parsed.unsupported_tail = Some(UnsupportedTail::MissingCommand);
                break;
            }
            cursor.next();
            let connector = match ch {
                '|' if cursor.peek() == Some('|') => {
                    cursor.next();
                    Connector::Or
                }
                '&' if cursor.peek() == Some('&') => {
                    cursor.next();
                    Connector::And
                }
                '|' => Connector::Pipe,
                '&' => Connector::Background,
                _ => Connector::Sequence,
            };
            segment.complete = true;
            parsed.segments.push(segment);
            segment = ShellSegment {
                words: Vec::new(),
                connector: Some(connector),
                redirections: Vec::new(),
                complete: false,
                assignment_prefix: 0,
            };
            if connector == Connector::Background {
                parsed.unsupported_tail = Some(UnsupportedTail::UnsupportedSyntax);
                break;
            }
            continue;
        }
        let both_outputs = cursor.starts("&>");
        let mut fd = None;
        if !matches!(ch, '<' | '>') && !both_outputs {
            match cursor.word(!segment.words.is_empty()) {
                Ok(Some(word)) => {
                    if word.plain_digits && cursor.peek().is_some_and(|ch| matches!(ch, '<' | '>'))
                    {
                        let Ok(value) = word.text.parse() else {
                            parsed.unsupported_tail = Some(UnsupportedTail::UnsupportedSyntax);
                            break;
                        };
                        fd = Some(value);
                    } else {
                        if word.assignment && segment.assignment_prefix == segment.words.len() {
                            segment.assignment_prefix += 1;
                        }
                        segment.words.push(word.text);
                        continue;
                    }
                }
                Ok(None) => continue,
                Err(reason) => {
                    parsed.unsupported_tail = Some(reason);
                    break;
                }
            }
        }
        if redirections >= MAX_REDIRECTIONS {
            parsed.unsupported_tail = Some(UnsupportedTail::RedirectionLimit);
            break;
        }
        if both_outputs {
            cursor.next();
        }
        match cursor.redirect(fd, both_outputs) {
            Ok(redirect) => {
                segment.redirections.push(redirect);
                redirections += 1;
            }
            Err(reason) => {
                parsed.unsupported_tail = Some(reason);
                break;
            }
        }
    }
    if (!segment.words.is_empty() || !segment.redirections.is_empty())
        && parsed.segments.len() < MAX_SEGMENTS
    {
        parsed.segments.push(segment);
    }
    parsed.complete = parsed.unsupported_tail.is_none() && !parsed.segments.is_empty();
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_connectors_and_literal_words() {
        let parsed = parse("cat 'a b' | head && ls || pwd; echo done\nuname");
        assert!(parsed.complete);
        assert_eq!(parsed.segments.len(), 6);
        assert_eq!(parsed.segments[0].words, ["cat", "a b"]);
        assert_eq!(
            parsed
                .segments
                .iter()
                .map(|s| s.connector)
                .collect::<Vec<_>>(),
            [
                None,
                Some(Connector::Pipe),
                Some(Connector::And),
                Some(Connector::Or),
                Some(Connector::Sequence),
                Some(Connector::Sequence)
            ]
        );
    }

    #[test]
    fn redirects_preserve_kind_fd_and_targets() {
        let parsed = parse("cat < .env 1> out 2>> error &> both 2>&1");
        assert!(parsed.complete);
        assert_eq!(parsed.segments[0].words, ["cat"]);
        let redirects = &parsed.segments[0].redirections;
        assert_eq!(redirects.len(), 5);
        assert_eq!(redirects[0].kind, RedirectionKind::Input);
        assert_eq!(redirects[0].target, ".env");
        assert_eq!(redirects[1].kind, RedirectionKind::Output);
        assert_eq!(redirects[1].fd, Some(1));
        assert_eq!(redirects[2].kind, RedirectionKind::Append);
        assert!(redirects[3].both_outputs);
        assert_eq!(redirects[4].kind, RedirectionKind::FdDuplicate);
        assert_eq!(redirects[4].target, "1");
        assert_eq!(redirects[4].fd, Some(2));
    }

    #[test]
    fn quotes_and_escapes_cannot_become_fd_identity() {
        for program in ["\"1\"", "'2'", "\\1", "''1", "1\"\""] {
            let parsed = parse(&format!("{program}>/dev/null ls"));
            assert!(parsed.complete);
            assert_eq!(parsed.segments[0].words.len(), 2);
            assert_eq!(parsed.segments[0].redirections[0].fd, None);
        }
        let parsed = parse("1>/dev/null ls > '&1'");
        assert_eq!(parsed.segments[0].words, ["ls"]);
        assert_eq!(parsed.segments[0].redirections[0].fd, Some(1));
        assert_eq!(
            parsed.segments[0].redirections[1].kind,
            RedirectionKind::Output
        );
        assert_eq!(parsed.segments[0].redirections[1].target, "&1");
    }

    #[test]
    fn unknown_tokens_are_not_finished_but_prior_facts_survive() {
        for command in [
            "cat .env; echo $UNKNOWN",
            "cat .env; echo \"$UNKNOWN\"",
            "cat .env; echo $(date)",
            "cat .env; echo `date`",
            "cat .env; echo \"tail",
            "cat .env; echo tail\\",
        ] {
            let parsed = parse(command);
            assert!(!parsed.complete, "{command}");
            assert_eq!(parsed.segments[0].words, ["cat", ".env"]);
            assert_eq!(parsed.segments[1].words, ["echo"]);
        }
        for command in ["\"sudo$UNKNOWN\" ls", "\"sudo", "rm$UNKNOWN -rf /", "*.sh"] {
            let parsed = parse(command);
            assert!(!parsed.complete);
            assert!(parsed.segments.is_empty(), "{command}");
        }
        let parsed = parse("echo demo > $TARGET");
        assert_eq!(parsed.segments[0].words, ["echo", "demo"]);
        assert!(parsed.segments[0].redirections.is_empty());
    }

    #[test]
    fn finite_posix_quoting_and_continuations() {
        let parsed = parse("cat .e\\\nnv \"a\\ b\" '$(literal)' \"\" a\\ b");
        assert!(parsed.complete);
        assert_eq!(
            parsed.segments[0].words,
            ["cat", ".env", "a\\ b", "$(literal)", "", "a b"]
        );
        assert!(parse("rm gone-*.tmp").complete);
        assert!(!parse("echo\runknown").complete);
        assert!(!parse("ls\u{00a0}unknown").complete);
    }

    #[test]
    fn unsupported_redirects_do_not_fabricate_targets() {
        for command in [
            "cat << EOF",
            "cat <(echo hi)",
            "echo >| out",
            "echo >",
            "echo > \"out",
            "echo >&$FD",
        ] {
            let parsed = parse(command);
            assert!(!parsed.complete, "{command}");
            assert!(parsed.segments[0].redirections.is_empty(), "{command}");
        }
        for command in ["ls &&", "ls;", "ls |", "ls & other", "ls ;; pwd"] {
            assert!(!parse(command).complete, "{command}");
        }
    }

    #[test]
    fn byte_budget_drops_cut_tokens_and_handles_utf8() {
        let exact = format!("echo {}", "a".repeat(MAX_COMMAND_BYTES - 5));
        assert!(parse(&exact).complete);
        let cut = parse(&(exact + "b"));
        assert!(!cut.complete);
        assert_eq!(cut.segments[0].words, ["echo"]);
        let unicode = parse(&format!("cat .env; echo {}", "é".repeat(MAX_COMMAND_BYTES)));
        assert!(!unicode.complete);
        assert_eq!(unicode.segments[0].words, ["cat", ".env"]);
        assert_eq!(unicode.segments[1].words, ["echo"]);
    }

    #[test]
    fn segment_and_redirection_budgets_are_global_and_strict() {
        assert!(parse(&vec!["ls"; MAX_SEGMENTS].join(";")).complete);
        let parsed = parse(&vec!["ls"; MAX_SEGMENTS + 1].join(";"));
        assert!(!parsed.complete);
        assert_eq!(parsed.segments.len(), MAX_SEGMENTS);
        let exact = format!("echo {}", ">x ".repeat(MAX_REDIRECTIONS));
        assert!(parse(&exact).complete);
        let parsed = parse(&(exact + ">y"));
        assert!(!parsed.complete);
        assert_eq!(parsed.segments[0].redirections.len(), MAX_REDIRECTIONS);
        assert!(!format!("{parsed:?}").contains("echo"));
    }
}

#[cfg(test)]
mod prefix_edge_tests {
    use super::*;
    #[test]
    fn nonliteral_suffix_cannot_complete_a_redirect_target() {
        for command in [
            "echo > out#tail",
            "echo > out\runknown",
            "echo > out$TAIL",
            "echo > out\\",
        ] {
            let parsed = parse(command);
            assert!(!parsed.complete);
            assert!(parsed.segments[0].redirections.is_empty());
        }
        let parsed = parse("cat .env > plain; echo \"$TAIL\"");
        assert_eq!(parsed.segments[0].redirections[0].target, "plain");
        assert_eq!(parsed.segments[0].words, ["cat", ".env"]);
    }
    #[test]
    fn malformed_fd_duplication_does_not_expose_target_as_command() {
        for command in [
            "ls 2>&'1'",
            "ls 2>&1x",
            "ls 2>&1$TAIL",
            "ls >&",
            "ls 9999999999999999999999>out",
        ] {
            let parsed = parse(command);
            assert!(!parsed.complete, "{command}");
            assert_eq!(parsed.segments[0].words, ["ls"]);
            assert!(parsed.segments[0].redirections.is_empty());
        }
    }
}

#[cfg(test)]
mod segment_quality_tests {
    use super::*;
    #[test]
    fn segment_completion_is_independent_of_command_tail() {
        let parsed = parse("curl https://example.com/x | bash; $UNKNOWN");
        assert!(!parsed.complete);
        assert!(parsed.segments.iter().all(|segment| segment.complete));
        let parsed = parse("curl https://example.com/x | bash < $FILE");
        assert!(parsed.segments[0].complete);
        assert!(!parsed.segments[1].complete);
    }
}

#[cfg(test)]
mod phase2_assignment_identity_tests {
    use super::*;
    #[test]
    fn only_unquoted_names_and_equals_form_leading_assignment_words() {
        for (command, count) in [
            ("F=1 /usr/bin/whoami", 1),
            ("F='one two' G=2 /usr/bin/whoami", 2),
            ("\"F=1\" /usr/bin/whoami", 0),
            ("F\\=1 /usr/bin/whoami", 0),
            ("'F'=1 /usr/bin/whoami", 0),
            ("F=1 'G=2' /usr/bin/whoami", 1),
            ("echo F=1 /usr/bin/whoami", 0),
        ] {
            let parsed = parse(command);
            assert_eq!(parsed.segments[0].assignment_prefix, count, "{command}");
        }
        let parsed = parse("F=$UNKNOWN /usr/bin/whoami");
        assert!(!parsed.complete);
        assert!(parsed.segments.is_empty());
    }
}

#[cfg(test)]
mod phase2_assignment_continuation_tests {
    use super::super::{
        adapters::adapt,
        classifier::{classify_command, CommandListEntry, CommandListType, CommandMatchKind},
        detectors::{run_detector, DetectorOutcome},
        types::{OperationType, RiskCategory, Severity},
    };
    use super::*;
    use serde_json::json;

    #[test]
    fn unquoted_continuations_preserve_assignment_identity() {
        for prefix in ["F\\\n=1", "F\\\nOO=1", "\\\nF=1", "F\\\n=\\\n'one two'"] {
            let parsed = parse(&format!("{prefix} /usr/bin/whoami"));
            assert!(parsed.complete);
            assert_eq!(parsed.segments[0].assignment_prefix, 1);
            assert_eq!(parsed.segments[0].executable_words(), ["/usr/bin/whoami"]);
        }
        for prefix in ["\"F\\\n=1\"", "'F\\\n=1'", "F\\=\\\n1", "F\\\\\n=1"] {
            let parsed = parse(&format!("{prefix} /usr/bin/whoami"));
            assert_eq!(parsed.segments[0].assignment_prefix, 0);
        }
    }

    #[test]
    fn continued_assignment_does_not_hide_program_blacklist() {
        let black = CommandListEntry {
            id: "phase2-continuation-black".into(),
            revision: 1,
            list_type: CommandListType::Blacklist,
            match_kind: CommandMatchKind::Program,
            pattern: "whoami".into(),
            enabled: true,
            severity: Severity::High,
            risk_category: RiskCategory::RemoteCodeExecution,
        };
        assert_eq!(
            classify_command("F\\\n=1 /usr/bin/whoami", &[black])
                .blacklist_hits
                .len(),
            1
        );
    }

    #[test]
    fn continued_assignment_preserves_adapter_and_detector_facts() {
        let result = adapt("Bash", &json!({"command": "F\\\n=1 head .env"}));
        assert!(result
            .operations
            .iter()
            .any(|op| op.op_type == OperationType::FileRead
                && op.target_path.as_deref() == Some(".env")));
        let result = adapt("Bash", &json!({"command": "F\\\n=1 rm -rf /"}));
        assert_eq!(
            run_detector(
                super::super::detectors::DESTRUCTIVE_PATH,
                &result.operations[0]
            ),
            DetectorOutcome::Match
        );
    }
}
