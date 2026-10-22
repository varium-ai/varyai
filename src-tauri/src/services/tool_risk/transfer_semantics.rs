//! Finite file-transfer facts over literal argv; no execution, DNS or config access.

use super::shell::{RedirectionKind, ShellSegment};

/// Not Debug: source paths and destinations are private in-memory facts.
pub(crate) struct Transfer {
    pub source: String,
    pub destination: String,
}

fn port(value: &str) -> Option<u16> {
    if value.is_empty() || !value.bytes().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    value.parse::<u16>().ok().filter(|port| *port != 0)
}

// Reject unqualified aliases rather than expand local SSH/DNS configuration.
fn host(value: &str) -> Option<String> {
    let raw = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    if let Ok(ip) = raw.parse::<std::net::IpAddr>() {
        return Some(match ip {
            std::net::IpAddr::V4(ip) => ip.to_string(),
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
        });
    }
    if !raw.is_ascii()
        || raw
            .bytes()
            .any(|ch| !ch.is_ascii_alphanumeric() && !matches!(ch, b'.' | b'-'))
    {
        return None;
    }
    match url::Host::parse(raw).ok()? {
        url::Host::Ipv4(ip) => Some(ip.to_string()),
        url::Host::Ipv6(ip) => Some(format!("[{ip}]")),
        url::Host::Domain(domain) => {
            if !domain.eq_ignore_ascii_case("localhost") && !domain.contains('.') {
                return None;
            }
            let labels = domain.strip_suffix('.').unwrap_or(&domain);
            if labels.len() > 253
                || labels.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                })
            {
                return None;
            }
            Some(domain.to_ascii_lowercase())
        }
    }
}

fn endpoint(scheme: &str, name: &str, number: Option<u16>) -> Option<String> {
    let name = host(name)?;
    Some(match number {
        Some(number) => format!("{scheme}://{name}:{number}/"),
        None => format!("{scheme}://{name}/"),
    })
}

/// Validity/loopback for the SSH/TCP contracts, independent of path sensitivity.
pub(crate) fn transport_loopback(url: &url::Url) -> Option<bool> {
    match url.scheme() {
        "ssh" => {
            if url.port() == Some(0) {
                return None;
            }
        }
        "tcp" => {
            url.port().filter(|port| *port != 0)?;
        }
        _ => return None,
    }
    let name = host(url.host_str()?)?;
    let raw = name
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(&name);
    Some(match raw.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
        Err(_) => raw.trim_end_matches('.').eq_ignore_ascii_case("localhost"),
    })
}

fn local_source(value: &str) -> bool {
    if value.is_empty() || value == "-" {
        return false;
    }
    // scp/rsync use a colon before any slash as remote syntax, even for an alias.
    value
        .find(':')
        .is_none_or(|colon| value.find('/').is_some_and(|slash| slash < colon))
}

fn ssh_destination(operand: &str, number: Option<u16>) -> Option<String> {
    let name = match operand.split_once('@') {
        Some((user, name))
            if !user.is_empty()
                && user
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-' | b'.')) =>
        {
            name
        }
        Some(_) => return None,
        None => operand,
    };
    let (name, path) = if name.starts_with('[') {
        let end = name.find(']')?;
        (&name[..=end], name.get(end + 1..)?.strip_prefix(':')?)
    } else {
        name.split_once(':')?
    };
    if path.is_empty() || path.starts_with(':') {
        return None;
    }
    endpoint("ssh", name, number)
}

fn ssh_transfers(words: &[String], scp: bool) -> Option<Vec<Transfer>> {
    let mut index = 1;
    let mut number = None;
    let mut options = true;
    let mut operands = Vec::new();
    let mut terminated = false;
    while let Some(word) = words.get(index) {
        if options && word == "--" {
            options = false;
            terminated = true;
            index += 1;
            continue;
        }
        if options && word.starts_with('-') && word != "-" {
            if scp && word.starts_with("-P") {
                let value = if word == "-P" {
                    index += 1;
                    words.get(index)?.as_str()
                } else {
                    &word[2..]
                };
                // Conflicting/repeated port roles remain outside the finite subset.
                if number.is_some() {
                    return None;
                }
                number = Some(port(value)?);
            } else if scp && word.starts_with("-i") {
                let value = if word == "-i" {
                    index += 1;
                    words.get(index)?.as_str()
                } else {
                    &word[2..]
                };
                if value.is_empty() {
                    return None;
                }
            } else {
                let known = if scp {
                    !word.starts_with("--")
                        && word[1..]
                            .bytes()
                            .all(|ch| matches!(ch, b'q' | b'C' | b'p' | b'r' | b'v'))
                } else {
                    matches!(
                        word.as_str(),
                        "--archive" | "--recursive" | "--verbose" | "--compress" | "--quiet"
                    ) || !word.starts_with("--")
                        && word[1..]
                            .bytes()
                            .all(|ch| matches!(ch, b'a' | b'r' | b'v' | b'z' | b'q' | b'p' | b't'))
                };
                if !known {
                    return None;
                }
            }
        } else {
            // Do not reinterpret unknown option-looking operands after the first file.
            if !terminated && word.starts_with('-') && word != "-" {
                return None;
            }
            options = false;
            operands.push(word.as_str());
        }
        index += 1;
    }
    if operands.len() < 2 {
        return None;
    }
    let destination = ssh_destination(operands.pop()?, number)?;
    if !operands.iter().all(|source| local_source(source)) {
        return None;
    }
    Some(
        operands
            .into_iter()
            .map(|source| Transfer {
                source: source.into(),
                destination: destination.clone(),
            })
            .collect(),
    )
}

fn http_destination(value: &str) -> Option<String> {
    let mut url = url::Url::parse(value).ok()?;
    // --post-file is an HTTP request body, not an FTP upload.
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    // The absolute DNS spelling denotes the same loopback host. Keep other
    // destinations unchanged, and leave historical curl extraction untouched.
    if url
        .host_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("localhost."))
    {
        url.set_host(Some("localhost")).ok()?;
    }
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    Some(url.into())
}

fn wget_transfer(words: &[String]) -> Option<Vec<Transfer>> {
    let mut index = 1;
    let mut options = true;
    let mut source = None;
    let mut destination = None;
    let mut output = None;
    while let Some(word) = words.get(index) {
        if options && word == "--" {
            options = false;
            index += 1;
            continue;
        }
        if options && word.starts_with('-') {
            match word.as_str() {
                "-q" | "--quiet" => {}
                "--post-file" => {
                    index += 1;
                    let value = words.get(index)?;
                    if source.is_some() || value.is_empty() || value.starts_with('-') {
                        return None;
                    }
                    source = Some(value.clone());
                }
                "-O" | "--output-document" => {
                    index += 1;
                    let value = words.get(index)?;
                    if value.is_empty() || output.is_some() {
                        return None;
                    }
                    output = Some(value.clone());
                }
                flag if flag.starts_with("--post-file=") => {
                    let value = flag.strip_prefix("--post-file=")?;
                    if source.is_some() || value.is_empty() {
                        return None;
                    }
                    source = Some(value.into());
                }
                flag if flag.starts_with("--output-document=") => {
                    let value = flag.strip_prefix("--output-document=")?;
                    if value.is_empty() || output.is_some() {
                        return None;
                    }
                    output = Some(value.into());
                }
                _ => return None,
            }
        } else {
            if destination.is_some() {
                return None;
            }
            destination = Some(http_destination(word)?);
        }
        index += 1;
    }
    let source = source?;
    if output.as_ref() == Some(&source) {
        return None;
    }
    Some(vec![Transfer {
        source,
        destination: destination?,
    }])
}

fn nc_transfer(segment: &ShellSegment, words: &[String]) -> Option<Vec<Transfer>> {
    if words.len() != 3 {
        return None;
    }
    let destination = endpoint("tcp", &words[1], Some(port(&words[2])?))?;
    // Redirections are ordered; the final known stdin file replaces earlier ones.
    let input = segment.redirections.iter().rev().find(|redirect| {
        redirect.fd == Some(0) || redirect.fd.is_none() && redirect.kind == RedirectionKind::Input
    })?;
    if input.kind != RedirectionKind::Input {
        return None;
    }
    Some(vec![Transfer {
        source: input.target.clone(),
        destination,
    }])
}

pub(crate) fn transfers(segment: &ShellSegment, words: &[String]) -> Vec<Transfer> {
    // An unknown tail can add operands/options or override stdin/destination.
    // Completed earlier segments remain eligible independently.
    if !segment.complete {
        return Vec::new();
    }
    match words.first().map(String::as_str) {
        Some("scp") => ssh_transfers(words, true),
        Some("rsync") => {
            // Explicit RSYNC_RSH is equivalent to an arbitrary -e/--rsh value.
            // Inspect only known assignment words; never read ambient config.
            if segment
                .words
                .iter()
                .take(segment.assignment_prefix)
                .any(|word| word.starts_with("RSYNC_RSH="))
            {
                return Vec::new();
            }
            ssh_transfers(words, false)
        }
        Some("wget") => wget_transfer(words),
        Some("nc") => nc_transfer(segment, words),
        _ => None,
    }
    .unwrap_or_default()
    .into_iter()
    // Shell opens/truncates every output file before invoking the program,
    // even if a later redirection replaces it. Compare exact literals only.
    .filter(|transfer| {
        !segment.redirections.iter().any(|redirect| {
            redirect.kind == RedirectionKind::Output && redirect.target == transfer.source
        })
    })
    .collect()
}
