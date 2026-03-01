use crate::config::Config;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Cross-platform utilities ────────────────────────────────────

fn mise_bin() -> Option<PathBuf> {
    std::env::var("PATH")
        .ok()
        .and_then(|paths| {
            paths.split(':').find_map(|dir| {
                let p = PathBuf::from(dir).join("mise");
                if p.is_file() {
                    Some(p)
                } else {
                    None
                }
            })
        })
}

fn mise_init_cmd(mise_path: &Path) -> String {
    let p = mise_path.display();
    format!("{p} trust && eval \"$({p} activate bash)\" && eval \"$({p} env)\"")
}

fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|s| {
            if s.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '\\') {
                format!("'{}'", s.replace('\'', "'\\''"))
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_shell_command(config: &Config) -> String {
    let use_mise = config.no_mise != Some(true);
    let mise_prefix = if use_mise {
        mise_bin().map(|p| mise_init_cmd(&p))
    } else {
        None
    };
    let mise_prefix = mise_prefix.as_deref().unwrap_or("true");

    let user_cmd = if config.command.is_empty() {
        "bash".to_string()
    } else {
        shell_join(&config.command)
    };

    format!("{mise_prefix} && {user_cmd}")
}

// ── Platform dispatcher ─────────────────────────────────────────

pub fn check_sandbox() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        check_bwrap()
    }
    #[cfg(target_os = "macos")]
    {
        check_sandbox_exec()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err("ai-jail is only supported on Linux and macOS".into())
    }
}

// ── Linux: bwrap ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
use crate::mounts::{self, Mount, MountSet};

#[cfg(target_os = "linux")]
fn check_bwrap() -> Result<(), String> {
    match Command::new("bwrap").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(_) => Err("bwrap found but returned an error. Check your installation.".into()),
        Err(_) => Err(
            "bwrap (bubblewrap) not found. Install it:\n  \
             Arch: pacman -S bubblewrap\n  \
             Debian/Ubuntu: apt install bubblewrap\n  \
             Fedora: dnf install bubblewrap"
                .into(),
        ),
    }
}

#[cfg(target_os = "linux")]
pub fn build_bwrap(
    config: &Config,
    project_dir: &Path,
    hosts_file: &Path,
    verbose: bool,
) -> Command {
    let mount_set = discover_mounts(config, project_dir, hosts_file, verbose);
    let mut cmd = Command::new("bwrap");

    // Add all mounts in order
    add_mounts(&mut cmd, &mount_set.base);
    add_mounts(&mut cmd, &mount_set.gpu);
    add_mounts(&mut cmd, &mount_set.shm);
    add_mounts(&mut cmd, &mount_set.docker);
    add_mounts(&mut cmd, &mount_set.display);
    add_mounts(&mut cmd, &mount_set.home_dotfiles);
    add_mounts(&mut cmd, &mount_set.config_hide);
    add_mounts(&mut cmd, &mount_set.cache_hide);
    add_mounts(&mut cmd, &mount_set.local_overrides);
    add_mounts(&mut cmd, &mount_set.extra);
    add_mounts(&mut cmd, &mount_set.project);

    // Working directory
    cmd.arg("--chdir").arg(project_dir);

    // Isolation
    cmd.arg("--die-with-parent");
    cmd.arg("--unshare-pid");
    cmd.arg("--unshare-uts");
    cmd.arg("--unshare-ipc");
    cmd.arg("--hostname").arg("ai-sandbox");

    // Display env vars
    for (key, val) in &mount_set.display_env {
        cmd.arg("--setenv").arg(key).arg(val);
    }

    // Standard env vars
    cmd.arg("--setenv").arg("PS1").arg("(jail) \\w \\$ ");
    cmd.arg("--setenv").arg("_ZO_DOCTOR").arg("0");

    let full_cmd = build_shell_command(config);
    cmd.arg("bash").arg("-c").arg(&full_cmd);

    cmd
}

#[cfg(target_os = "linux")]
pub fn build_bwrap_dry_run(
    config: &Config,
    project_dir: &Path,
    hosts_file: &Path,
    verbose: bool,
) -> Vec<String> {
    let mount_set = discover_mounts(config, project_dir, hosts_file, verbose);
    let mut args: Vec<String> = vec!["bwrap".into()];

    // Mounts
    args.extend(mount_args(&mount_set.base));
    args.extend(mount_args(&mount_set.gpu));
    args.extend(mount_args(&mount_set.shm));
    args.extend(mount_args(&mount_set.docker));
    args.extend(mount_args(&mount_set.display));
    args.extend(mount_args(&mount_set.home_dotfiles));
    args.extend(mount_args(&mount_set.config_hide));
    args.extend(mount_args(&mount_set.cache_hide));
    args.extend(mount_args(&mount_set.local_overrides));
    args.extend(mount_args(&mount_set.extra));
    args.extend(mount_args(&mount_set.project));

    args.push("--chdir".into());
    args.push(project_dir.display().to_string());

    args.push("--die-with-parent".into());
    args.push("--unshare-pid".into());
    args.push("--unshare-uts".into());
    args.push("--unshare-ipc".into());
    args.push("--hostname".into());
    args.push("ai-sandbox".into());

    for (key, val) in &mount_set.display_env {
        args.push("--setenv".into());
        args.push(key.clone());
        args.push(val.clone());
    }

    args.push("--setenv".into());
    args.push("PS1".into());
    args.push("(jail) \\w \\$ ".into());
    args.push("--setenv".into());
    args.push("_ZO_DOCTOR".into());
    args.push("0".into());

    let full_cmd = build_shell_command(config);
    args.push("bash".into());
    args.push("-c".into());
    args.push(full_cmd);

    args
}

#[cfg(target_os = "linux")]
fn discover_mounts(
    config: &Config,
    project_dir: &Path,
    hosts_file: &Path,
    verbose: bool,
) -> MountSet {
    let enable_gpu = config.no_gpu != Some(true);
    let enable_docker = config.no_docker != Some(true);
    let enable_display = config.no_display != Some(true);

    let (display_mounts, display_env) = if enable_display {
        mounts::discover_display(verbose)
    } else {
        (vec![], vec![])
    };

    MountSet {
        base: mounts::discover_base(hosts_file),
        home_dotfiles: mounts::discover_home_dotfiles(verbose),
        config_hide: mounts::discover_config_hide(),
        cache_hide: mounts::discover_cache_hide(),
        local_overrides: mounts::discover_local_overrides(),
        gpu: if enable_gpu {
            mounts::discover_gpu(verbose)
        } else {
            vec![]
        },
        docker: if enable_docker {
            mounts::discover_docker()
        } else {
            vec![]
        },
        shm: mounts::discover_shm(),
        display: display_mounts,
        display_env,
        extra: mounts::extra_mounts(&config.rw_maps, &config.ro_maps),
        project: mounts::project_mount(project_dir),
    }
}

#[cfg(target_os = "linux")]
fn add_mounts(cmd: &mut Command, mounts: &[Mount]) {
    for m in mounts {
        match m {
            Mount::RoBind { src, dest } => {
                cmd.arg("--ro-bind").arg(src).arg(dest);
            }
            Mount::Bind { src, dest } => {
                cmd.arg("--bind").arg(src).arg(dest);
            }
            Mount::DevBind { src, dest } => {
                cmd.arg("--dev-bind").arg(src).arg(dest);
            }
            Mount::Dev { dest } => {
                cmd.arg("--dev").arg(dest);
            }
            Mount::Proc { dest } => {
                cmd.arg("--proc").arg(dest);
            }
            Mount::Tmpfs { dest } => {
                cmd.arg("--tmpfs").arg(dest);
            }
            Mount::Symlink { src, dest } => {
                cmd.arg("--symlink").arg(src).arg(dest);
            }
            Mount::FileRoBind { src, dest } => {
                cmd.arg("--ro-bind").arg(src).arg(dest);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn mount_args(mounts: &[Mount]) -> Vec<String> {
    let mut args = Vec::new();
    for m in mounts {
        match m {
            Mount::RoBind { src, dest } => {
                args.extend(["--ro-bind".into(), src.display().to_string(), dest.display().to_string()]);
            }
            Mount::Bind { src, dest } => {
                args.extend(["--bind".into(), src.display().to_string(), dest.display().to_string()]);
            }
            Mount::DevBind { src, dest } => {
                args.extend(["--dev-bind".into(), src.display().to_string(), dest.display().to_string()]);
            }
            Mount::Dev { dest } => {
                args.extend(["--dev".into(), dest.display().to_string()]);
            }
            Mount::Proc { dest } => {
                args.extend(["--proc".into(), dest.display().to_string()]);
            }
            Mount::Tmpfs { dest } => {
                args.extend(["--tmpfs".into(), dest.display().to_string()]);
            }
            Mount::Symlink { src, dest } => {
                args.extend(["--symlink".into(), src.clone(), dest.display().to_string()]);
            }
            Mount::FileRoBind { src, dest } => {
                args.extend(["--ro-bind".into(), src.display().to_string(), dest.display().to_string()]);
            }
        }
    }
    args
}

pub fn format_dry_run(args: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < args.len() {
        if i == 0 {
            out.push_str(&args[0]);
            out.push_str(" \\\n");
            i += 1;
            continue;
        }
        let arg = &args[i];
        if arg.starts_with("--") {
            out.push_str("  ");
            out.push_str(arg);
            // Collect following non-flag args
            let mut j = i + 1;
            while j < args.len() && !args[j].starts_with("--") && args[j] != "bash" {
                out.push(' ');
                let val = &args[j];
                if val.contains(|c: char| c.is_whitespace()) {
                    out.push_str(&format!("'{val}'"));
                } else {
                    out.push_str(val);
                }
                j += 1;
            }
            out.push_str(" \\\n");
            i = j;
        } else {
            // bare args (bash -c ...)
            out.push_str("  ");
            for k in i..args.len() {
                if k > i {
                    out.push(' ');
                }
                let val = &args[k];
                if val.contains(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '$')
                {
                    out.push_str(&format!("'{val}'"));
                } else {
                    out.push_str(val);
                }
            }
            out.push('\n');
            break;
        }
    }
    out
}

// ── macOS: sandbox-exec ─────────────────────────────────────────

#[cfg(target_os = "macos")]
use crate::mounts;

#[cfg(target_os = "macos")]
fn check_sandbox_exec() -> Result<(), String> {
    let path = Path::new("/usr/bin/sandbox-exec");
    if path.is_file() {
        Ok(())
    } else {
        Err("sandbox-exec not found at /usr/bin/sandbox-exec. \
             This tool is required for sandboxing on macOS."
            .into())
    }
}

#[cfg(target_os = "macos")]
fn canonicalize_or_keep(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(target_os = "macos")]
pub fn generate_sbpl_profile(
    config: &Config,
    project_dir: &Path,
    enable_docker: bool,
) -> String {
    let deny_paths = mounts::macos_read_deny_paths();
    let writable_paths = mounts::macos_writable_paths(project_dir, config);

    let mut profile = String::new();
    profile.push_str("(version 1)\n");
    profile.push_str("(deny default)\n\n");

    // Process operations
    profile.push_str("; Process operations\n");
    profile.push_str("(allow process-exec)\n");
    profile.push_str("(allow process-fork)\n");
    profile.push_str("(allow signal)\n");
    profile.push_str("(allow sysctl-read)\n\n");

    // IPC and Mach
    profile.push_str("; IPC and Mach\n");
    profile.push_str("(allow mach-lookup)\n");
    profile.push_str("(allow mach-register)\n");
    profile.push_str("(allow ipc-posix-shm-read-data)\n");
    profile.push_str("(allow ipc-posix-shm-write-data)\n");
    profile.push_str("(allow ipc-posix-shm-read-metadata)\n");
    profile.push_str("(allow ipc-posix-shm-write-create)\n\n");

    // Pseudo-terminal operations
    profile.push_str("; Pseudo-terminal\n");
    profile.push_str("(allow pseudo-tty)\n\n");

    // Network
    profile.push_str("; Network\n");
    profile.push_str("(allow network-outbound)\n");
    profile.push_str("(allow network-inbound)\n");
    profile.push_str("(allow network-bind)\n");
    profile.push_str("(allow system-socket)\n\n");

    // File reads: allow globally, then deny sensitive paths
    profile.push_str("; File reads: allow globally, deny sensitive paths\n");
    profile.push_str("(allow file-read*)\n");

    for deny_path in &deny_paths {
        let canonical = canonicalize_or_keep(deny_path);
        let display = canonical.display();
        if canonical.is_dir() {
            profile.push_str(&format!("(deny file-read* (subpath \"{display}\"))\n"));
        } else {
            profile.push_str(&format!("(deny file-read* (literal \"{display}\"))\n"));
        }
    }
    profile.push('\n');

    // File writes: deny by default (from deny default), allow specific paths
    profile.push_str("; File writes: allow specific paths\n");
    for wr_path in &writable_paths {
        let canonical = canonicalize_or_keep(wr_path);
        let display = canonical.display();
        if canonical.is_dir() || !canonical.exists() {
            profile.push_str(&format!("(allow file-write* (subpath \"{display}\"))\n"));
        } else {
            profile.push_str(&format!("(allow file-write* (literal \"{display}\"))\n"));
        }
    }
    profile.push('\n');

    // Docker socket
    if enable_docker {
        if let Some(sock) = mounts::macos_docker_socket() {
            let canonical = canonicalize_or_keep(&sock);
            let display = canonical.display();
            profile.push_str("; Docker socket\n");
            profile.push_str(&format!("(allow file-write* (literal \"{display}\"))\n"));
            profile.push('\n');
        }
    }

    profile
}

#[cfg(target_os = "macos")]
pub fn build_sandbox_exec(
    config: &Config,
    project_dir: &Path,
    verbose: bool,
) -> Command {
    let enable_docker = config.no_docker != Some(true);
    let profile = generate_sbpl_profile(config, project_dir, enable_docker);

    if verbose {
        crate::output::verbose("SBPL profile:");
        for line in profile.lines() {
            crate::output::verbose(&format!("  {line}"));
        }
    }

    let full_cmd = build_shell_command(config);

    let mut cmd = Command::new("/usr/bin/sandbox-exec");
    cmd.arg("-p").arg(&profile);
    cmd.arg("bash").arg("-c").arg(&full_cmd);
    cmd.current_dir(project_dir);

    // Standard env vars
    cmd.env("PS1", "(jail) \\w \\$ ");
    cmd.env("_ZO_DOCTOR", "0");

    cmd
}

#[cfg(target_os = "macos")]
pub fn build_sandbox_exec_dry_run(
    config: &Config,
    project_dir: &Path,
    verbose: bool,
) -> (String, String) {
    let enable_docker = config.no_docker != Some(true);
    let profile = generate_sbpl_profile(config, project_dir, enable_docker);

    if verbose {
        crate::output::verbose("SBPL profile:");
        for line in profile.lines() {
            crate::output::verbose(&format!("  {line}"));
        }
    }

    let full_cmd = build_shell_command(config);
    let command_line = format!(
        "sandbox-exec -p '<profile>' bash -c '{full_cmd}'"
    );
    (command_line, profile)
}

#[cfg(target_os = "macos")]
pub fn format_dry_run_macos(command_line: &str, profile: &str) -> String {
    let mut out = String::new();
    out.push_str("# sandbox-exec command:\n");
    out.push_str(command_line);
    out.push('\n');
    out.push_str("\n# SBPL profile:\n");
    out.push_str(profile);
    out
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── shell_join tests (cross-platform) ───────────────────────

    #[test]
    fn shell_join_simple() {
        let parts = vec!["claude".to_string()];
        assert_eq!(shell_join(&parts), "claude");
    }

    #[test]
    fn shell_join_multiple_words() {
        let parts = vec!["claude".into(), "--model".into(), "opus".into()];
        assert_eq!(shell_join(&parts), "claude --model opus");
    }

    #[test]
    fn shell_join_with_spaces() {
        let parts = vec!["echo".into(), "hello world".into()];
        assert_eq!(shell_join(&parts), "echo 'hello world'");
    }

    #[test]
    fn shell_join_with_single_quotes() {
        let parts = vec!["echo".into(), "it's".into()];
        assert_eq!(shell_join(&parts), "echo 'it'\\''s'");
    }

    #[test]
    fn shell_join_empty() {
        let parts: Vec<String> = vec![];
        assert_eq!(shell_join(&parts), "");
    }

    // ── mise_init_cmd tests (cross-platform) ────────────────────

    #[test]
    fn mise_init_cmd_format() {
        let cmd = mise_init_cmd(Path::new("/usr/bin/mise"));
        assert!(cmd.contains("/usr/bin/mise trust"));
        assert!(cmd.contains("/usr/bin/mise activate bash"));
        assert!(cmd.contains("/usr/bin/mise env"));
    }

    // ── build_shell_command tests (cross-platform) ──────────────

    #[test]
    fn build_shell_command_default_is_bash() {
        let config = Config {
            no_mise: Some(true),
            ..Config::default()
        };
        let cmd = build_shell_command(&config);
        assert_eq!(cmd, "true && bash");
    }

    #[test]
    fn build_shell_command_with_command() {
        let config = Config {
            command: vec!["claude".into()],
            no_mise: Some(true),
            ..Config::default()
        };
        let cmd = build_shell_command(&config);
        assert_eq!(cmd, "true && claude");
    }

    // ── format_dry_run tests (cross-platform) ───────────────────

    #[test]
    fn format_dry_run_basic() {
        let args: Vec<String> = vec![
            "bwrap".into(),
            "--ro-bind".into(), "/usr".into(), "/usr".into(),
            "--tmpfs".into(), "/tmp".into(),
            "bash".into(), "-c".into(), "true && bash".into(),
        ];
        let output = format_dry_run(&args);
        assert!(output.starts_with("bwrap \\\n"));
        assert!(output.contains("--ro-bind /usr /usr"));
        assert!(output.contains("--tmpfs /tmp"));
        assert!(output.contains("bash -c"));
    }

    #[test]
    fn format_dry_run_empty() {
        let args: Vec<String> = vec![];
        let output = format_dry_run(&args);
        assert!(output.is_empty());
    }

    // ── Linux-only tests ────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_ro_bind() {
        let mounts = vec![Mount::RoBind {
            src: "/usr".into(),
            dest: "/usr".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--ro-bind", "/usr", "/usr"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_bind() {
        let mounts = vec![Mount::Bind {
            src: "/tmp".into(),
            dest: "/tmp".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--bind", "/tmp", "/tmp"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_dev_bind() {
        let mounts = vec![Mount::DevBind {
            src: "/dev/dri".into(),
            dest: "/dev/dri".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--dev-bind", "/dev/dri", "/dev/dri"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_dev() {
        let mounts = vec![Mount::Dev {
            dest: "/dev".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--dev", "/dev"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_proc() {
        let mounts = vec![Mount::Proc {
            dest: "/proc".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--proc", "/proc"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_tmpfs() {
        let mounts = vec![Mount::Tmpfs {
            dest: "/tmp".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--tmpfs", "/tmp"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_symlink() {
        let mounts = vec![Mount::Symlink {
            src: "usr/bin".into(),
            dest: "/bin".into(),
        }];
        let args = mount_args(&mounts);
        assert_eq!(args, vec!["--symlink", "usr/bin", "/bin"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_multiple() {
        let mounts = vec![
            Mount::RoBind {
                src: "/usr".into(),
                dest: "/usr".into(),
            },
            Mount::Tmpfs {
                dest: "/tmp".into(),
            },
        ];
        let args = mount_args(&mounts);
        assert_eq!(
            args,
            vec!["--ro-bind", "/usr", "/usr", "--tmpfs", "/tmp"]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_args_empty() {
        let mounts: Vec<Mount> = vec![];
        let args = mount_args(&mounts);
        assert!(args.is_empty());
    }

    // ── Linux dry_run integration tests ─────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_contains_isolation_flags() {
        let config = Config {
            command: vec!["bash".into()],
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        assert!(args.contains(&"--die-with-parent".to_string()));
        assert!(args.contains(&"--unshare-pid".to_string()));
        assert!(args.contains(&"--unshare-uts".to_string()));
        assert!(args.contains(&"--unshare-ipc".to_string()));
        assert!(args.contains(&"--hostname".to_string()));
        assert!(args.contains(&"ai-sandbox".to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_contains_project_dir() {
        let config = Config {
            command: vec!["bash".into()],
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        // Should have --bind /home/user/project /home/user/project
        let project_str = "/home/user/project".to_string();
        let bind_idx = args
            .windows(3)
            .position(|w| w[0] == "--bind" && w[1] == project_str && w[2] == project_str);
        assert!(bind_idx.is_some(), "Project dir should be bound rw");

        // Should have --chdir /home/user/project
        let chdir_idx = args
            .windows(2)
            .position(|w| w[0] == "--chdir" && w[1] == project_str);
        assert!(chdir_idx.is_some(), "Should chdir to project dir");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_no_gpu_excludes_gpu_mounts() {
        let config = Config {
            command: vec!["bash".into()],
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        let has_gpu_dev_bind = args.windows(3).any(|w| {
            w[0] == "--dev-bind"
                && (w[1].contains("nvidia") || w[1].contains("/dev/dri"))
        });
        assert!(!has_gpu_dev_bind, "GPU disabled: no --dev-bind for GPU devices expected");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_no_docker_excludes_docker_socket() {
        let config = Config {
            command: vec!["bash".into()],
            no_docker: Some(true),
            no_gpu: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        let has_docker = args.iter().any(|a| a.contains("docker.sock"));
        assert!(!has_docker, "Docker disabled: no docker socket expected");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_no_display_excludes_display_env() {
        let config = Config {
            command: vec!["bash".into()],
            no_display: Some(true),
            no_gpu: Some(true),
            no_docker: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        let display_idx = args
            .windows(3)
            .any(|w| w[0] == "--setenv" && (w[1] == "DISPLAY" || w[1] == "WAYLAND_DISPLAY"));
        assert!(!display_idx, "Display disabled: no DISPLAY env expected");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_mise_disabled_uses_true_prefix() {
        let config = Config {
            command: vec!["claude".into()],
            no_mise: Some(true),
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        let last = args.last().unwrap();
        assert!(last.starts_with("true && "), "Mise disabled: should use 'true' prefix, got: {last}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_default_command_is_bash() {
        let config = Config {
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);
        let last = args.last().unwrap();
        assert!(last.ends_with("bash"), "Default command should be bash, got: {last}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_env_vars_present() {
        let config = Config {
            command: vec!["bash".into()],
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        let has_ps1 = args.windows(3).any(|w| w[0] == "--setenv" && w[1] == "PS1");
        assert!(has_ps1, "PS1 env var should be set");

        let has_zo = args.windows(3).any(|w| w[0] == "--setenv" && w[1] == "_ZO_DOCTOR" && w[2] == "0");
        assert!(has_zo, "_ZO_DOCTOR env var should be set to 0");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dry_run_extra_rw_maps_present() {
        let config = Config {
            command: vec!["bash".into()],
            rw_maps: vec![PathBuf::from("/tmp")],
            no_gpu: Some(true),
            no_docker: Some(true),
            no_display: Some(true),
            no_mise: Some(true),
            ..Config::default()
        };
        let hosts = PathBuf::from("/tmp/test-hosts");
        let project = PathBuf::from("/home/user/project");

        let args = build_bwrap_dry_run(&config, &project, &hosts, false);

        let has_tmp_bind = args
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == "/tmp" && w[2] == "/tmp");
        assert!(has_tmp_bind, "Extra rw map /tmp should be present");
    }

    // ── macOS-specific tests ────────────────────────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_profile_has_deny_default() {
        let config = Config {
            command: vec!["bash".into()],
            no_mise: Some(true),
            ..Config::default()
        };
        let project = PathBuf::from("/tmp/test-project");
        let profile = generate_sbpl_profile(&config, &project, false);
        assert!(profile.contains("(deny default)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_profile_allows_file_read() {
        let config = Config::default();
        let project = PathBuf::from("/tmp/test-project");
        let profile = generate_sbpl_profile(&config, &project, false);
        assert!(profile.contains("(allow file-read*)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_profile_denies_ssh() {
        let config = Config::default();
        let project = PathBuf::from("/tmp/test-project");
        let profile = generate_sbpl_profile(&config, &project, false);
        let home = mounts::home_dir();
        if home.join(".ssh").exists() {
            assert!(profile.contains(".ssh"));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_profile_allows_project_write() {
        let config = Config::default();
        let project = PathBuf::from("/tmp/test-project");
        let profile = generate_sbpl_profile(&config, &project, false);
        assert!(profile.contains("file-write*"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_profile_allows_network() {
        let config = Config::default();
        let project = PathBuf::from("/tmp/test-project");
        let profile = generate_sbpl_profile(&config, &project, false);
        assert!(profile.contains("(allow network-outbound)"));
        assert!(profile.contains("(allow network-inbound)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dry_run_macos_output() {
        let config = Config {
            command: vec!["bash".into()],
            no_mise: Some(true),
            ..Config::default()
        };
        let project = PathBuf::from("/tmp/test-project");
        let (cmd_line, profile) = build_sandbox_exec_dry_run(&config, &project, false);
        let output = format_dry_run_macos(&cmd_line, &profile);
        assert!(output.contains("sandbox-exec"));
        assert!(output.contains("SBPL profile"));
    }
}
