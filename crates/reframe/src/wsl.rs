//! WSL distributions and what it takes to run claude in one. That is asked once per distribution
//! and run of reframe, through the user's login shell (where nvm and the like put claude and node
//! on the PATH), which also starts the distribution. wsl.exe and the share can be replaced for
//! tests (`REFRAME_WSL_EXE`, a script run by `REFRAME_NODE`, and `REFRAME_WSL_SHARE`).
//!
//! Ported from the Electron version's `src/main/wsl/distros.ts`. Everything here blocks (it runs
//! wsl.exe or reads the share, which can take seconds): call it off the UI thread. It can be
//! shared between threads.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(windows)]
use reframe_core::claude::registry::RegistryEntry;
use reframe_core::log::Logger;
use reframe_core::wsl::WSL_SHARES;
#[cfg(windows)]
use reframe_core::wsl::linux_path_of;
use reframe_core::wsl_distro::{
    PROBE_SCRIPT, WslInfo, decode_wsl_output, looks_like_claude, parse_distro_list, parse_probe,
};
#[cfg(windows)]
use reframe_core::wsl_distro::{interop_of, is_idle, parent_pid_command, shells_of};
use serde_json::json;

use crate::process::NoWindow;

/// How long reading from a distribution's share may take before it counts as no answer: the
/// share starts a distribution that is not running, which takes seconds.
pub const SHARE_TIMEOUT: Duration = Duration::from_millis(3000);
/// How long a distribution without claude is taken at its word before it is asked again.
const NO_CLAUDE: Duration = Duration::from_millis(30_000);
/// How long the list of running distributions holds.
const RUNNING: Duration = Duration::from_millis(3000);
/// How long a claude whose terminal could not be told is left alone before it is asked again.
#[cfg(windows)]
const NO_TERMINAL: Duration = Duration::from_millis(5000);

/// wsl.exe, or a stand-in script run by node (as in the end-to-end tests).
#[derive(Clone, Debug)]
pub struct WslCommand {
    pub file: PathBuf,
    /// Arguments before wsl.exe's own (the script of a stand-in).
    pub prefix_args: Vec<String>,
}

impl WslCommand {
    pub fn from_env(env: &HashMap<String, String>) -> Self {
        let set = |name: &str| env.get(name).filter(|value| !value.is_empty()).cloned();
        match set("REFRAME_WSL_EXE") {
            Some(script)
                if [".js", ".mjs", ".cjs"]
                    .iter()
                    .any(|ext| script.to_lowercase().ends_with(ext)) =>
            {
                Self {
                    file: PathBuf::from(set("REFRAME_NODE").unwrap_or_else(|| "node".into())),
                    prefix_args: vec![script],
                }
            }
            Some(exe) => Self {
                file: PathBuf::from(exe),
                prefix_args: Vec::new(),
            },
            None => Self {
                file: Path::new(&set("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
                    .join("System32")
                    .join("wsl.exe"),
                prefix_args: Vec::new(),
            },
        }
    }
}

pub struct WslDistros {
    pub command: WslCommand,
    /// Where Windows shows the distributions' files.
    pub share: String,
    log: Logger,
    /// What each distribution answered (by its name in lower case), and when.
    infos: Mutex<HashMap<String, (Instant, Option<WslInfo>)>>,
    /// One probe at a time.
    probing: Mutex<()>,
    running_now: Mutex<Option<(Instant, Vec<String>)>>,
    /// Processes that keep distributions running (started from the session list).
    keepers: Mutex<HashMap<String, Child>>,
    #[cfg(windows)]
    terminals: Mutex<HashMap<String, (Instant, Option<u32>)>>,
    /// Distributions Claude Code was found in since the controller last asked.
    found: Mutex<HashSet<String>>,
}

impl WslDistros {
    pub fn new(env: &HashMap<String, String>, log: Logger) -> Self {
        let share = env
            .get("REFRAME_WSL_SHARE")
            .filter(|share| !share.is_empty())
            .cloned()
            .unwrap_or_else(|| WSL_SHARES[0].to_owned());
        Self {
            command: WslCommand::from_env(env),
            share,
            log,
            infos: Mutex::new(HashMap::new()),
            probing: Mutex::new(()),
            running_now: Mutex::new(None),
            keepers: Mutex::new(HashMap::new()),
            #[cfg(windows)]
            terminals: Mutex::new(HashMap::new()),
            found: Mutex::new(HashSet::new()),
        }
    }

    /// Runs wsl.exe with these arguments; its output, or what went wrong.
    fn run(&self, args: &[&str], timeout: Duration) -> Result<String, String> {
        if cfg!(not(windows)) {
            return Err("WSL is a Windows thing".into());
        }
        let mut child = Command::new(&self.command.file)
            .args(&self.command.prefix_args)
            .args(args)
            .env("WSL_UTF8", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .no_window()
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut stdout = child.stdout.take().ok_or("no output")?;
        let reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = stdout.read_to_end(&mut output);
            output
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Ok(None) => {
                    let _ = child.kill();
                    return Err("timed out".into());
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let output = reader.join().unwrap_or_default();
        if status.success() {
            Ok(decode_wsl_output(&output))
        } else {
            Err(format!("exited with {status}"))
        }
    }

    /// What it takes to run claude in `distro`; starts the distribution. Asked again a while
    /// later when it has none.
    pub fn info(&self, distro: &str) -> Option<WslInfo> {
        let key = distro.to_lowercase();
        let known = |infos: &HashMap<String, (Instant, Option<WslInfo>)>| {
            infos
                .get(&key)
                .filter(|(at, info)| {
                    info.as_ref().is_some_and(|info| info.claude.is_some()) || at.elapsed() < NO_CLAUDE
                })
                .map(|(_, info)| info.clone())
        };
        if let Some(info) = self.infos.lock().ok().and_then(|infos| known(&infos)) {
            return info;
        }
        let _probing = self.probing.lock();
        // Another thread may have asked meanwhile.
        if let Some(info) = self.infos.lock().ok().and_then(|infos| known(&infos)) {
            return info;
        }
        let info = self.probe(distro);
        if let Ok(mut infos) = self.infos.lock() {
            infos.insert(key, (Instant::now(), info.clone()));
        }
        if info.as_ref().is_some_and(|info| info.claude.is_some())
            && let Ok(mut found) = self.found.lock()
        {
            found.insert(distro.to_owned());
        }
        info
    }

    /// What is known already about `distro`, without asking it.
    pub fn cached(&self, distro: &str) -> Option<WslInfo> {
        let infos = self.infos.lock().ok()?;
        infos
            .get(&distro.to_lowercase())
            .and_then(|(_, info)| info.clone())
            .filter(|info| info.claude.is_some())
    }

    /// Distributions Claude Code was found in since this was last asked.
    pub fn take_found(&self) -> Vec<String> {
        self.found
            .lock()
            .map(|mut found| found.drain().collect())
            .unwrap_or_default()
    }

    fn probe(&self, distro: &str) -> Option<WslInfo> {
        let started = Instant::now();
        match self.run(&["-d", distro, "-e", "sh", "-c", PROBE_SCRIPT], Duration::from_secs(60)) {
            Ok(output) => {
                let info = parse_probe(distro, &output);
                let mut data = json!({
                    "distro": distro,
                    "ms": started.elapsed().as_millis() as u64,
                    "claude": info.as_ref().and_then(|info| info.claude.clone()).unwrap_or_else(|| "not found".into()),
                });
                if info.is_some() {
                    self.log.info("wsl distribution asked", Some(data));
                } else {
                    data["problem"] = json!("no answer");
                    self.log.warn("wsl distribution asked", Some(data));
                }
                info
            }
            Err(error) => {
                self.log.warn(
                    "wsl distribution could not be asked",
                    Some(json!({ "distro": distro, "error": error })),
                );
                None
            }
        }
    }

    /// The distributions that run now, without Docker Desktop's own (asking does not start any);
    /// an answer up to `max_age` old will do.
    pub fn running(&self, max_age: Option<Duration>) -> Vec<String> {
        let max_age = max_age.unwrap_or(RUNNING);
        if let Ok(now) = self.running_now.lock()
            && let Some((at, names)) = now.as_ref()
            && at.elapsed() <= max_age
        {
            return names.clone();
        }
        let names: Vec<String> = parse_distro_list(
            &self
                .run(&["-l", "--running", "-q"], Duration::from_secs(10))
                .unwrap_or_default(),
        )
        .into_iter()
        .filter(|name| !name.to_lowercase().starts_with("docker-desktop"))
        .collect();
        if let Ok(mut now) = self.running_now.lock() {
            *now = Some((Instant::now(), names.clone()));
        }
        names
    }

    fn proc_path(&self, distro: &str, pid: u32, file: &str) -> PathBuf {
        let mut path = Path::new(&self.share).join(distro).join("proc").join(pid.to_string());
        for part in file.split('/').filter(|part| !part.is_empty()) {
            path.push(part);
        }
        path
    }

    /// A file in the folder /proc/<pid> of `distro`; `None` when it cannot be read.
    fn read_proc(&self, distro: &str, pid: u32, file: &str) -> Option<String> {
        std::fs::read(self.proc_path(distro, pid, file))
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Whether a Linux process runs in `distro` (as its /proc shows it).
    pub fn alive(&self, distro: &str, pid: u32) -> bool {
        self.proc_path(distro, pid, "").exists()
    }

    /// Whether the Linux process `pid` runs in `distro` and is a claude. A claude that was killed
    /// leaves its registry entry behind, and a distribution numbers its processes anew every time
    /// it starts: the pid in such an entry is soon another program's.
    pub fn is_claude(&self, distro: &str, pid: u32) -> bool {
        looks_like_claude(&self.read_proc(distro, pid, "cmdline").unwrap_or_default())
    }

    /// The shells between a claude and WSL's own process for its terminal (see
    /// `wsl_distro::shells_of`).
    #[cfg(windows)]
    pub fn shells_of(&self, distro: &str, pid: u32) -> Vec<u32> {
        shells_of(pid, &|pid, file| self.read_proc(distro, pid, file))
    }

    /// Whether a Linux process runs no program of its own (a shell at its prompt).
    #[cfg(windows)]
    pub fn idle(&self, distro: &str, pid: u32) -> bool {
        is_idle(pid, &|pid, file| self.read_proc(distro, pid, file))
    }

    /// The Windows process whose console is the terminal of claude `entry` in `distro`: the
    /// program (wsl.exe) that started the WSL session it runs in. A Windows program started
    /// through that session's interop socket is that program's child, so reframe's own
    /// executable, run that way, tells which it is. Asked once per claude.
    #[cfg(windows)]
    pub fn terminal_of(&self, distro: &str, entry: &RegistryEntry) -> Option<u32> {
        let started = entry
            .proc_start
            .clone()
            .or_else(|| entry.started_at.map(|at| at.to_string()))
            .unwrap_or_default();
        let key = format!("{}:{}:{started}", distro.to_lowercase(), entry.pid);
        if let Ok(terminals) = self.terminals.lock()
            && let Some((at, terminal)) = terminals.get(&key)
            && (terminal.is_some() || at.elapsed() < NO_TERMINAL)
        {
            return *terminal;
        }
        let terminal = self.ask_terminal(distro, entry.pid);
        if let Ok(mut terminals) = self.terminals.lock() {
            if terminals.len() > 200 {
                terminals.clear();
            }
            terminals.insert(key, (Instant::now(), terminal));
        }
        terminal
    }

    #[cfg(windows)]
    fn ask_terminal(&self, distro: &str, pid: u32) -> Option<u32> {
        let interop = interop_of(&self.read_proc(distro, pid, "environ").unwrap_or_default());
        let info = self.info(distro);
        let exe = std::env::current_exe().ok();
        let program = info
            .as_ref()
            .zip(exe.as_ref())
            .and_then(|(info, exe)| linux_path_of(&exe.to_string_lossy(), distro, &info.mount_root, Some(&self.share)));
        let (Some(interop), Some(program)) = (interop.clone(), program.clone()) else {
            self.log.warn(
                "cannot tell the terminal of a claude in wsl",
                Some(json!({ "distro": distro, "pid": pid, "interop": interop, "program": program })),
            );
            return None;
        };
        let command = parent_pid_command(&interop, &program);
        let mut args: Vec<&str> = vec!["-d", distro, "-e"];
        args.extend(command.iter().map(String::as_str));
        let output = self.run(&args, Duration::from_secs(10)).unwrap_or_else(|error| error);
        let terminal = output.trim().parse::<u32>().ok().filter(|pid| *pid > 0);
        match terminal {
            Some(terminal) => self.log.info(
                "terminal of a claude in wsl found",
                Some(json!({ "distro": distro, "pid": pid, "terminal": terminal })),
            ),
            None => self.log.warn(
                "cannot tell the terminal of a claude in wsl",
                Some(json!({ "distro": distro, "pid": pid, "answer": output.chars().take(300).collect::<String>() })),
            ),
        }
        terminal
    }

    /// Ends a claude running in `distro`, from outside (taking it over): an interrupt as from
    /// Ctrl+C, which Claude Code answers by ending cleanly, a second one, then a plain
    /// termination. Whether it is gone.
    pub fn stop_claude(&self, distro: &str, pid: u32) -> bool {
        // Never a program that merely has the pid of a claude by now.
        if !self.is_claude(distro, pid) {
            self.log.warn(
                "not a claude (any more), left alone",
                Some(json!({ "distro": distro, "pid": pid })),
            );
            return false;
        }
        let pid_text = pid.to_string();
        for signal in ["INT", "INT", "TERM"] {
            let _ = self.run(
                &["-d", distro, "-e", "kill", "-s", signal, &pid_text],
                Duration::from_secs(10),
            );
            let deadline = Instant::now() + Duration::from_millis(3000);
            while Instant::now() < deadline {
                if !self.alive(distro, pid) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        !self.alive(distro, pid)
    }

    /// Starts `distro` and keeps it running until `release` (WSL stops a distribution soon when
    /// nothing runs in it).
    pub fn keep(&self, distro: &str) {
        let Ok(mut keepers) = self.keepers.lock() else { return };
        if let Some(keeper) = keepers.get_mut(distro)
            && keeper.try_wait().ok().flatten().is_none()
        {
            return;
        }
        let keeper = Command::new(&self.command.file)
            .args(&self.command.prefix_args)
            .args(["-d", distro, "-e", "sleep", "infinity"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .no_window()
            .spawn();
        match keeper {
            Ok(keeper) => {
                keepers.insert(distro.to_owned(), keeper);
                if let Ok(mut now) = self.running_now.lock() {
                    *now = None;
                }
                self.log.info(
                    "wsl distribution started from the session list",
                    Some(json!({ "distro": distro })),
                );
            }
            Err(error) => self.log.warn(
                "wsl distribution could not be started",
                Some(json!({ "distro": distro, "error": error.to_string() })),
            ),
        }
    }

    /// Lets go of the distributions kept running (reframe quits).
    pub fn release(&self) {
        if let Ok(mut keepers) = self.keepers.lock() {
            for keeper in keepers.values_mut() {
                let _ = keeper.kill();
            }
            keepers.clear();
        }
    }
}

/// Started through a WSL session's interop socket to tell which program started that session:
/// prints the pid of reframe's parent and leaves. `None` when this is not such a start.
pub fn print_parent_pid_from_args() -> Option<i32> {
    if std::env::args().nth(1).as_deref() != Some(reframe_core::wsl_distro::PARENT_PID_ARG) {
        return None;
    }
    let parent = crate::process::parent_of(std::process::id()).unwrap_or(0);
    print!("{parent}");
    Some(0)
}
