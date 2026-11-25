use crate::discovery::{enriched_path, find_binary};
use crate::truncate_chars;
use anyhow::{Result, bail};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use umisong_core::{ProviderId, ProviderInstallState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallAttempt {
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

/// Plan ordered Unix installation attempts for a provider CLI from discovered system tools.
pub fn plan_install_attempts_unix(
    provider: ProviderId,
    is_macos: bool,
    has_curl: bool,
    bash: Option<PathBuf>,
    sh: Option<PathBuf>,
    npm: Option<PathBuf>,
    brew: Option<PathBuf>,
) -> Vec<InstallAttempt> {
    let mut attempts = Vec::new();
    match provider {
        ProviderId::ClaudeCode => {
            if has_curl {
                if let Some(bash_bin) = bash {
                    attempts.push(InstallAttempt {
                        label: "official installer".into(),
                        program: bash_bin.to_string_lossy().into_owned(),
                        args: vec![
                            "-c".into(),
                            "set -o pipefail; curl -fsSL https://claude.ai/install.sh | bash".into(),
                        ],
                    });
                }
            }
            if let Some(npm_bin) = npm {
                attempts.push(InstallAttempt {
                    label: "npm".into(),
                    program: npm_bin.to_string_lossy().into_owned(),
                    args: vec![
                        "install".into(),
                        "-g".into(),
                        "@anthropic-ai/claude-code".into(),
                    ],
                });
            }
        }
        ProviderId::Antigravity => {
            if has_curl {
                if let Some(bash_bin) = bash {
                    attempts.push(InstallAttempt {
                        label: "official installer".into(),
                        program: bash_bin.to_string_lossy().into_owned(),
                        args: vec![
                            "-c".into(),
                            "set -o pipefail; curl -fsSL https://antigravity.google/cli/install.sh | bash".into(),
                        ],
                    });
                }
            }
            if is_macos {
                if let Some(brew_bin) = brew {
                    attempts.push(InstallAttempt {
                        label: "Homebrew".into(),
                        program: brew_bin.to_string_lossy().into_owned(),
                        args: vec![
                            "install".into(),
                            "--cask".into(),
                            "antigravity-cli".into(),
                        ],
                    });
                }
            }
        }
        ProviderId::Codex => {
            if has_curl {
                if let Some(sh_bin) = sh {
                    attempts.push(InstallAttempt {
                        label: "official installer".into(),
                        program: sh_bin.to_string_lossy().into_owned(),
                        args: vec![
                            "-c".into(),
                            "curl -fsSL https://chatgpt.com/codex/install.sh | sh".into(),
                        ],
                    });
                }
            }
            if let Some(npm_bin) = npm {
                attempts.push(InstallAttempt {
                    label: "npm".into(),
                    program: npm_bin.to_string_lossy().into_owned(),
                    args: vec!["install".into(), "-g".into(), "@openai/codex".into()],
                });
            }
            if is_macos {
                if let Some(brew_bin) = brew {
                    attempts.push(InstallAttempt {
                        label: "Homebrew".into(),
                        program: brew_bin.to_string_lossy().into_owned(),
                        args: vec!["install".into(), "--cask".into(), "codex".into()],
                    });
                }
            }
        }
    }
    attempts
}

#[cfg(unix)]
pub fn plan_install_attempts(provider: ProviderId) -> Vec<InstallAttempt> {
    let has_curl = find_binary("curl", None).is_some();
    let bash = find_binary("bash", None);
    let sh = find_binary("sh", None);
    let npm = find_binary("npm", None);
    let brew = find_binary("brew", None);
    plan_install_attempts_unix(
        provider,
        cfg!(target_os = "macos"),
        has_curl,
        bash,
        sh,
        npm,
        brew,
    )
}

#[cfg(windows)]
pub fn plan_install_attempts(provider: ProviderId) -> Vec<InstallAttempt> {
    let mut attempts = Vec::new();
    let ps = find_binary("powershell.exe", None).or_else(|| find_binary("pwsh.exe", None));
    let npm = find_binary("npm.cmd", None).or_else(|| find_binary("npm", None));

    match provider {
        ProviderId::ClaudeCode => {
            if let Some(ps_bin) = ps {
                attempts.push(InstallAttempt {
                    label: "official installer".into(),
                    program: ps_bin.to_string_lossy().into_owned(),
                    args: vec![
                        "-NoProfile".into(),
                        "-ExecutionPolicy".into(),
                        "Bypass".into(),
                        "-Command".into(),
                        "irm https://claude.ai/install.ps1 | iex".into(),
                    ],
                });
            }
            if let Some(npm_bin) = npm {
                attempts.push(InstallAttempt {
                    label: "npm".into(),
                    program: npm_bin.to_string_lossy().into_owned(),
                    args: vec![
                        "install".into(),
                        "-g".into(),
                        "@anthropic-ai/claude-code".into(),
                    ],
                });
            }
        }
        ProviderId::Antigravity => {
            if let Some(ps_bin) = ps {
                attempts.push(InstallAttempt {
                    label: "official installer".into(),
                    program: ps_bin.to_string_lossy().into_owned(),
                    args: vec![
                        "-NoProfile".into(),
                        "-ExecutionPolicy".into(),
                        "Bypass".into(),
                        "-Command".into(),
                        "irm https://antigravity.google/cli/install.ps1 | iex".into(),
                    ],
                });
            }
        }
        ProviderId::Codex => {
            if let Some(ps_bin) = ps {
                attempts.push(InstallAttempt {
                    label: "official installer".into(),
                    program: ps_bin.to_string_lossy().into_owned(),
                    args: vec![
                        "-NoProfile".into(),
                        "-ExecutionPolicy".into(),
                        "Bypass".into(),
                        "-Command".into(),
                        "irm https://chatgpt.com/codex/install.ps1 | iex".into(),
                    ],
                });
            }
            if let Some(npm_bin) = npm {
                attempts.push(InstallAttempt {
                    label: "npm".into(),
                    program: npm_bin.to_string_lossy().into_owned(),
                    args: vec!["install".into(), "-g".into(), "@openai/codex".into()],
                });
            }
        }
    }
    attempts
}

#[cfg(not(any(unix, windows)))]
pub fn plan_install_attempts(_provider: ProviderId) -> Vec<InstallAttempt> {
    Vec::new()
}

/// Install a provider CLI using its official installer and package-manager fallbacks.
pub async fn install_provider_cli(
    provider: ProviderId,
    on_progress: &(dyn Fn(ProviderInstallState) + Send + Sync),
) -> Result<PathBuf> {
    if let Some(existing) = find_binary(provider.cli_binary_name(), None) {
        return Ok(existing);
    }

    let attempts = plan_install_attempts(provider);
    if attempts.is_empty() {
        bail!(
            "No supported installer tool found (curl, npm, or brew). Run manually: {}",
            provider.install_hint()
        );
    }

    let mut last_error = String::new();

    for attempt in attempts {
        on_progress(ProviderInstallState::Installing {
            method: attempt.label.clone(),
            detail: format!(
                "Installing {} via {}…",
                provider.display_name(),
                attempt.label
            ),
        });

        let fut = Command::new(&attempt.program)
            .args(&attempt.args)
            .env("PATH", enriched_path())
            .env("CI", "1")
            .env("NONINTERACTIVE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        match tokio::time::timeout(Duration::from_secs(180), fut).await {
            Ok(Ok(out)) => {
                if out.status.success() {
                    if let Some(bin) = find_binary(provider.cli_binary_name(), None) {
                        return Ok(bin);
                    }
                    last_error = format!(
                        "{} finished but `{}` was not found in PATH",
                        attempt.label,
                        provider.cli_binary_name()
                    );
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let msg = stderr
                        .lines()
                        .chain(stdout.lines())
                        .map(str::trim)
                        .rfind(|l| !l.is_empty())
                        .map(|l| truncate_chars(l, 160))
                        .unwrap_or_else(|| format!("exit code {:?}", out.status.code()));
                    last_error = format!("{} failed: {msg}", attempt.label);
                }
            }
            Ok(Err(err)) => {
                last_error = format!("{} failed to launch: {err}", attempt.label);
            }
            Err(_) => {
                last_error = format!("{} timed out after 180s", attempt.label);
            }
        }
    }

    bail!(
        "{last_error}. You can also run manually: {}",
        provider.install_hint()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_official_then_fallbacks_when_all_tools_present_on_macos() {
        let bash = Some(PathBuf::from("/bin/bash"));
        let sh = Some(PathBuf::from("/bin/sh"));
        let npm = Some(PathBuf::from("/usr/local/bin/npm"));
        let brew = Some(PathBuf::from("/opt/homebrew/bin/brew"));

        let claude = plan_install_attempts_unix(
            ProviderId::ClaudeCode,
            true,
            true,
            bash.clone(),
            sh.clone(),
            npm.clone(),
            brew.clone(),
        );
        assert_eq!(claude.len(), 2);
        assert_eq!(claude[0].label, "official installer");
        assert_eq!(claude[1].label, "npm");

        let agy = plan_install_attempts_unix(
            ProviderId::Antigravity,
            true,
            true,
            bash.clone(),
            sh.clone(),
            npm.clone(),
            brew.clone(),
        );
        assert_eq!(agy.len(), 2);
        assert_eq!(agy[0].label, "official installer");
        assert_eq!(agy[1].label, "Homebrew");

        let codex = plan_install_attempts_unix(
            ProviderId::Codex,
            true,
            true,
            bash,
            sh,
            npm,
            brew,
        );
        assert_eq!(codex.len(), 3);
        assert_eq!(codex[0].label, "official installer");
        assert_eq!(codex[1].label, "npm");
        assert_eq!(codex[2].label, "Homebrew");
    }

    #[test]
    fn falls_back_to_npm_and_skips_brew_on_linux_without_curl() {
        let npm = Some(PathBuf::from("/usr/bin/npm"));
        let brew = Some(PathBuf::from("/home/linuxbrew/.linuxbrew/bin/brew"));

        let claude = plan_install_attempts_unix(
            ProviderId::ClaudeCode,
            false,
            false,
            Some(PathBuf::from("/bin/bash")),
            Some(PathBuf::from("/bin/sh")),
            npm.clone(),
            brew.clone(),
        );
        assert_eq!(claude.len(), 1);
        assert_eq!(claude[0].label, "npm");

        let agy = plan_install_attempts_unix(
            ProviderId::Antigravity,
            false,
            false,
            Some(PathBuf::from("/bin/bash")),
            Some(PathBuf::from("/bin/sh")),
            npm,
            brew,
        );
        assert!(agy.is_empty());
    }

    #[test]
    fn returns_empty_when_no_tools_available() {
        for pid in ProviderId::ALL {
            let attempts = plan_install_attempts_unix(pid, true, false, None, None, None, None);
            assert!(attempts.is_empty());
        }
    }
}
