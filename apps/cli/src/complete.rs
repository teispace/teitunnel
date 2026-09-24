//! Live shell completion. `teitunnel completions <shell>` prints a small script that
//! asks `teitunnel __complete` for candidates on every Tab: subcommands and flags from
//! the command line definition, and names (hostnames, tunnels, domains, shares,
//! accounts) from the local database, read-only and without the network
//! (`teitunnel_core::completion`). clap's own dynamic completion is still unstable
//! (`clap_complete` `unstable-dynamic`), so this is the stable equivalent.
//!
//! Each candidate is printed on its own line as `value<TAB>description`.

use clap::{Arg, ArgAction, Command};
use teitunnel_core::completion::Candidates;

/// Shells with a live completion script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Shell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    Powershell,
    Elvish,
}

impl Shell {
    fn static_shell(self) -> clap_complete::Shell {
        match self {
            Self::Bash => clap_complete::Shell::Bash,
            Self::Zsh => clap_complete::Shell::Zsh,
            Self::Fish => clap_complete::Shell::Fish,
            Self::Powershell => clap_complete::Shell::PowerShell,
            Self::Elvish => clap_complete::Shell::Elvish,
        }
    }
}

const BASH: &str = r#"# teitunnel live completion for bash. Add to ~/.bashrc:
#   eval "$(teitunnel completions bash)"
_teitunnel() {
    local line
    COMPREPLY=()
    while IFS= read -r line; do
        [ -n "$line" ] && COMPREPLY+=("${line%%$'\t'*}")
    done < <(teitunnel __complete bash "$COMP_CWORD" -- "${COMP_WORDS[@]}" 2>/dev/null)
}
complete -o default -F _teitunnel teitunnel
"#;

const ZSH: &str = r#"#compdef teitunnel
# teitunnel live completion for zsh. Add to ~/.zshrc:
#   source <(teitunnel completions zsh)
# or save it as _teitunnel in a folder on your $fpath.
_teitunnel() {
    local -a lines described
    local line value
    lines=("${(@f)$(teitunnel __complete zsh $((CURRENT - 1)) -- "${words[@]}" 2>/dev/null)}")
    for line in $lines; do
        [[ -z $line ]] && continue
        value=${line%%$'\t'*}
        value=${value//:/\\:}
        if [[ $line == *$'\t'* ]]; then
            described+=("$value:${line#*$'\t'}")
        else
            described+=("$value")
        fi
    done
    _describe -t values teitunnel described
}
if [ "$funcstack[1]" = "_teitunnel" ]; then
    _teitunnel "$@"
else
    compdef _teitunnel teitunnel
fi
"#;

const FISH: &str = r#"# teitunnel live completion for fish. Save as
# ~/.config/fish/completions/teitunnel.fish, or run:
#   teitunnel completions fish | source
function __teitunnel_complete
    set -l tokens (commandline -opc)
    teitunnel __complete fish (count $tokens) -- $tokens (commandline -ct) 2>/dev/null
end
complete -c teitunnel -f -a '(__teitunnel_complete)'
"#;

const POWERSHELL: &str = r#"# teitunnel live completion for PowerShell. Add to your $PROFILE:
#   teitunnel completions powershell | Out-String | Invoke-Expression
Register-ArgumentCompleter -Native -CommandName teitunnel -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $words = @($commandAst.CommandElements |
        Where-Object { $_.Extent.StartOffset -lt $cursorPosition } |
        ForEach-Object { $_.ToString() })
    if ($wordToComplete -eq '') { $words += '' }
    $index = $words.Count - 1
    teitunnel __complete powershell $index -- @words 2>$null | ForEach-Object {
        $value, $description = $_ -split "`t", 2
        if (-not $description) { $description = $value }
        [System.Management.Automation.CompletionResult]::new($value, $value, 'ParameterValue', $description)
    }
}
"#;

/// Prints the completion script for `shell` (`static_script`: clap's, with commands
/// and flags only, which never runs `teitunnel`).
pub(crate) fn script(shell: Shell, static_script: bool, command: &mut Command) -> String {
    let live = match shell {
        _ if static_script => None,
        Shell::Bash => Some(BASH),
        Shell::Zsh => Some(ZSH),
        Shell::Fish => Some(FISH),
        Shell::Powershell => Some(POWERSHELL),
        // Elvish gets clap's static script.
        Shell::Elvish => None,
    };
    if let Some(live) = live {
        return live.to_owned();
    }
    let mut out = Vec::new();
    clap_complete::generate(shell.static_shell(), command, "teitunnel", &mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// A candidate and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) value: String,
    pub(crate) help: Option<String>,
}

impl Candidate {
    fn new(value: impl Into<String>, help: Option<String>) -> Self {
        Self {
            value: value.into(),
            help: help.map(|h| h.lines().next().unwrap_or_default().to_owned()),
        }
    }

    /// The line printed for the shell.
    pub(crate) fn line(&self) -> String {
        match &self.help {
            Some(help) if !help.is_empty() => format!("{}\t{help}", self.value),
            _ => self.value.clone(),
        }
    }
}

/// Where values for an argument come from.
fn names_for(command: &str, arg: &str, current: &str, names: &Candidates) -> Vec<String> {
    match (command, arg) {
        (_, "account") => names.accounts.clone(),
        (_, "tunnel") | ("delete" | "adopt", "name") => names.tunnels.clone(),
        ("shares", "stop") => names.shares.clone(),
        // A new hostname: the label typed so far on each known domain.
        ("add", "hostname") | ("share", "on") => new_hostnames(current, &names.domains),
        (_, "hostname") => names.hostnames.clone(),
        _ => Vec::new(),
    }
}

fn new_hostnames(current: &str, domains: &[String]) -> Vec<String> {
    let Some((label, rest)) = current.split_once('.') else {
        return if current.is_empty() {
            domains.to_vec()
        } else {
            domains.iter().map(|d| format!("{current}.{d}")).collect()
        };
    };
    domains
        .iter()
        .filter(|d| d.starts_with(rest))
        .map(|d| format!("{label}.{d}"))
        .collect()
}

fn takes_value(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Set | ArgAction::Append)
}

fn option<'a>(command: &'a Command, word: &str) -> Option<&'a Arg> {
    if let Some(long) = word.strip_prefix("--") {
        command.get_arguments().find(|a| a.get_long() == Some(long))
    } else if let Some(short) = word.strip_prefix('-')
        && short.chars().count() == 1
    {
        let short = short.chars().next()?;
        command
            .get_arguments()
            .find(|a| a.get_short() == Some(short))
    } else {
        None
    }
}

fn values(arg: &Arg, command: &str, current: &str, names: &Candidates) -> Vec<Candidate> {
    let possible = arg.get_possible_values();
    if !possible.is_empty() {
        return possible
            .iter()
            .filter(|v| !v.is_hide_set())
            .map(|v| Candidate::new(v.get_name(), v.get_help().map(ToString::to_string)))
            .collect();
    }
    names_for(command, arg.get_id().as_str(), current, names)
        .into_iter()
        .map(|v| Candidate::new(v, None))
        .collect()
}

/// The candidates for `words[index]` (`words[0]` is the program).
pub(crate) fn complete(
    root: &Command,
    words: &[String],
    index: usize,
    names: &Candidates,
) -> Vec<Candidate> {
    let current = words.get(index).map_or("", String::as_str);
    let mut command = root;
    let mut positionals = 0usize;
    let mut i = 1;
    while i < index {
        let word = words.get(i).map_or("", String::as_str);
        if let Some(sub) = command.find_subcommand(word).filter(|s| !s.is_hide_set()) {
            command = sub;
            positionals = 0;
        } else if word.starts_with('-') {
            if !word.contains('=') && option(command, word).is_some_and(takes_value) {
                i += 1;
            }
        } else {
            positionals += 1;
        }
        i += 1;
    }
    let name = command.get_name();
    let previous = index
        .checked_sub(1)
        .filter(|&p| p > 0)
        .and_then(|p| words.get(p))
        .map_or("", String::as_str);
    let mut found: Vec<Candidate> = if let Some(arg) =
        option(command, previous).filter(|a| takes_value(a) && previous.starts_with('-'))
    {
        values(arg, name, current, names)
    } else if let Some((flag, value)) = current.split_once('=')
        && flag.starts_with("--")
        && let Some(arg) = option(command, flag).filter(|a| takes_value(a))
    {
        values(arg, name, value, names)
            .into_iter()
            .map(|c| Candidate::new(format!("{flag}={}", c.value), c.help))
            .collect()
    } else if current.starts_with('-') {
        command
            .get_arguments()
            .filter(|a| !a.is_hide_set())
            .filter_map(|a| {
                a.get_long().map(|l| {
                    Candidate::new(format!("--{l}"), a.get_help().map(ToString::to_string))
                })
            })
            .chain(std::iter::once(Candidate::new(
                "--help",
                Some("Print help".into()),
            )))
            .collect()
    } else {
        let subcommands = command
            .get_subcommands()
            .filter(|s| !s.is_hide_set())
            .map(|s| Candidate::new(s.get_name(), s.get_about().map(ToString::to_string)));
        let positional = command
            .get_positionals()
            .filter(|a| !a.is_hide_set())
            .nth(positionals)
            .map(|arg| values(arg, name, current, names))
            .unwrap_or_default();
        subcommands.chain(positional).collect()
    };
    found.retain(|c| c.value.starts_with(current));
    found.dedup_by(|a, b| a.value == b.value);
    found
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::Cli;

    fn names() -> Candidates {
        Candidates {
            hostnames: vec!["api.example.com".into(), "app.example.com".into()],
            tunnels: vec!["mac-mini".into(), "staging".into()],
            domains: vec!["example.com".into(), "other.dev".into()],
            accounts: vec!["Personal".into()],
            shares: vec!["demo.other.dev".into(), "qs-1".into()],
        }
    }

    fn values_for(line: &str) -> Vec<String> {
        let mut words: Vec<String> = line.split(' ').map(str::to_owned).collect();
        words.insert(0, "teitunnel".into());
        let index = words.len() - 1;
        complete(&Cli::command(), &words, index, &names())
            .into_iter()
            .map(|c| c.value)
            .collect()
    }

    #[test]
    fn completes_commands_and_flags() {
        let top = values_for("sh");
        assert_eq!(top, ["share", "shares"]);
        assert!(values_for("").contains(&"top".to_owned()));
        assert!(
            !values_for("").iter().any(|v| v.starts_with("__")),
            "hidden"
        );
        let flags = values_for("share 3000 --");
        assert!(flags.contains(&"--here".to_owned()) && flags.contains(&"--app".to_owned()));
        assert_eq!(values_for("completions p"), ["powershell"]);
    }

    #[test]
    fn completes_names_from_the_store() {
        assert_eq!(
            values_for("route remove ap"),
            ["api.example.com", "app.example.com"]
        );
        assert_eq!(values_for("routes --account "), ["Personal"]);
        assert_eq!(values_for("routes -a P"), ["Personal"]);
        assert_eq!(values_for("routes --account=P"), ["--account=Personal"]);
        assert_eq!(values_for("tunnel delete s"), ["staging"]);
        assert_eq!(
            values_for("route add beta"),
            ["beta.example.com", "beta.other.dev"]
        );
        assert_eq!(values_for("share 3000 --on demo.o"), ["demo.other.dev"]);
        assert_eq!(values_for("shares --stop "), ["demo.other.dev", "qs-1"]);
        assert_eq!(
            values_for("analytics a"),
            ["api.example.com", "app.example.com"]
        );
        // A value already given moves on to the next positional.
        assert!(values_for("route add beta.example.com ").is_empty());
    }

    #[test]
    fn completes_fast_from_a_real_store() {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let names = teitunnel_core::completion::candidates(dir.path());
        let words: Vec<String> = ["teitunnel", "route", "remove", ""]
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        let _ = complete(&Cli::command(), &words, 3, &names);
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn prints_scripts_that_call_back() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Powershell] {
            let script = script(shell, false, &mut Cli::command());
            assert!(script.contains("teitunnel __complete"), "{shell:?}");
        }
        let fallback = script(Shell::Zsh, true, &mut Cli::command());
        assert!(fallback.contains("#compdef teitunnel") && !fallback.contains("__complete zsh"));
        assert!(script(Shell::Elvish, false, &mut Cli::command()).contains("teitunnel"));
        assert_eq!(
            Candidate::new("share", Some("Share a port\nMore".into())).line(),
            "share\tShare a port"
        );
    }
}
