//! Command-line mode: `tern -c "<host>"` connects in the current terminal, without the GUI.

use std::io::{BufRead, Read, Write};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use secrecy::SecretString;
use tern_core::Host;
use tern_ssh::{SessionCommand, SessionEvent};
use tern_store::Store;
use tokio::sync::mpsc;

use crate::sessions::{AppKnownHosts, ConnTarget, friendly_error, resolve};

const HELP: &str = "\
tern - keyboard-first SSH client

USAGE:
    tern                   open the GUI
    tern -c <host>         connect to a saved host in this terminal (no GUI)
    tern -h | --help       show this help
    tern -V | --version    show the version
    tern --completions <zsh|bash|fish>
                           print a shell completion script (host names after -c), e.g.
                           eval \"$(tern --completions zsh)\" in ~/.zshrc

<host> is a saved host's label (case-insensitive); a unique part of a label or the exact
hostname also works. You are asked for the master password first.";

const PASSWORD_ATTEMPTS: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Gui,
    Connect(String),
    Print(String),
    /// Hidden: one saved host label per line, for shell completion.
    ListHosts,
}

/// Decide what to do from the arguments after the program name.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Mode> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Mode::Gui);
    };
    match first.as_str() {
        "-h" | "--help" => Ok(Mode::Print(HELP.to_owned())),
        "-V" | "--version" => Ok(Mode::Print(format!("tern {}", env!("CARGO_PKG_VERSION")))),
        "--list-hosts" => Ok(Mode::ListHosts),
        "--completions" => {
            let shell = args.next().unwrap_or_default();
            completion_script(&shell)
                .map(|s| Mode::Print(s.trim_end().to_owned()))
                .ok_or_else(|| anyhow!("--completions needs one of: zsh, bash, fish"))
        }
        "-c" | "--connect" => {
            // `tern -c Pikaia stage` works without quotes too.
            let name = args.collect::<Vec<_>>().join(" ");
            if name.trim().is_empty() {
                bail!("-c needs a host name, e.g. tern -c \"My server\"");
            }
            Ok(Mode::Connect(name.trim().to_owned()))
        }
        other => bail!("unknown argument '{other}'\n\n{HELP}"),
    }
}

fn completion_script(shell: &str) -> Option<&'static str> {
    Some(match shell {
        "zsh" => ZSH,
        "bash" => BASH,
        "fish" => FISH,
        _ => return None,
    })
}

const ZSH: &str = r#"
_tern_hosts() {
  local -a hosts
  hosts=("${(@f)$(tern --list-hosts 2>/dev/null)}")
  compadd -- "${hosts[@]}"
}
_tern() {
  _arguments \
    '(-c --connect)'{-c,--connect}'[connect to a saved host]:host:_tern_hosts' \
    '(-h --help)'{-h,--help}'[show help]' \
    '(-V --version)'{-V,--version}'[show the version]' \
    '--completions[print a completion script]:shell:(zsh bash fish)'
}
compdef _tern tern
"#;

const BASH: &str = r#"
_tern() {
  local cur=${COMP_WORDS[COMP_CWORD]} prev=${COMP_WORDS[COMP_CWORD-1]}
  local IFS=$'\n'
  if [[ $prev == -c || $prev == --connect ]]; then
    COMPREPLY=($(compgen -W "$(tern --list-hosts 2>/dev/null)" -- "$cur"))
    COMPREPLY=("${COMPREPLY[@]// /\\ }")
  else
    COMPREPLY=($(compgen -W "-c --connect -h --help -V --version --completions" -- "$cur"))
  fi
}
complete -F _tern tern
"#;

const FISH: &str = r#"
complete -c tern -s c -l connect -x -a '(tern --list-hosts 2>/dev/null)' -d 'connect to a saved host'
complete -c tern -s h -l help -d 'show help'
complete -c tern -s V -l version -d 'show the version'
complete -c tern -l completions -x -a 'zsh bash fish'
"#;

/// Print the saved host labels, one per line. Needs no password: labels are not secret.
pub fn list_hosts(store: Store) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("starting the async runtime")?;
    let hosts = rt
        .block_on(store.list_hosts())
        .map_err(|e| anyhow!(e.user_message()))?;
    let mut out = std::io::stdout().lock();
    for h in hosts {
        let _ = writeln!(out, "{}", h.label);
    }
    Ok(())
}

/// Pick the host a typed name refers to: exact label, exact hostname, then a unique substring of
/// a label (all case-insensitive).
pub fn find_host<'a>(hosts: &'a [Host], query: &str) -> Result<&'a Host> {
    let q = query.trim().to_lowercase();
    let exact =
        |pred: &dyn Fn(&Host) -> bool| -> Vec<&Host> { hosts.iter().filter(|h| pred(h)).collect() };
    let tiers: [Vec<&Host>; 3] = [
        exact(&|h| h.label.to_lowercase() == q),
        exact(&|h| h.hostname.to_lowercase() == q),
        exact(&|h| h.label.to_lowercase().contains(&q)),
    ];
    for found in tiers {
        match found.as_slice() {
            [] => {}
            [one] => return Ok(one),
            many => {
                let names: Vec<_> = many.iter().map(|h| format!("  {}", h.label)).collect();
                bail!("'{query}' matches several hosts:\n{}", names.join("\n"));
            }
        }
    }
    bail!("no saved host matches '{query}'")
}

/// Run the CLI connect flow. Returns the process exit code.
pub fn connect(store: Store, query: &str) -> Result<i32> {
    let rt = tokio::runtime::Runtime::new().context("starting the async runtime")?;
    let code = rt.block_on(run(store, query))?;
    // The stdin reader thread may still be blocked in `read`; do not wait for it.
    rt.shutdown_background();
    Ok(code)
}

async fn run(store: Store, query: &str) -> Result<i32> {
    if !store
        .vault_exists()
        .await
        .map_err(|e| anyhow!(e.user_message()))?
    {
        bail!("no vault yet: start tern once without arguments to set a master password");
    }
    let hosts = store
        .list_hosts()
        .await
        .map_err(|e| anyhow!(e.user_message()))?;
    let host = find_host(&hosts, query)?.clone();
    eprintln!("Host: {} ({}:{})", host.label, host.hostname, host.port);

    unlock(&store).await?;

    let target = ConnTarget::Saved(host.id.to_string());
    let mut params = resolve(&store, &target).await.map_err(|e| anyhow!(e))?;
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        params.cols = u32::from(cols);
        params.rows = u32::from(rows);
    }
    let known = Arc::new(AppKnownHosts(store.clone()));
    let (handle, mut events) = tern_ssh::spawn(params, known);

    let mut input: Option<mpsc::UnboundedReceiver<Vec<u8>>> = None;
    let mut raw: Option<RawMode> = None;
    let mut exit_code = 0;
    let mut winch = Winch::new()?;

    loop {
        tokio::select! {
            ev = events.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    SessionEvent::Connecting => eprintln!("Connecting..."),
                    SessionEvent::HostKeyPrompt { host, port, key_type, fingerprint, changed_from } => {
                        let accept = ask_host_key(&host, port, &key_type, &fingerprint, changed_from.as_deref()).await;
                        handle.send(SessionCommand::HostKeyDecision(accept)).await;
                    }
                    SessionEvent::AuthPrompt { name, instructions, prompts } => {
                        if !name.is_empty() { eprintln!("{name}"); }
                        if !instructions.is_empty() { eprintln!("{instructions}"); }
                        let mut answers = Vec::new();
                        for p in prompts {
                            answers.push(read_answer(p.prompt, p.echo).await?);
                        }
                        handle.send(SessionCommand::AuthResponse(answers)).await;
                    }
                    SessionEvent::Connected => {
                        let _ = store.touch_last_connected(host.id).await;
                        raw = Some(RawMode::enable()?);
                        input = Some(spawn_stdin_reader());
                        // The size may have changed while authenticating.
                        send_size(&handle);
                    }
                    SessionEvent::Output(bytes) => {
                        let mut out = std::io::stdout().lock();
                        let _ = out.write_all(&bytes);
                        let _ = out.flush();
                    }
                    SessionEvent::Exited(code) => exit_code = code.map_or(0, |c| i32::try_from(c).unwrap_or(1)),
                    SessionEvent::Closed => break,
                    SessionEvent::Error(e) => {
                        drop(raw.take());
                        eprintln!("\n{}", friendly_error(&e, &host.hostname));
                        return Ok(1);
                    }
                }
            }
            Some(bytes) = recv_input(&mut input) => {
                handle.send(SessionCommand::Write(bytes.into())).await;
            }
            () = winch.changed() => send_size(&handle),
        }
    }
    drop(raw.take());
    Ok(exit_code)
}

/// Fires when the terminal window is resized (SIGWINCH).
struct Winch(#[cfg(unix)] tokio::signal::unix::Signal);

impl Winch {
    #[cfg(unix)]
    fn new() -> Result<Self> {
        Ok(Self(tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::window_change(),
        )?))
    }

    #[cfg(not(unix))]
    fn new() -> Result<Self> {
        Ok(Self())
    }

    #[cfg(unix)]
    async fn changed(&mut self) {
        self.0.recv().await;
    }

    #[cfg(not(unix))]
    async fn changed(&mut self) {
        std::future::pending::<()>().await;
    }
}

fn send_size(handle: &tern_ssh::SessionHandle) {
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        handle.try_send(SessionCommand::Resize {
            cols: u32::from(cols),
            rows: u32::from(rows),
        });
    }
}

async fn recv_input(rx: &mut Option<mpsc::UnboundedReceiver<Vec<u8>>>) -> Option<Vec<u8>> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Keystrokes are forwarded byte for byte; the remote does the line editing.
fn spawn_stdin_reader() -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

/// Puts the terminal in raw mode; restores it on drop (also on error paths and panics).
struct RawMode;

impl RawMode {
    fn enable() -> Result<Self> {
        crossterm::terminal::enable_raw_mode().context("switching the terminal to raw mode")?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

async fn unlock(store: &Store) -> Result<()> {
    for attempt in 1..=PASSWORD_ATTEMPTS {
        let pw = read_answer("Master password: ".into(), false).await?;
        match store.unlock(pw).await {
            Ok(()) => return Ok(()),
            Err(tern_store::Error::Vault(tern_vault::Error::WrongPassword)) => {
                if attempt < PASSWORD_ATTEMPTS {
                    eprintln!("Wrong password, try again.");
                }
            }
            Err(e) => bail!("{}", e.user_message()),
        }
    }
    bail!("wrong master password")
}

/// Prompt on the terminal; hidden input unless `echo`. Runs on a blocking thread.
async fn read_answer(prompt: String, echo: bool) -> Result<SecretString> {
    tokio::task::spawn_blocking(move || -> Result<SecretString> {
        let line = if echo {
            eprint!("{prompt}");
            let _ = std::io::stderr().flush();
            let mut s = String::new();
            std::io::stdin().lock().read_line(&mut s)?;
            s.trim_end_matches(['\r', '\n']).to_owned()
        } else {
            rpassword::prompt_password(prompt)?
        };
        Ok(SecretString::from(line))
    })
    .await?
}

async fn ask_host_key(
    host: &str,
    port: u16,
    key_type: &str,
    fingerprint: &str,
    changed_from: Option<&str>,
) -> bool {
    match changed_from {
        Some(old) => eprintln!(
            "WARNING: the host key of {host}:{port} CHANGED.\n  stored: {old}\n  now:    {fingerprint} ({key_type})\nThis may be a man-in-the-middle attack."
        ),
        None => {
            eprintln!("The authenticity of {host}:{port} is unknown.\n  {key_type} {fingerprint}")
        }
    }
    let question = if changed_from.is_some() {
        "Replace the stored key and continue? [y/N] "
    } else {
        "Trust this key and continue? [y/N] "
    };
    read_answer(question.into(), true).await.is_ok_and(|a| {
        use secrecy::ExposeSecret;
        matches!(
            a.expose_secret().trim().to_lowercase().as_str(),
            "y" | "yes"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<Mode> {
        parse_args(a.iter().map(|s| (*s).to_owned()))
    }

    fn host(label: &str, hostname: &str) -> Host {
        Host::new(label, hostname)
    }

    #[test]
    fn completion_flags_parse() {
        assert_eq!(args(&["--list-hosts"]).unwrap(), Mode::ListHosts);
        assert!(matches!(
            args(&["--completions", "zsh"]).unwrap(),
            Mode::Print(_)
        ));
        assert!(args(&["--completions", "csh"]).is_err());
        assert!(args(&["--completions"]).is_err());
    }

    #[test]
    fn no_args_opens_the_gui() {
        assert_eq!(args(&[]).unwrap(), Mode::Gui);
    }

    #[test]
    fn connect_takes_a_quoted_or_split_name() {
        assert_eq!(
            args(&["-c", "Pikaia stage"]).unwrap(),
            Mode::Connect("Pikaia stage".into())
        );
        assert_eq!(
            args(&["--connect", "Pikaia", "stage"]).unwrap(),
            Mode::Connect("Pikaia stage".into())
        );
        assert!(args(&["-c"]).is_err());
        assert!(args(&["--bogus"]).is_err());
    }

    #[test]
    fn host_matching_prefers_exact_then_unique_substring() {
        let hosts = vec![
            host("Pikaia stage", "stage.pikaia.io"),
            host("Pikaia prod", "prod.pikaia.io"),
            host("db", "10.0.0.5"),
        ];
        assert_eq!(
            find_host(&hosts, "pikaia STAGE").unwrap().label,
            "Pikaia stage"
        );
        assert_eq!(find_host(&hosts, "10.0.0.5").unwrap().label, "db");
        assert_eq!(find_host(&hosts, "prod").unwrap().label, "Pikaia prod");
        assert!(
            find_host(&hosts, "pikaia")
                .unwrap_err()
                .to_string()
                .contains("several")
        );
        assert!(find_host(&hosts, "nope").is_err());
    }
}
