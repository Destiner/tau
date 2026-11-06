use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SOURCE: &str = include_str!("../pi-extensions/tau-reload.ts");
const FAILURE: &str = "Could not prepare Tau's private Pi helper. Check application storage permissions and try again.";

fn revision() -> String {
    format!("reload-v1-{:x}", Sha256::digest(SOURCE.as_bytes()))
}

fn private_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::other("unexpected helper directory type"));
    }
    #[cfg(unix)]
    if metadata.mode() & 0o777 != 0o700 || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::other("helper directory is not private"));
    }
    Ok(())
}

fn verify(path: &Path) -> std::io::Result<()> {
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != SOURCE.len() as u64 {
        return Err(std::io::Error::other("unexpected helper file"));
    }
    #[cfg(unix)]
    if metadata.mode() & 0o777 != 0o400 || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::other(
            "helper file is not private and immutable",
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes != SOURCE.as_bytes() {
        return Err(std::io::Error::other("helper contents differ"));
    }
    Ok(())
}

pub fn provision(data_dir: &Path) -> Result<PathBuf, String> {
    provision_inner(data_dir).map_err(|_| FAILURE.to_string())
}

fn provision_inner(data_dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(data_dir)?;
    if !fs::symlink_metadata(data_dir)?.is_dir() {
        return Err(std::io::Error::other("unexpected storage directory type"));
    }
    let root = data_dir.join("pi-helpers");
    private_directory(&root)?;
    let directory = root.join(revision());
    private_directory(&directory)?;
    let destination = directory.join("tau-reload.ts");
    match fs::symlink_metadata(&destination) {
        Ok(_) => verify(&destination)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut staging = tempfile::NamedTempFile::new_in(&directory)?;
            staging.write_all(SOURCE.as_bytes())?;
            #[cfg(unix)]
            staging
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o400))?;
            staging.as_file().sync_all()?;
            match staging.persist_noclobber(&destination) {
                Ok(_) => (),
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(error.error),
            }
            verify(&destination)?;
        }
        Err(error) => return Err(error),
    }
    destination.canonicalize()
}

// The login shell only interprets a quoted /bin/sh invocation; RPC stdin is never used.
pub fn remote_bootstrap(working_directory: &str, session_path: Option<&str>) -> String {
    let script = format!(
        r#"set -eu
umask 077
fail() {{ printf '%s\n' 'Tau could not prepare its private Pi helper.' >&2; exit 1; }}
[ -n "${{HOME:-}}" ] && [ "${{HOME#/}}" != "$HOME" ] || fail
for dir in "$HOME/.cache" "$HOME/.cache/tau" "$HOME/.cache/tau/pi-helpers" "$HOME/.cache/tau/pi-helpers/{revision}"; do
  [ ! -L "$dir" ] || fail
  if [ ! -d "$dir" ]; then mkdir "$dir" 2>/dev/null || [ -d "$dir" ] || fail; fi
  [ ! -L "$dir" ] && [ -d "$dir" ] || fail
  case "$dir" in "$HOME/.cache") ;; *) chmod 700 "$dir" || fail ;; esac
done
root="$HOME/.cache/tau/pi-helpers/{revision}"
helper="$root/tau-reload.ts"
staging=$(mktemp "$root/.staging.XXXXXXXX") || fail
trap 'rm -f "$staging"' EXIT HUP INT TERM
printf '%s' "$1" > "$staging" || fail
chmod 400 "$staging" || fail
if [ ! -e "$helper" ] && [ ! -L "$helper" ]; then
  ln "$staging" "$helper" 2>/dev/null || [ -f "$helper" ] || fail
fi
[ ! -L "$helper" ] && [ -f "$helper" ] && cmp -s "$staging" "$helper" || fail
chmod 400 "$helper" || fail
rm -f "$staging"
trap - EXIT HUP INT TERM
cd "$2" || exit 1
shift 2
exec pi --mode rpc --extension "$helper" "$@""#,
        revision = revision(),
    );
    let mut command = format!(
        "exec /bin/sh -c {} tau {} {}",
        shell_words::quote(&script),
        shell_words::quote(SOURCE),
        shell_words::quote(working_directory),
    );
    if let Some(path) = session_path {
        command.push_str(" --session ");
        command.push_str(&shell_words::quote(path));
    }
    // csh cannot carry literal newlines inside quoted arguments and expands
    // history markers even in quotes. Decode only after entering POSIX sh.
    let encoded = command
        .bytes()
        .map(|byte| format!("\\0{byte:03o}"))
        .collect::<String>();
    let decoder = format!("exec /bin/sh -c \"$(printf '%b' '{encoded}')\"");
    format!("exec /bin/sh -c {}", shell_words::quote(&decoder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_private_immutable_bytes_and_reuses_concurrently() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("app");
        let paths = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| provision(&root).unwrap()))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(paths.iter().all(|path| path == &paths[0]));
        assert!(paths[0].starts_with(root.canonicalize().unwrap()));
        assert_eq!(fs::read_to_string(&paths[0]).unwrap(), SOURCE);
        #[cfg(unix)]
        {
            assert_eq!(fs::metadata(&paths[0]).unwrap().mode() & 0o777, 0o400);
            assert_eq!(
                fs::metadata(paths[0].parent().unwrap()).unwrap().mode() & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    fn remote_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let pi = bin.join("pi");
        fs::write(&pi, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/args\"\nprintf '%s\\n' \"$PWD\" > \"$HOME/cwd\"\ncat\n").unwrap();
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o700)).unwrap();
        let project = home.path().join("project ' ; $(touch NO) space");
        fs::create_dir(&project).unwrap();
        (home, bin, project)
    }

    #[cfg(unix)]
    fn run_remote(home: &Path, bin: &Path, project: &Path) -> std::process::Child {
        std::process::Command::new("/bin/sh")
            .args([
                "-c",
                &remote_bootstrap(
                    project.to_str().unwrap(),
                    Some("/sessions/a '; $(touch BAD).jsonl"),
                ),
            ])
            .env("HOME", home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn remote_bootstrap_preserves_exact_bytes_arguments_and_rpc_stdin() {
        let (home, bin, project) = remote_fixture();
        for _ in 0..3 {
            let mut child = run_remote(home.path(), &bin, &project);
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"{\"id\":\"first-record\"}\n")
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, b"{\"id\":\"first-record\"}\n");
            assert!(output.stderr.is_empty());
        }
        let helper = home
            .path()
            .join(".cache/tau/pi-helpers")
            .join(revision())
            .join("tau-reload.ts");
        assert_eq!(fs::read_to_string(&helper).unwrap(), SOURCE);
        let args = fs::read_to_string(home.path().join("args")).unwrap();
        assert_eq!(
            args,
            format!(
                "--mode\nrpc\n--extension\n{}\n--session\n/sessions/a '; $(touch BAD).jsonl\n",
                helper.display()
            )
        );
        assert_eq!(
            fs::read_to_string(home.path().join("cwd")).unwrap().trim(),
            project.to_str().unwrap()
        );
        assert!(!project.join("BAD").exists());
        assert!(!project.join("NO").exists());
        assert!(!home.path().join(".pi").exists());
    }

    #[cfg(unix)]
    #[test]
    fn remote_bootstrap_crosses_csh_login_boundary() {
        let shell = Path::new("/bin/csh");
        if !shell.is_file() {
            return;
        }
        let (home, bin, project) = remote_fixture();
        fs::write(
            home.path().join(".login"),
            format!(
                "setenv PATH {}:/usr/bin:/bin\n",
                shell_words::quote(bin.to_str().unwrap())
            ),
        )
        .unwrap();
        let output = std::process::Command::new(shell)
            .args([
                "-c",
                &crate::ssh::remote_pi_command(project.to_str().unwrap(), None),
            ])
            .env("SHELL", shell)
            .env("HOME", home.path())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(fs::read_to_string(home.path().join("args"))
            .unwrap()
            .contains("--extension\n"));
    }

    #[cfg(unix)]
    #[test]
    fn remote_publication_is_concurrent_and_failures_never_start_pi() {
        let (home, bin, project) = remote_fixture();
        let children = (0..8)
            .map(|_| run_remote(home.path(), &bin, &project))
            .collect::<Vec<_>>();
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stdout.is_empty());
        }
        fs::remove_file(home.path().join("args")).unwrap();
        let helper = home
            .path()
            .join(".cache/tau/pi-helpers")
            .join(revision())
            .join("tau-reload.ts");
        fs::remove_file(&helper).unwrap();
        let target = home.path().join("outside.ts");
        fs::write(&target, SOURCE).unwrap();
        std::os::unix::fs::symlink(&target, &helper).unwrap();
        let output = run_remote(home.path(), &bin, &project)
            .wait_with_output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!home.path().join("args").exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("private Pi helper"));
        assert!(fs::read_dir(helper.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".staging.")));
        fs::remove_file(&helper).unwrap();
        fs::write(&helper, "wrong bytes").unwrap();
        let output = run_remote(home.path(), &bin, &project)
            .wait_with_output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!home.path().join("args").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_managed_directory_symlinks_and_insecure_permissions() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = home.path().join("pi-helpers");
        std::os::unix::fs::symlink(outside.path(), &root).unwrap();
        assert!(provision(home.path()).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        fs::remove_file(&root).unwrap();
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(provision(home.path()).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let path = provision(home.path()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(provision(home.path()).is_err());
        fs::write(&path, SOURCE.replace("reload-v1", "reload-v2")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(provision(home.path()).is_err());
    }

    #[test]
    fn rejects_wrong_contents_and_managed_types() {
        let home = tempfile::tempdir().unwrap();
        let path = provision(home.path()).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, "wrong").unwrap();
        assert!(provision(home.path()).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(provision(home.path()).is_err());
        fs::remove_dir(&path).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.path().join("missing"), &path).unwrap();
            assert!(provision(home.path()).is_err());
        }
        let blocked = home.path().join("blocked");
        fs::write(&blocked, "file").unwrap();
        assert!(provision(&blocked).is_err());
    }
}
