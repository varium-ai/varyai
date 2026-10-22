//! Finite argv semantics shared by shell classification and operation extraction.

/// Internal facts only; raw argv and paths must not be formatted into diagnostics.
pub(crate) struct CommandSemantics {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub mutates: bool,
    complete: bool,
    certifiable: bool,
}
impl CommandSemantics {
    pub(crate) fn read_only(&self) -> bool {
        self.complete && self.certifiable && !self.mutates && self.writes.is_empty()
    }
    fn read(&mut self, path: &str) {
        if !path.is_empty() && path != "-" {
            self.reads.push(path.into());
        }
    }
    fn write(&mut self, path: &str) {
        self.mutates = true;
        if !path.is_empty() && path != "-" {
            self.writes.push(path.into());
        }
    }
}

fn decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|ch| ch.is_ascii_digit())
}
fn assignment(value: &str) -> bool {
    value.split_once('=').is_some_and(|(name, _)| {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}
fn print_script(script: &str) -> bool {
    let Some(address) = script.strip_suffix('p') else {
        return false;
    };
    let positive = |value: &str| decimal(value) && value.bytes().any(|ch| ch != b'0');
    address.is_empty()
        || address.split_once(',').map_or_else(
            || positive(address),
            |(start, end)| positive(start) && positive(end),
        )
}

// This only labels a finite write form; it never extracts a script path.
fn write_script(script: &str) -> bool {
    if script.starts_with("w ") || script.starts_with("w\t") {
        return true;
    }
    let mut chars = script.chars();
    if chars.next() != Some('s') {
        return false;
    }
    let Some(delimiter) = chars
        .next()
        .filter(|ch| !ch.is_alphanumeric() && !ch.is_whitespace() && *ch != '\\')
    else {
        return false;
    };
    let mut escaped = false;
    let mut fields = 0;
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == delimiter {
            fields += 1;
            if fields == 2 {
                while let Some(flag) = chars.next() {
                    if flag == 'w' {
                        return chars.next().is_some_and(|ch| matches!(ch, ' ' | '\t'));
                    }
                    if !matches!(flag, 'g' | 'p' | 'i' | 'I' | 'm' | 'M' | '0'..='9') {
                        return false;
                    }
                }
                return false;
            }
        }
    }
    false
}

// grep's explicit pattern flags may occur after ordinary file operands. Only scan
// the understood prefix, respecting option values and --; never inspect a tail
// following an unknown flag as if its argument roles were known.
fn grep_explicit_pattern(args: &[String]) -> bool {
    let mut index = 0;
    let mut explicit = false;
    while let Some(word) = args.get(index) {
        match word.as_str() {
            "--" => break,
            "-e" | "-f" => {
                let Some(value) = args.get(index + 1) else {
                    break;
                };
                if word == "-f" && value.is_empty() {
                    break;
                }
                explicit = true;
                index += 2;
                continue;
            }
            "-c" | "-n" | "-v" | "-i" | "-l" | "-q" => {}
            flag if flag.starts_with('-') && flag != "-" => break,
            _ => {}
        }
        index += 1;
    }
    explicit
}

/// Recognize only the finite forms in the Phase 2 contract. None means unmanaged.
pub(crate) fn analyze(words: &[String]) -> Option<CommandSemantics> {
    let program = words.first()?.as_str();
    if !matches!(
        program,
        "head"
            | "tail"
            | "less"
            | "more"
            | "strings"
            | "base64"
            | "xxd"
            | "od"
            | "grep"
            | "awk"
            | "sed"
            | "dd"
    ) {
        return None;
    }
    let mut result = CommandSemantics {
        reads: vec![],
        writes: vec![],
        mutates: false,
        complete: true,
        certifiable: !matches!(program, "less" | "more" | "awk"),
    };
    let args = &words[1..];
    if program == "dd" {
        let mut input = false;
        let mut seen = Vec::new();
        for word in args {
            let Some((key, value)) = word.split_once('=') else {
                result.complete = false;
                continue;
            };
            if seen.contains(&key) {
                result.complete = false;
            }
            seen.push(key);
            match key {
                "if" => {
                    input = !value.is_empty();
                    result.reads.clear();
                    result.read(value);
                }
                "of" => {
                    result.certifiable = false;
                    result.writes.clear();
                    result.write(value);
                }
                "bs" | "count" | "skip" if decimal(value) => {}
                _ => result.complete = false,
            }
            if value.is_empty() {
                result.complete = false;
            }
        }
        result.complete &= input;
        return Some(result);
    }
    let mut index = 0;
    let mut options = true;
    let mut pattern = program == "grep" && grep_explicit_pattern(args);
    let mut sed_quiet = false;
    let mut file_count = 0;
    while let Some(word) = args.get(index) {
        if options && word == "--" {
            options = false;
            index += 1;
            continue;
        }
        if options && word.starts_with('-') && word != "-" {
            let flag = word.as_str();
            if program == "sed"
                && (flag == "-i"
                    || flag.starts_with("-i.")
                    || flag == "--in-place"
                    || flag.starts_with("--in-place="))
            {
                result.mutates = true;
                result.complete = false;
                break;
            }
            let no_value = match program {
                "head" | "tail" => matches!(flag, "-q" | "-v"),
                "strings" => flag == "-a",
                "base64" => matches!(flag, "-d" | "--decode"),
                "xxd" => flag == "-p",
                "grep" => matches!(flag, "-c" | "-n" | "-v" | "-i" | "-l" | "-q"),
                "sed" => flag == "-n",
                _ => false,
            };
            if no_value {
                if program == "sed" {
                    sed_quiet = true;
                }
                index += 1;
                continue;
            }
            if matches!(program, "head" | "tail") {
                if flag.strip_prefix('-').is_some_and(decimal) {
                    index += 1;
                    continue;
                }
                if let Some(value) = flag
                    .strip_prefix("--lines=")
                    .or_else(|| flag.strip_prefix("--bytes="))
                {
                    if !decimal(value) {
                        result.complete = false;
                        break;
                    }
                    index += 1;
                    continue;
                }
            }
            let takes_value = match program {
                "head" | "tail" => matches!(flag, "-n" | "-c" | "--lines" | "--bytes"),
                "strings" => flag == "-n",
                "base64" => matches!(flag, "-i" | "-o"),
                "xxd" => matches!(flag, "-g" | "-l" | "-s"),
                "od" => matches!(flag, "-A" | "-t" | "-N" | "-j"),
                "grep" => matches!(flag, "-e" | "-f"),
                "awk" => matches!(flag, "-v" | "-f"),
                "sed" => flag == "-e" && !pattern,
                _ => false,
            };
            if !takes_value {
                result.complete = false;
                break;
            }
            let Some(value) = args.get(index + 1) else {
                result.complete = false;
                break;
            };
            let valid = match program {
                "head" | "tail" | "strings" => decimal(value),
                "xxd" => {
                    decimal(value)
                        || flag == "-s"
                            && value.strip_prefix("0x").is_some_and(|v| {
                                !v.is_empty() && v.bytes().all(|ch| ch.is_ascii_hexdigit())
                            })
                }
                "od" => match flag {
                    "-A" => matches!(value.as_str(), "d" | "o" | "x" | "n"),
                    "-t" => matches!(
                        value.as_str(),
                        "a" | "c"
                            | "d"
                            | "o"
                            | "u"
                            | "x"
                            | "d1"
                            | "d2"
                            | "d4"
                            | "d8"
                            | "o1"
                            | "o2"
                            | "o4"
                            | "o8"
                            | "u1"
                            | "u2"
                            | "u4"
                            | "u8"
                            | "x1"
                            | "x2"
                            | "x4"
                            | "x8"
                            | "f4"
                            | "f8"
                    ),
                    _ => decimal(value),
                },
                "awk" if flag == "-v" => assignment(value),
                "sed" => {
                    result.mutates |= write_script(value);
                    print_script(value)
                }
                "grep" if flag == "-e" => true,
                _ => !value.is_empty() && (!value.starts_with('-') || value == "-" || flag == "-e"),
            };
            if !valid {
                result.complete = false;
                break;
            }
            match (program, flag) {
                ("base64", "-i") => {
                    result.reads.clear();
                    result.read(value);
                }
                ("grep" | "awk", "-f") => result.read(value),
                ("base64", "-o") => {
                    result.writes.clear();
                    result.write(value);
                }
                _ => {}
            }
            if matches!(program, "grep" | "awk") && matches!(flag, "-e" | "-f") || program == "sed"
            {
                pattern = true;
            }
            index += 2;
            continue;
        }
        // awk stops option processing at its first ordinary argument.
        if program == "awk" {
            options = false;
        }
        if matches!(program, "grep" | "awk" | "sed") && !pattern {
            if program == "sed" && !print_script(word) {
                result.mutates |= write_script(word);
                result.complete = false;
                break;
            }
            pattern = true;
        } else if program == "awk" && assignment(word) {
            // A valid awk assignment is not a path operand.
        } else if program == "xxd" {
            options = false;
            match file_count {
                0 => result.read(word),
                1 => result.write(word),
                _ => {
                    result.complete = false;
                    break;
                }
            }
            file_count += 1;
        } else {
            result.read(word);
        }
        index += 1;
    }
    if matches!(program, "grep" | "awk" | "sed") {
        result.complete &= pattern;
    }
    if program == "sed" {
        result.complete &= sed_quiet;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn semantics(command: &str) -> CommandSemantics {
        let parsed = super::super::shell::parse(command);
        analyze(&parsed.segments[0].words).expect("finite program")
    }
    #[test]
    fn sed_only_certifies_the_complete_print_subset() {
        for command in [
            "sed -n p README",
            "sed -n '1p' README",
            "sed -n -e '1,200p' README",
        ] {
            let result = semantics(command);
            assert!(result.read_only());
            assert_eq!(result.reads, ["README"]);
        }
        for command in [
            "sed p README",
            "sed -n '0p' README",
            "sed -n 'w out' README",
            "sed -n 's/a/b/w out' README",
            "sed -n -e p -e w README",
            "sed -f external README",
        ] {
            assert!(!semantics(command).read_only(), "{command}");
        }
    }
    #[test]
    fn dd_keeps_known_roles_without_certifying_unknown_operands() {
        let result = semantics("dd if=input of=output unknown=1");
        assert_eq!(result.reads, ["input"]);
        assert_eq!(result.writes, ["output"]);
        assert!(!result.read_only());
        assert!(semantics("dd if=input bs=1 count=2 skip=0").read_only());
        assert!(!semantics("dd bs=1").read_only());
    }
    #[test]
    fn option_values_and_patterns_have_explicit_roles() {
        assert_eq!(semantics("grep -e .env README").reads, ["README"]);
        assert_eq!(
            semantics("awk -v x=.env '{print}' README").reads,
            ["README"]
        );
        assert!(!semantics("awk '{print}' README").read_only());
        assert!(semantics("head -n .env README").reads.is_empty());
        assert!(semantics("head --unknown .env").reads.is_empty());
    }
}

#[cfg(test)]
mod phase2_quality_tests {
    use super::*;
    fn semantics(command: &str) -> CommandSemantics {
        let parsed = super::super::shell::parse(command);
        analyze(&parsed.segments[0].words).expect("managed program")
    }
    #[test]
    fn unknown_sed_scripts_are_not_automatically_mutations() {
        for command in [
            "sed -n -e '-i' README",
            "sed -f script.sed README",
            "sed -n 's/-i/x/' README",
        ] {
            let result = semantics(command);
            assert!(!result.mutates, "{command}");
            assert!(!result.read_only(), "{command}");
        }
        for command in [
            "sed -i p README",
            "sed -i.bak p README",
            "sed --in-place=.bak p README",
            "sed -n 'w out' README",
            "sed 's/a/b/w out' README",
        ] {
            assert!(semantics(command).mutates, "{command}");
        }
    }
    #[test]
    fn unknown_semantics_cannot_be_whitelisted_into_complete_coverage() {
        use super::super::classifier::*;
        use super::super::types::*;
        for command in [
            "head --unknown README",
            "awk '{print}' README",
            "sed -f script.sed README",
        ] {
            let list = CommandListEntry {
                id: "phase2-unknown-white".into(),
                revision: 1,
                list_type: CommandListType::Whitelist,
                match_kind: CommandMatchKind::Exact,
                pattern: command.into(),
                enabled: true,
                severity: Severity::None,
                risk_category: RiskCategory::SensitiveRead,
            };
            assert!(!classify_command(command, &[list]).complete, "{command}");
        }
    }
}

#[cfg(test)]
mod phase2_format_edge_tests {
    use super::*;
    fn semantics(command: &str) -> CommandSemantics {
        let parsed = super::super::shell::parse(command);
        analyze(&parsed.segments[0].words).expect("managed program")
    }
    #[test]
    fn od_type_values_require_a_finite_format_not_any_alphanumeric_word() {
        for command in [
            "od -t credentials README",
            "od -t x999 README",
            "od -t invalid .env",
        ] {
            let result = semantics(command);
            assert!(!result.read_only(), "{command}");
            assert!(result.reads.is_empty(), "{command}");
        }
        for command in [
            "od -t x1 README",
            "od -t d2 README",
            "od -t c README",
            "od -t f8 README",
        ] {
            assert!(semantics(command).read_only(), "{command}");
        }
    }
    #[test]
    fn substitution_pattern_text_is_not_a_write_flag() {
        for command in [
            "sed -n 's/w text/x/' README",
            "sed -n 's/a/w text/' README",
            "sed -n 's/a/b\\/w text/' README",
        ] {
            assert!(!semantics(command).mutates, "{command}");
        }
        for command in ["sed -n 's/a/b/w out' README", "sed -n 's#a#b#w out' README"] {
            assert!(semantics(command).mutates, "{command}");
        }
    }
}
