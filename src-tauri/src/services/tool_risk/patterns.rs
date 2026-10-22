use std::sync::LazyLock;

use regex::{RegexSet, RegexSetBuilder};

use super::detectors::DetectorOutcome;

/// Patterns match normalized path components, never arbitrary command text.
/// Add a separate entry instead of extending a chain of special-case branches.
const SENSITIVE_PATH_PATTERNS: &[&str] = &[
    r"(?:^|/)\.ssh/id_(?:rsa|dsa|ecdsa|ed25519|ecdsa_sk|ed25519_sk)\z",
    r"(?:^|/)\.aws/credentials\z",
    r"(?:^|/)\.config/gcloud(?:/|\z)",
    r"(?:^|/)\.kube/config\z",
    r"(?:^|/)\.docker/config\.json\z",
    r"(?:^|/)\.env(?:\.[^/]*)?\z",
    r"(?:^|/)(?:\.netrc|\.npmrc|\.pypirc|credentials\.json|service-account(?:\.json)?)\z",
    r"\.(?:pem|key|p12|pfx)\z",
];
// These exclusions apply only to the final filename, not any parent directory.
const SENSITIVE_PATH_EXCLUSIONS: &[&str] = &[
    r"\.(?:pub|example)\z",
    r"^\.env\.(?:example|sample|template)",
];
const SECURITY_PATH_PATTERNS: &[&str] = &[
    r"^/etc/(?:sudoers|passwd|shadow|hosts|ssh/sshd_config)\z",
    r"^/etc/(?:sudoers\.d|pam\.d|systemd/system)(?:/|\z)",
    r"^/var/spool/cron(?:/|\z)",
    r"^/etc/(?:crontab|cron\.(?:d|daily|hourly|weekly|monthly))(?:/|\z)",
    r"(?:^|/)library/(?:launchagents|launchdaemons)(?:/|\z)",
    r"(?:^|/)authorized_keys\z",
];

#[derive(Clone, Copy)]
pub(crate) enum PathRule {
    SensitiveRead,
    SecurityConfig,
}

struct PathPatterns {
    sensitive: RegexSet,
    excluded_names: RegexSet,
    security: RegexSet,
}
impl PathPatterns {
    fn compile(
        sensitive: &[&str],
        exclusions: &[&str],
        security: &[&str],
    ) -> Result<Self, regex::Error> {
        let compile = |patterns: &[&str]| {
            RegexSetBuilder::new(patterns)
                .size_limit(1024 * 1024)
                .dfa_size_limit(1024 * 1024)
                .build()
        };
        Ok(Self {
            sensitive: compile(sensitive)?,
            excluded_names: compile(exclusions)?,
            security: compile(security)?,
        })
    }
}

// Compile only once in the background evaluation path; failure stays indeterminate.
static PATH_PATTERNS: LazyLock<Option<PathPatterns>> =
    LazyLock::new(|| {
        match PathPatterns::compile(
            SENSITIVE_PATH_PATTERNS,
            SENSITIVE_PATH_EXCLUSIONS,
            SECURITY_PATH_PATTERNS,
        ) {
            Ok(patterns) => Some(patterns),
            Err(_) => {
                log::error!("[ToolRisk] 内置路径正则初始化失败，路径检查保持未判定");
                None
            }
        }
    });

/// Preserve the old lexical policy: lowercase, remove empty/`.` components, keep `..`.
/// A leading slash is retained to distinguish absolute system paths from relative names.
fn normalized_path(path: &str) -> String {
    let lowercase = path.to_lowercase();
    let components = lowercase
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    if path.starts_with('/') {
        format!("/{components}")
    } else {
        components
    }
}

fn evaluate_path(patterns: Option<&PathPatterns>, rule: PathRule, path: &str) -> DetectorOutcome {
    let Some(patterns) = patterns else {
        return DetectorOutcome::Indeterminate;
    };
    let normalized = normalized_path(path);
    let matched = match rule {
        PathRule::SensitiveRead => {
            let name = normalized.rsplit('/').next().unwrap_or("");
            !patterns.excluded_names.is_match(name) && patterns.sensitive.is_match(&normalized)
        }
        PathRule::SecurityConfig => patterns.security.is_match(&normalized),
    };
    if matched {
        DetectorOutcome::Match
    } else {
        DetectorOutcome::NoMatch
    }
}

pub(crate) fn match_path(rule: PathRule, path: &str) -> DetectorOutcome {
    evaluate_path(PATH_PATTERNS.as_ref(), rule, path)
}

#[cfg(test)]
mod tests {
    use super::super::detectors::DetectorOutcome::{Indeterminate, Match, NoMatch};
    use super::*;

    #[test]
    fn sensitive_path_table_covers_registered_files_and_directories() {
        for path in [
            ".env",
            "/app/.env.production",
            ".ssh/id_rsa",
            ".ssh/id_dsa",
            ".ssh/id_ecdsa",
            ".ssh/id_ed25519",
            ".ssh/id_ecdsa_sk",
            ".ssh/id_ed25519_sk",
            "~/.aws/credentials",
            ".config/gcloud",
            ".config/gcloud/token.json",
            ".kube/config",
            ".docker/config.json",
            ".netrc",
            ".npmrc",
            ".pypirc",
            "credentials.json",
            "service-account",
            "service-account.json",
            "private.pem",
            "private.key",
            "private.p12",
            "private.pfx",
        ] {
            assert_eq!(match_path(PathRule::SensitiveRead, path), Match, "{path}");
        }
    }

    #[test]
    fn sensitive_path_exclusions_apply_only_to_the_target_name() {
        for path in [
            ".ssh/id_rsa.pub",
            ".env.example",
            ".env.sample",
            ".env.template",
            ".env.example-secret",
            ".env.sample.local",
            "private.key.example",
            ".config/gcloud/token.pub",
            ".config/gcloud/example.example",
        ] {
            assert_eq!(match_path(PathRule::SensitiveRead, path), NoMatch, "{path}");
        }
        for path in [
            "/tmp/notes.example/.env",
            "/tmp/public.pub/.ssh/id_ed25519",
            "/tmp/.env.sample/.aws/credentials",
        ] {
            assert_eq!(match_path(PathRule::SensitiveRead, path), Match, "{path}");
        }
    }

    #[test]
    fn sensitive_path_patterns_preserve_component_and_filename_boundaries() {
        for path in [
            "",
            "/",
            "src/main.rs",
            "docs/credentials.json.md",
            "my.env",
            ".ssh/id_rsa.backup",
            ".ssh-extra/id_rsa",
            ".aws/credentials-backup.txt",
            ".config/gcloud-extra/token.json",
            ".kube/configuration",
            ".docker/config.json.md",
            "service-account-notes.md",
            "file.key.txt",
        ] {
            assert_eq!(match_path(PathRule::SensitiveRead, path), NoMatch, "{path}");
        }
    }

    #[test]
    fn path_normalization_keeps_existing_case_dot_and_separator_behavior() {
        for path in [
            ".ENV",
            "./.env",
            "//home///u/.SSH/./ID_RSA/",
            "/x/.AWS//credentials/",
            "/x/.config/./GCLOUD/token.json",
            "/x/../.kube/config",
            "./private.PEM",
        ] {
            assert_eq!(match_path(PathRule::SensitiveRead, path), Match, "{path}");
        }
        // Do not resolve .. or use filesystem knowledge to invent a match.
        for path in [".ssh/../id_rsa", "x/.env/..", " ./private.pem.txt"] {
            assert_eq!(match_path(PathRule::SensitiveRead, path), NoMatch, "{path}");
        }
    }

    #[test]
    fn security_path_table_covers_registered_locations() {
        for path in [
            "/etc/sudoers",
            "/etc/passwd",
            "/etc/shadow",
            "/etc/hosts",
            "/etc/ssh/sshd_config",
            "/etc/sudoers.d",
            "/etc/sudoers.d/user",
            "/etc/pam.d/login",
            "/etc/systemd/system/app.service",
            "/var/spool/cron/user",
            "/etc/crontab",
            "/etc/cron.d/job",
            "/etc/cron.daily/job",
            "/etc/cron.hourly/job",
            "/etc/cron.weekly/job",
            "/etc/cron.monthly/job",
            "~/.ssh/authorized_keys",
            "/Library/LaunchDaemons/app.plist",
            "~/Library/LaunchAgents/app.plist",
            "./Library/LaunchAgents",
            "//ETC///./sudoers/",
            "/etc/./ssh//sshd_config",
        ] {
            assert_eq!(match_path(PathRule::SecurityConfig, path), Match, "{path}");
        }
    }

    #[test]
    fn security_path_patterns_do_not_match_notes_or_relative_system_paths() {
        for path in [
            "",
            "/",
            "etc/sudoers",
            "/app/etc/sudoers",
            "/etc/hosts.backup",
            "/etc/sudoers.d-backup/user",
            "/etc/pam.draft/login",
            "/etc/systemd/system-extra/job",
            "/var/spool/cron-backup/user",
            "/etc/cron.daily-backup/job",
            "/etc/ssh/sshd_config.md",
            "Library/LaunchAgentsNotes/app.plist",
            "Library-extra/LaunchAgents/app.plist",
            "/tmp/authorized_keys-notes.md",
            "/etc/../sudoers",
        ] {
            assert_eq!(
                match_path(PathRule::SecurityConfig, path),
                NoMatch,
                "{path}"
            );
        }
    }

    #[test]
    fn path_regex_tables_compile_and_initialization_failure_is_indeterminate() {
        assert!(PathPatterns::compile(
            SENSITIVE_PATH_PATTERNS,
            SENSITIVE_PATH_EXCLUSIONS,
            SECURITY_PATH_PATTERNS
        )
        .is_ok());
        assert!(
            PathPatterns::compile(&["("], SENSITIVE_PATH_EXCLUSIONS, SECURITY_PATH_PATTERNS)
                .is_err()
        );
        for rule in [PathRule::SensitiveRead, PathRule::SecurityConfig] {
            assert_eq!(evaluate_path(None, rule, ".env"), Indeterminate);
        }
    }
}
