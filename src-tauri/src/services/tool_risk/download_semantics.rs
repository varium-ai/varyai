//! Finite, non-executing download-to-file execution facts over the shared parser.
//! Paths stay borrowed and private; no filesystem, expansion, PATH or network I/O.

use super::shell::{Connector, ParsedShell, RedirectionKind, ShellSegment};

/// Unknown parameter roles are different from a known non-chain.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChainOutcome {
    Match,
    NoMatch,
    Indeterminate,
}

type Fact<'a> = Result<Option<&'a str>, ()>;

fn literal_path(path: &str) -> bool {
    !path.is_empty()
        && path != "-"
        && !path.starts_with(['-', '~'])
        && !path.starts_with("/dev/fd/")
        && !(path.starts_with("/proc/") && path.contains("/fd/"))
        && !path
            .chars()
            .any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '\0'))
        && !matches!(
            path,
            "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/stdin"
        )
}

fn network_url(value: &str) -> bool {
    // URL-library repairs do not establish the literal CLI parameter contract.
    let Some((scheme, tail)) = value.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https" | "ftp")
        || tail.split(['/', '?', '#']).next().is_none_or(str::is_empty)
        || value
            .chars()
            .any(|ch| ch == '\\' || ch.is_control() || ch.is_whitespace())
    {
        return false;
    }
    url::Url::parse(value).ok().is_some_and(|url| {
        matches!(url.scheme(), "http" | "https" | "ftp") && url.host_str().is_some()
    })
}

fn affects_stdout(redirect: &super::shell::Redirection) -> bool {
    redirect.both_outputs
        || match redirect.kind {
            RedirectionKind::Input => false,
            RedirectionKind::Output | RedirectionKind::Append => {
                matches!(redirect.fd, None | Some(1))
            }
            RedirectionKind::FdDuplicate => redirect.fd == Some(1),
        }
}

fn writes_path(segment: &ShellSegment, path: &str) -> bool {
    segment.redirections.iter().any(|redirect| {
        matches!(
            redirect.kind,
            RedirectionKind::Output | RedirectionKind::Append
        ) && redirect.target == path
    })
}

fn download(segment: &ShellSegment) -> Fact<'_> {
    let words = segment.executable_words();
    let Some(program) = words.first().map(String::as_str) else {
        return Ok(None);
    };
    if !matches!(program, "curl" | "wget") {
        return Ok(None);
    }
    if !segment.complete {
        return Err(());
    }
    let curl = program == "curl";
    let mut output = None;
    let mut url = None;
    let mut options = true;
    let mut index = 1;
    while let Some(word) = words.get(index) {
        if options && word == "--" {
            options = false;
            index += 1;
            continue;
        }
        if options && word.starts_with('-') {
            let separated_output = if curl {
                matches!(word.as_str(), "-o" | "--output")
            } else {
                matches!(word.as_str(), "-O" | "--output-document")
            };
            let joined_output = if curl {
                word.strip_prefix("--output=")
                    .or_else(|| word.strip_prefix("-o"))
            } else {
                word.strip_prefix("--output-document=")
                    .or_else(|| word.strip_prefix("-O"))
            };
            if separated_output {
                index += 1;
                let value = words.get(index).ok_or(())?;
                if output.replace(value.as_str()).is_some() {
                    return Err(());
                }
            } else if let Some(value) = joined_output {
                if value.is_empty() || output.replace(value).is_some() {
                    return Err(());
                }
            } else {
                let known = if curl {
                    matches!(
                        word.as_str(),
                        "--silent" | "--show-error" | "--fail" | "--location" | "--disable"
                    ) || !word.starts_with("--")
                        && word.len() > 1
                        && word[1..]
                            .bytes()
                            .all(|ch| matches!(ch, b'f' | b's' | b'S' | b'L' | b'q'))
                } else {
                    matches!(word.as_str(), "-q" | "--quiet")
                };
                if !known {
                    return Err(());
                }
            }
        } else {
            // curl performs its own URL glob expansion even inside shell quotes.
            // Braces anywhere expand; only IPv6 authority brackets are literal.
            let curl_glob = curl
                && (word.contains(['{', '}'])
                    || word.split_once("://").is_some_and(|(_, tail)| {
                        let path_start = tail.find(['/', '?', '#']).unwrap_or(tail.len());
                        let authority = &tail[..path_start];
                        let authority_glob = match authority.rsplit_once('@') {
                            Some((userinfo, name)) => {
                                userinfo.contains(['[', ']'])
                                    || (!name.starts_with('[') && name.contains(['[', ']']))
                            }
                            None => !authority.starts_with('[') && authority.contains(['[', ']']),
                        };
                        authority_glob || tail[path_start..].contains(['[', ']'])
                    }));
            if !network_url(word) || curl_glob || url.replace(word.as_str()).is_some() {
                return Err(());
            }
        }
        index += 1;
    }
    if url.is_none() {
        return Ok(None);
    }
    // wget without -O writes an implicit filename, not response bytes to stdout.
    if !curl && output.is_none() {
        return Ok(None);
    }
    let to_stdout = output.is_none() || output == Some("-");
    if to_stdout {
        let Some(redirect) = segment
            .redirections
            .iter()
            .rev()
            .find(|r| affects_stdout(r))
        else {
            return Ok(None);
        };
        if redirect.kind != RedirectionKind::Output
            || redirect.both_outputs
            || !literal_path(&redirect.target)
            || segment.redirections.iter().any(|r| {
                r.kind == RedirectionKind::FdDuplicate
                    || r.target == redirect.target
                        && (r.kind == RedirectionKind::Append
                            || r.kind == RedirectionKind::Output
                                && (r.both_outputs || !affects_stdout(r)))
            })
        {
            return Err(());
        }
        Ok(Some(&redirect.target))
    } else {
        let path = output.ok_or(())?;
        if !literal_path(path)
            || writes_path(segment, path)
            || segment.redirections.iter().any(affects_stdout)
        {
            return Err(());
        }
        Ok(Some(path))
    }
}

fn direct_path(path: &str) -> bool {
    literal_path(path) && (path.starts_with('/') || path.starts_with("./"))
}

fn execution(segment: &ShellSegment) -> Fact<'_> {
    if !segment.complete {
        return Err(());
    }
    let words = segment.executable_words();
    let Some(program) = words.first().map(String::as_str) else {
        return Ok(None);
    };
    if direct_path(program) {
        return Ok(Some(program));
    }
    if !matches!(program, "bash" | "sh" | "zsh" | "python" | "node") {
        return Ok(None);
    }
    let mut options = true;
    for word in &words[1..] {
        if options && word == "--" {
            options = false;
            continue;
        }
        if options && word.starts_with('-') {
            // Only an option terminator is certified initially. Eval/module and
            // options with script-looking values never become a script operand.
            return Err(());
        }
        return if literal_path(word) {
            Ok(Some(word))
        } else {
            Err(())
        };
    }
    Ok(None)
}

fn connected(segment: &ShellSegment) -> bool {
    matches!(
        segment.connector,
        Some(Connector::And | Connector::Sequence)
    )
}

fn is_chmod(segment: &ShellSegment, path: &str) -> bool {
    let words = segment.executable_words();
    segment.complete
        && connected(segment)
        && words.len() == 3
        && words[0] == "chmod"
        && words[1] == "+x"
        && words[2] == path
        && !writes_path(segment, path)
}

/// Only adjacent pairs / the controlled chmod triple can prove a new chain.
/// A later unsupported tail cannot erase a previously confirmed chain.
pub(crate) fn file_execution(parsed: &ParsedShell) -> ChainOutcome {
    let mut unknown = !parsed.complete;
    // Background syntax can apply to the entire terminal AND/OR/pipeline group.
    // The parser drops its empty successor, so conservatively exclude that group
    // up to the last Sequence boundary. Separately unknown tokens preserve prior
    // foreground chains; this does not alter the historical pipeline detector.
    let uncertain_group = if parsed.unsupported_tail
        == Some(super::shell::UnsupportedTail::UnsupportedSyntax)
        && parsed
            .segments
            .last()
            .is_some_and(|segment| segment.complete)
    {
        Some(
            parsed
                .segments
                .iter()
                .rposition(|segment| segment.connector == Some(Connector::Sequence))
                .unwrap_or(0),
        )
    } else {
        None
    };
    let boundary_known = |index: usize| {
        uncertain_group.is_none_or(|start| index < start)
            && !parsed
                .segments
                .get(index + 1)
                .is_some_and(|segment| segment.connector == Some(Connector::Pipe))
    };
    for (index, segment) in parsed.segments.iter().enumerate() {
        // A download sharing an incoming pipeline has concurrent peer effects.
        if segment.connector == Some(Connector::Pipe) {
            continue;
        }
        // Unsupported connectors and isolated downloads are not candidates for
        // this file-chain contract; leave the historical pipeline outcome intact.
        let Some(next) = parsed.segments.get(index + 1).filter(|s| connected(s)) else {
            continue;
        };
        let path = match download(segment) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(()) => {
                unknown = true;
                continue;
            }
        };
        match execution(next) {
            Ok(Some(script))
                if script == path && !writes_path(next, path) && boundary_known(index + 1) =>
            {
                return ChainOutcome::Match
            }
            Err(()) => unknown = true,
            _ => {}
        }
        if is_chmod(next, path) {
            if let Some(last) = parsed.segments.get(index + 2).filter(|s| connected(s)) {
                let words = last.executable_words();
                if last.complete
                    && boundary_known(index + 2)
                    && words
                        .first()
                        .is_some_and(|program| program == path && direct_path(program))
                    && !writes_path(last, path)
                {
                    return ChainOutcome::Match;
                }
                if !last.complete {
                    unknown = true;
                }
            }
        }
    }
    if unknown {
        ChainOutcome::Indeterminate
    } else {
        ChainOutcome::NoMatch
    }
}
