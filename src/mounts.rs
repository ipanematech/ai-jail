use crate::output;
use std::path::{Path, PathBuf};

// Dotdirs never mounted (sensitive data)
const DOTDIR_DENY: &[&str] = &[".gnupg", ".aws", ".ssh", ".mozilla", ".basilisk-dev", ".sparrow"];

// Dotdirs requiring read-write access
const DOTDIR_RW: &[&str] = &[
    ".claude", ".crush", ".codex", ".aider", ".config", ".cargo", ".cache", ".docker",
];

// Subdirs of ~/.config to hide (tmpfs over rw config mount)
#[cfg(target_os = "linux")]
const CONFIG_DENY: &[&str] = &["BraveSoftware", "Bitwarden"];

// Subdirs of ~/.cache to hide
#[cfg(target_os = "linux")]
const CACHE_DENY: &[&str] = &[
    "BraveSoftware",
    "basilisk-dev",
    "chromium",
    "spotify",
    "nvidia",
    "mesa_shader_cache",
];

// ~/.local/share subdirs to mount rw
#[cfg(target_os = "linux")]
const LOCAL_SHARE_RW: &[&str] = &[
    "zoxide", "crush", "opencode", "atuin", "mise", "yarn", "flutter", "kotlin", "NuGet",
    "pipx", "ruby-advisory-db", "uv",
];

#[derive(Debug, Clone)]
pub enum Mount {
    RoBind { src: PathBuf, dest: PathBuf },
    Bind { src: PathBuf, dest: PathBuf },
    DevBind { src: PathBuf, dest: PathBuf },
    Dev { dest: PathBuf },
    Proc { dest: PathBuf },
    Tmpfs { dest: PathBuf },
    Symlink { src: String, dest: PathBuf },
    FileRoBind { src: PathBuf, dest: PathBuf },
}

pub struct MountSet {
    pub base: Vec<Mount>,
    pub home_dotfiles: Vec<Mount>,
    pub config_hide: Vec<Mount>,
    pub cache_hide: Vec<Mount>,
    pub local_overrides: Vec<Mount>,
    pub gpu: Vec<Mount>,
    pub docker: Vec<Mount>,
    pub shm: Vec<Mount>,
    pub display: Vec<Mount>,
    pub display_env: Vec<(String, String)>,
    pub extra: Vec<Mount>,
    pub project: Vec<Mount>,
}

pub fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}

fn path_exists(p: &Path) -> bool {
    p.exists() || p.symlink_metadata().is_ok()
}

#[cfg(target_os = "linux")]
pub fn discover_base(hosts_file: &Path) -> Vec<Mount> {
    vec![
        Mount::RoBind {
            src: "/usr".into(),
            dest: "/usr".into(),
        },
        Mount::Symlink {
            src: "usr/bin".into(),
            dest: "/bin".into(),
        },
        Mount::Symlink {
            src: "usr/lib".into(),
            dest: "/lib".into(),
        },
        Mount::Symlink {
            src: "usr/lib".into(),
            dest: "/lib64".into(),
        },
        Mount::RoBind {
            src: "/etc".into(),
            dest: "/etc".into(),
        },
        Mount::FileRoBind {
            src: hosts_file.to_path_buf(),
            dest: "/etc/hosts".into(),
        },
        Mount::RoBind {
            src: "/opt".into(),
            dest: "/opt".into(),
        },
        Mount::RoBind {
            src: "/sys".into(),
            dest: "/sys".into(),
        },
        Mount::Dev {
            dest: "/dev".into(),
        },
        Mount::Proc {
            dest: "/proc".into(),
        },
        Mount::Tmpfs {
            dest: "/tmp".into(),
        },
        Mount::Tmpfs {
            dest: "/run".into(),
        },
    ]
}

#[cfg(target_os = "linux")]
pub fn discover_home_dotfiles(verbose: bool) -> Vec<Mount> {
    let home = home_dir();
    let mut mounts = vec![Mount::Tmpfs { dest: home.clone() }];

    // Discover dot-directories
    let entries = match std::fs::read_dir(&home) {
        Ok(e) => e,
        Err(e) => {
            output::warn(&format!("Cannot read home directory: {e}"));
            return mounts;
        }
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with('.') {
            continue;
        }
        if name_str == "." || name_str == ".." {
            continue;
        }

        let path = entry.path();

        // Only directories
        if !path.is_dir() {
            continue;
        }

        if DOTDIR_DENY.contains(&name_str.as_ref()) {
            if verbose {
                output::verbose(&format!("deny: {}", path.display()));
            }
            continue;
        }

        let dest = home.join(&name_str.as_ref());
        if DOTDIR_RW.contains(&name_str.as_ref()) {
            if verbose {
                output::verbose(&format!("rw: {}", path.display()));
            }
            mounts.push(Mount::Bind {
                src: path,
                dest,
            });
        } else {
            if verbose {
                output::verbose(&format!("ro: {}", path.display()));
            }
            mounts.push(Mount::RoBind {
                src: path,
                dest,
            });
        }
    }

    // Explicit dotfile mounts (regular files)
    let gitconfig = home.join(".gitconfig");
    if gitconfig.is_file() {
        mounts.push(Mount::RoBind {
            src: gitconfig.clone(),
            dest: gitconfig,
        });
    }
    let claude_json = home.join(".claude.json");
    if claude_json.is_file() {
        mounts.push(Mount::Bind {
            src: claude_json.clone(),
            dest: claude_json,
        });
    }

    mounts
}

#[cfg(target_os = "linux")]
pub fn discover_config_hide() -> Vec<Mount> {
    let home = home_dir();
    CONFIG_DENY
        .iter()
        .filter_map(|name| {
            let path = home.join(".config").join(name);
            if path.is_dir() {
                Some(Mount::Tmpfs { dest: path })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub fn discover_cache_hide() -> Vec<Mount> {
    let home = home_dir();
    CACHE_DENY
        .iter()
        .filter_map(|name| {
            let path = home.join(".cache").join(name);
            if path.is_dir() {
                Some(Mount::Tmpfs { dest: path })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub fn discover_local_overrides() -> Vec<Mount> {
    let home = home_dir();
    let mut mounts = Vec::new();

    let state = home.join(".local/state");
    if state.is_dir() {
        mounts.push(Mount::Bind {
            src: state.clone(),
            dest: state,
        });
    }

    for name in LOCAL_SHARE_RW {
        let path = home.join(".local/share").join(name);
        if path.is_dir() {
            mounts.push(Mount::Bind {
                src: path.clone(),
                dest: path,
            });
        }
    }

    mounts
}

#[cfg(target_os = "linux")]
pub fn discover_gpu(verbose: bool) -> Vec<Mount> {
    let mut mounts = Vec::new();

    // /dev/nvidia*
    if let Ok(entries) = std::fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("nvidia") {
                let path = entry.path();
                if verbose {
                    output::verbose(&format!("gpu: {}", path.display()));
                }
                mounts.push(Mount::DevBind {
                    src: path.clone(),
                    dest: path,
                });
            }
        }
    }

    // /dev/dri
    let dri = PathBuf::from("/dev/dri");
    if path_exists(&dri) {
        if verbose {
            output::verbose(&format!("gpu: {}", dri.display()));
        }
        mounts.push(Mount::DevBind {
            src: dri.clone(),
            dest: dri,
        });
    }

    mounts
}

#[cfg(target_os = "linux")]
pub fn discover_docker() -> Vec<Mount> {
    let sock = PathBuf::from("/var/run/docker.sock");
    if path_exists(&sock) {
        vec![Mount::Bind {
            src: sock.clone(),
            dest: sock,
        }]
    } else {
        vec![]
    }
}

#[cfg(target_os = "linux")]
pub fn discover_shm() -> Vec<Mount> {
    let shm = PathBuf::from("/dev/shm");
    if shm.is_dir() {
        vec![Mount::DevBind {
            src: shm.clone(),
            dest: shm,
        }]
    } else {
        vec![]
    }
}

#[cfg(target_os = "linux")]
pub fn discover_display(verbose: bool) -> (Vec<Mount>, Vec<(String, String)>) {
    let mut mounts = Vec::new();
    let mut env = Vec::new();

    // X11/XWayland socket
    let x11 = PathBuf::from("/tmp/.X11-unix");
    if x11.is_dir() {
        mounts.push(Mount::Bind {
            src: x11.clone(),
            dest: x11,
        });
    }

    if let Ok(display) = std::env::var("DISPLAY") {
        env.push(("DISPLAY".into(), display));
    }

    if let Ok(xauth) = std::env::var("XAUTHORITY") {
        let xauth_path = PathBuf::from(&xauth);
        if path_exists(&xauth_path) {
            mounts.push(Mount::RoBind {
                src: xauth_path.clone(),
                dest: xauth_path,
            });
        }
        env.push(("XAUTHORITY".into(), xauth));
    }

    // Wayland / XDG_RUNTIME_DIR
    if let Ok(xdg_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let xdg_path = PathBuf::from(&xdg_dir);
        if xdg_path.is_dir() {
            mounts.push(Mount::Bind {
                src: xdg_path.clone(),
                dest: xdg_path,
            });
            env.push(("XDG_RUNTIME_DIR".into(), xdg_dir));
            if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
                env.push(("WAYLAND_DISPLAY".into(), wayland));
            }
        }
    }

    if verbose {
        for m in &mounts {
            if let Mount::Bind { src, .. } | Mount::RoBind { src, .. } = m {
                output::verbose(&format!("display: {}", src.display()));
            }
        }
    }

    (mounts, env)
}

pub fn extra_mounts(rw_maps: &[PathBuf], ro_maps: &[PathBuf]) -> Vec<Mount> {
    let mut mounts = Vec::new();

    for path in rw_maps {
        if path_exists(path) {
            mounts.push(Mount::Bind {
                src: path.clone(),
                dest: path.clone(),
            });
        } else {
            output::warn(&format!("Path {} not found, skipping.", path.display()));
        }
    }

    for path in ro_maps {
        if path_exists(path) {
            mounts.push(Mount::RoBind {
                src: path.clone(),
                dest: path.clone(),
            });
        } else {
            output::warn(&format!("Path {} not found, skipping.", path.display()));
        }
    }

    mounts
}

pub fn project_mount(project_dir: &Path) -> Vec<Mount> {
    vec![Mount::Bind {
        src: project_dir.to_path_buf(),
        dest: project_dir.to_path_buf(),
    }]
}

// ── macOS-specific path discovery ────────────────────────────────

#[cfg(target_os = "macos")]
pub fn macos_read_deny_paths() -> Vec<PathBuf> {
    let home = home_dir();
    let candidates = [
        home.join(".ssh"),
        home.join(".gnupg"),
        home.join(".aws"),
        home.join(".sparrow"),
        home.join("Library/Keychains"),
        home.join("Library/Mail"),
        home.join("Library/Messages"),
        home.join("Library/Safari"),
        home.join("Library/Cookies"),
    ];
    candidates
        .into_iter()
        .filter(|p| path_exists(p))
        .collect()
}

#[cfg(target_os = "macos")]
pub fn macos_writable_paths(
    project_dir: &Path,
    config: &crate::config::Config,
) -> Vec<PathBuf> {
    let home = home_dir();
    let mut paths = Vec::new();

    // Project directory
    paths.push(project_dir.to_path_buf());

    // AI tool directories
    for name in &[".claude", ".crush", ".codex", ".aider"] {
        let p = home.join(name);
        if path_exists(&p) {
            paths.push(p);
        }
    }

    // Tool infrastructure
    for name in &[".config", ".cargo", ".cache", ".docker", ".local"] {
        let p = home.join(name);
        if path_exists(&p) {
            paths.push(p);
        }
    }

    // .claude.json file (rw)
    let claude_json = home.join(".claude.json");
    if claude_json.is_file() {
        paths.push(claude_json);
    }

    // /tmp and /private/tmp
    paths.push(PathBuf::from("/tmp"));
    paths.push(PathBuf::from("/private/tmp"));

    // User rw-maps
    for p in &config.rw_maps {
        if path_exists(p) {
            paths.push(p.clone());
        }
    }

    paths
}

#[cfg(target_os = "macos")]
pub fn macos_docker_socket() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/var/run/docker.sock"),
        home_dir().join(".docker/run/docker.sock"),
    ];
    candidates.into_iter().find(|p| path_exists(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deny list tests ────────────────────────────────────────

    #[test]
    fn deny_list_contains_sensitive_dirs() {
        for name in &[".gnupg", ".aws", ".ssh", ".mozilla", ".basilisk-dev", ".sparrow"] {
            assert!(
                DOTDIR_DENY.contains(name),
                "{name} should be in deny list"
            );
        }
    }

    #[test]
    fn rw_list_contains_ai_tool_dirs() {
        for name in &[".claude", ".crush", ".codex", ".aider"] {
            assert!(
                DOTDIR_RW.contains(name),
                "{name} should be in rw list"
            );
        }
    }

    #[test]
    fn rw_list_contains_tool_dirs() {
        for name in &[".config", ".cargo", ".cache", ".docker"] {
            assert!(
                DOTDIR_RW.contains(name),
                "{name} should be in rw list"
            );
        }
    }

    #[test]
    fn deny_and_rw_lists_do_not_overlap() {
        for name in DOTDIR_DENY {
            assert!(
                !DOTDIR_RW.contains(name),
                "{name} is in both deny and rw lists"
            );
        }
    }

    // ── Linux-only tests ────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn config_deny_contains_sensitive_apps() {
        assert!(CONFIG_DENY.contains(&"BraveSoftware"));
        assert!(CONFIG_DENY.contains(&"Bitwarden"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cache_deny_contains_browser_caches() {
        assert!(CACHE_DENY.contains(&"BraveSoftware"));
        assert!(CACHE_DENY.contains(&"chromium"));
    }

    // ── Base mounts tests (Linux only) ──────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn base_mounts_has_correct_count() {
        let hosts = PathBuf::from("/tmp/test-hosts");
        let mounts = discover_base(&hosts);
        // /usr, symlink bin, symlink lib, symlink lib64, /etc, hosts, /opt, /sys, /dev, /proc, /tmp, /run
        assert_eq!(mounts.len(), 12);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn base_mounts_starts_with_usr() {
        let hosts = PathBuf::from("/tmp/test-hosts");
        let mounts = discover_base(&hosts);
        match &mounts[0] {
            Mount::RoBind { src, dest } => {
                assert_eq!(src, &PathBuf::from("/usr"));
                assert_eq!(dest, &PathBuf::from("/usr"));
            }
            _ => panic!("First mount should be ro-bind /usr"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn base_mounts_includes_hosts_file() {
        let hosts = PathBuf::from("/tmp/my-hosts-file");
        let mounts = discover_base(&hosts);
        let has_hosts = mounts.iter().any(|m| match m {
            Mount::FileRoBind { src, dest } => {
                src == &PathBuf::from("/tmp/my-hosts-file")
                    && dest == &PathBuf::from("/etc/hosts")
            }
            _ => false,
        });
        assert!(has_hosts, "Base mounts should include custom hosts file");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn base_mounts_has_symlinks() {
        let hosts = PathBuf::from("/tmp/test-hosts");
        let mounts = discover_base(&hosts);
        let symlinks: Vec<_> = mounts
            .iter()
            .filter_map(|m| match m {
                Mount::Symlink { src, dest } => Some((src.clone(), dest.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(symlinks.len(), 3);
        assert!(symlinks.contains(&("usr/bin".into(), PathBuf::from("/bin"))));
        assert!(symlinks.contains(&("usr/lib".into(), PathBuf::from("/lib"))));
        assert!(symlinks.contains(&("usr/lib".into(), PathBuf::from("/lib64"))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn base_mounts_has_tmpfs_for_tmp_and_run() {
        let hosts = PathBuf::from("/tmp/test-hosts");
        let mounts = discover_base(&hosts);
        let tmpfs_paths: Vec<_> = mounts
            .iter()
            .filter_map(|m| match m {
                Mount::Tmpfs { dest } => Some(dest.clone()),
                _ => None,
            })
            .collect();
        assert!(tmpfs_paths.contains(&PathBuf::from("/tmp")));
        assert!(tmpfs_paths.contains(&PathBuf::from("/run")));
    }

    // ── SHM tests (Linux only) ──────────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn shm_mount_when_exists() {
        let mounts = discover_shm();
        if PathBuf::from("/dev/shm").is_dir() {
            assert_eq!(mounts.len(), 1);
            match &mounts[0] {
                Mount::DevBind { src, dest } => {
                    assert_eq!(src, &PathBuf::from("/dev/shm"));
                    assert_eq!(dest, &PathBuf::from("/dev/shm"));
                }
                _ => panic!("SHM should be dev-bind"),
            }
        } else {
            assert!(mounts.is_empty());
        }
    }

    // ── Extra mounts tests (cross-platform) ─────────────────────

    #[test]
    fn extra_mounts_existing_rw_path() {
        // /tmp always exists
        let mounts = extra_mounts(&[PathBuf::from("/tmp")], &[]);
        assert_eq!(mounts.len(), 1);
        match &mounts[0] {
            Mount::Bind { src, dest } => {
                assert_eq!(src, &PathBuf::from("/tmp"));
                assert_eq!(dest, &PathBuf::from("/tmp"));
            }
            _ => panic!("Expected Bind mount"),
        }
    }

    #[test]
    fn extra_mounts_existing_ro_path() {
        let mounts = extra_mounts(&[], &[PathBuf::from("/tmp")]);
        assert_eq!(mounts.len(), 1);
        match &mounts[0] {
            Mount::RoBind { src, dest } => {
                assert_eq!(src, &PathBuf::from("/tmp"));
                assert_eq!(dest, &PathBuf::from("/tmp"));
            }
            _ => panic!("Expected RoBind mount"),
        }
    }

    #[test]
    fn extra_mounts_nonexistent_path_skipped() {
        let mounts = extra_mounts(
            &[PathBuf::from("/nonexistent/path/that/should/not/exist")],
            &[],
        );
        assert!(mounts.is_empty());
    }

    // ── Project mount tests (cross-platform) ────────────────────

    #[test]
    fn project_mount_is_rw_bind() {
        let mounts = project_mount(Path::new("/home/user/project"));
        assert_eq!(mounts.len(), 1);
        match &mounts[0] {
            Mount::Bind { src, dest } => {
                assert_eq!(src, &PathBuf::from("/home/user/project"));
                assert_eq!(dest, &PathBuf::from("/home/user/project"));
            }
            _ => panic!("Project mount should be a rw bind"),
        }
    }

    // ── macOS-specific tests ────────────────────────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_read_deny_includes_ssh() {
        // ~/.ssh almost always exists on dev machines
        let paths = macos_read_deny_paths();
        let has_ssh = paths.iter().any(|p| p.ends_with(".ssh"));
        // Only assert if .ssh exists
        let ssh_dir = home_dir().join(".ssh");
        if ssh_dir.exists() {
            assert!(has_ssh, "Should deny reads to ~/.ssh");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_writable_paths_includes_project() {
        let config = crate::config::Config::default();
        let project = PathBuf::from("/tmp/test-project");
        let paths = macos_writable_paths(&project, &config);
        assert!(paths.contains(&project), "Project dir must be writable");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_writable_paths_includes_tmp() {
        let config = crate::config::Config::default();
        let project = PathBuf::from("/tmp/test-project");
        let paths = macos_writable_paths(&project, &config);
        assert!(paths.contains(&PathBuf::from("/tmp")));
        assert!(paths.contains(&PathBuf::from("/private/tmp")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_writable_paths_includes_rw_maps() {
        let config = crate::config::Config {
            rw_maps: vec![PathBuf::from("/tmp")],
            ..crate::config::Config::default()
        };
        let project = PathBuf::from("/tmp/test-project");
        let paths = macos_writable_paths(&project, &config);
        assert!(paths.contains(&PathBuf::from("/tmp")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_docker_socket_returns_none_when_missing() {
        // This test just verifies the function doesn't panic
        let _ = macos_docker_socket();
    }
}
