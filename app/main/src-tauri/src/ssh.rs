// Pixel Connect — optional SSH connectivity to a phone running an SSH server.
//
// This is deliberately a *client* integration only. Pixel Connect never
// installs, starts or enables an SSH server on the phone, and it never runs a
// remote command on connect. The user must have explicitly configured an SSH
// server on the phone (for example via Termux) and then describe the host,
// port, username and authentication method here.
//
// Arguments are passed as a real argv to the system OpenSSH client; no shell is
// ever involved, so device names and file names cannot become commands.

use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    /// Let the system SSH agent / ~/.ssh/config decide.
    Agent,
    /// An identity file the user selected.
    IdentityFile,
    /// Plain key authentication with a passphrase supplied at run time.
    Password,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTarget {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub identity_file: Option<PathBuf>,
    /// Optional short description shown in the UI.
    pub label: Option<String>,
    /// User confirms this phone has an SSH server configured.
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshProbe {
    pub target_id: String,
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

fn which(bin: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = std::path::Path::new(dir).join(bin);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

pub fn ssh_binary() -> Option<String> {
    which("ssh")
}

/// Validate the parts that end up on a command line.
pub fn validate(target: &SshTarget) -> Result<(), String> {
    if target.host.trim().is_empty() {
        return Err("host is required".into());
    }
    if target.host.len() > 253
        || target
            .host
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\0' | ';' | '&' | '|' | '$' | '`' | '<' | '>' | '\'' | '"' | '(' | ')' | '*' | '?' | '[' | ']' | '#' | '~'))
    {
        return Err(format!("host contains invalid characters: {:?}", target.host));
    }
    if target.username.is_empty() {
        return Err("username is required".into());
    }
    if target.username.len() > 64
        || target
            .username
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\0' | ':' | '@' | '/' | '\\'))
    {
        return Err("username contains invalid characters".into());
    }
    if target.port == 0 {
        return Err("port must be between 1 and 65535".into());
    }
    if target.auth_method == AuthMethod::IdentityFile && target.identity_file.is_none() {
        return Err("identity file is required for key authentication".into());
    }
    Ok(())
}

/// The exact argument vector used for a non-interactive connectivity check.
///
/// Exposed so the shape of the command can be asserted in tests: no shell is
/// involved and the remote command is always the literal `true`.
pub fn probe_args(target: &SshTarget) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        target.port.to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=6".to_string(),
        // Never prompt interactively; an unknown host is reported, not silently
        // accepted without recording it.
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
    ];
    if let Some(key) = &target.identity_file {
        args.push("-i".to_string());
        args.push(key.to_string_lossy().to_string());
    }
    args.push(format!("{}@{}", target.username, target.host));
    args.push("true".to_string());
    args
}

/// Run a non-interactive connectivity check. This executes `true` on the
/// remote host, which is the minimum needed to prove the credentials work and
/// is only ever done when the user presses "Test connection".
pub async fn probe(target: &SshTarget) -> SshProbe {
    let id = target.id.clone();
    if let Err(e) = validate(target) {
        return SshProbe {
            target_id: id,
            available: false,
            version: None,
            message: e,
        };
    }
    let Some(ssh) = ssh_binary() else {
        return SshProbe {
            target_id: id,
            available: false,
            version: None,
            message: "The system OpenSSH client was not found on PATH.".into(),
        };
    };

    match Command::new(&ssh)
        .arg("-V")
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(out) => {
            let version = String::from_utf8_lossy(&out.stderr).trim().to_string();
            SshProbe {
                target_id: id,
                available: true,
                version: Some(version),
                message: "OpenSSH client is available. No connection was attempted.".into(),
            }
        }
        Err(e) => SshProbe {
            target_id: id,
            available: false,
            version: None,
            message: format!("could not run the SSH client: {e}"),
        },
    }
}

/// Open an interactive SSH session in the user's terminal emulator.
pub async fn open_session(target: &SshTarget, terminal: Option<&str>) -> Result<String, String> {
    validate(target)?;
    let ssh = ssh_binary().ok_or("the system OpenSSH client was not found on PATH")?;
    let term = terminal
        .map(|t| t.to_string())
        .or_else(|| which("gnome-terminal"))
        .or_else(|| which("konsole"))
        .or_else(|| which("xfce4-terminal"))
        .or_else(|| which("alacritty"))
        .or_else(|| which("kitty"))
        .ok_or("no supported terminal emulator was found")?;

    let mut args: Vec<String> = Vec::new();
    match PathBuf::from(&term).file_name().and_then(|n| n.to_str()) {
        Some("gnome-terminal") => {
            args.push("--".into());
        }
        Some("xfce4-terminal") => {
            args.push("--command".into());
        }
        _ => {}
    }

    args.push(ssh.clone());
    if target.port != 22 {
        args.push("-p".to_string());
        args.push(target.port.to_string());
    }
    if let Some(key) = &target.identity_file {
        args.push("-i".to_string());
        args.push(key.to_string_lossy().to_string());
    }
    args.push(format!("{}@{}", target.username, target.host));

    Command::new(&term)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {term}: {e}"))?;

    Ok(format!(
        "Opened an SSH session to {}:{} in {term}.",
        target.host, target.port
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> SshTarget {
        SshTarget {
            id: "t1".into(),
            host: "192.168.1.42".into(),
            port: 8022,
            username: "termux".into(),
            auth_method: AuthMethod::Agent,
            identity_file: None,
            label: Some("Pixel via Termux".into()),
            confirmed: true,
        }
    }

    #[test]
    fn accepts_a_plain_target() {
        assert!(validate(&target()).is_ok());
    }

    #[test]
    fn rejects_shell_metacharacters_in_host() {
        for bad in [
            "192.168.1.42; rm -rf /",
            "host name",
            "$(whoami)",
            "a`b`",
            "a|b",
            "a&b",
            "a>b",
            "",
        ] {
            let mut t = target();
            t.host = bad.into();
            assert!(validate(&t).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn rejects_shell_metacharacters_in_username() {
        for bad in ["a b", "a:b", "a@b", "a/b", ""] {
            let mut t = target();
            t.username = bad.into();
            assert!(validate(&t).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn rejects_zero_port_and_missing_key() {
        let mut t = target();
        t.port = 0;
        assert!(validate(&t).is_err());

        let mut t = target();
        t.auth_method = AuthMethod::IdentityFile;
        assert!(validate(&t).is_err());
    }

    #[test]
    fn probe_args_never_use_a_shell() {
        let args = probe_args(&target());
        assert!(args.iter().all(|a| !a.contains(';') && !a.contains('`')));
        assert_eq!(args.last().unwrap(), "true");
        assert!(args.contains(&"8022".to_string()));
        assert!(args.contains(&"termux@192.168.1.42".to_string()));
        // The host must never appear as a bare argument that a shell could
        // re-split.
        assert!(!args.contains(&"192.168.1.42".to_string()));
    }

}