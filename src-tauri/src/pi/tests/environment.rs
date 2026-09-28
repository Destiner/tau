use super::*;

#[test]
fn dev_session_arguments_keep_new_and_resumed_sessions_in_the_profile() {
    let profile = crate::profile::StorageProfile::ephemeral().unwrap();
    let directory = profile.session_dir("/imported/project", Path::new("/real/pi"));
    let mut fresh = Command::new("pi");
    configure_session_arguments(&mut fresh, Some(&directory), None);
    assert_eq!(
        fresh.get_args().collect::<Vec<_>>(),
        vec![OsStr::new("--session-dir"), directory.as_os_str()]
    );
    let saved = directory.join("saved.jsonl");
    let mut resumed = Command::new("pi");
    configure_session_arguments(&mut resumed, Some(&directory), saved.to_str());
    assert_eq!(
        resumed.get_args().collect::<Vec<_>>(),
        vec![
            OsStr::new("--session-dir"),
            directory.as_os_str(),
            OsStr::new("--session"),
            saved.as_os_str()
        ]
    );

    {
        let mut fresh = Command::new("pi");
        configure_session_arguments(&mut fresh, None, None);
        assert_eq!(fresh.get_args().count(), 0);
        let mut resumed = Command::new("pi");
        configure_session_arguments(&mut resumed, None, Some("/real/pi/saved.jsonl"));
        assert_eq!(
            resumed.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("--session"), OsStr::new("/real/pi/saved.jsonl")]
        );
    }
}

#[test]
fn configured_pi_path_takes_priority() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current = std::env::current_exe().expect("current executable");
    let _environment = EnvironmentGuard::set(&[("TAU_PI_PATH", Some(current.as_os_str()))]);
    assert_eq!(resolve_pi_binary(), Some(current));

    {
        let directory = tempfile::tempdir().expect("temporary directory");
        let executable = directory.path().join("pi");
        std::fs::write(&executable, "").expect("executable");
        #[cfg(unix)]
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("executable permissions");
        let path =
            env::join_paths([Path::new("/nonexistent"), directory.path()]).expect("search PATH");

        assert_eq!(find_on_path("pi", Some(&path)), Some(executable));
    }

    {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("pi");
        std::fs::write(&path, "").expect("plain file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("plain file permissions");
        assert!(!is_executable_file(&path));
    }
}

#[test]
fn child_path_prepends_and_deduplicates_executable_directories() {
    let path = build_child_path(
        &[
            Path::new("/opt/homebrew/bin/pi"),
            Path::new("/opt/homebrew/bin/node"),
        ],
        None,
        Some(OsStr::new("/usr/bin:/bin")),
    )
    .expect("child PATH");
    assert_eq!(
        env::split_paths(&path).collect::<Vec<_>>(),
        vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ],
    );

    let path = build_child_path(
        &[Path::new("/opt/homebrew/bin/pi")],
        Some(OsStr::new("/Users/tau/.bun/bin:/usr/bin")),
        Some(OsStr::new("/usr/bin:/bin")),
    )
    .expect("child PATH");
    assert_eq!(
        env::split_paths(&path).collect::<Vec<_>>(),
        vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/Users/tau/.bun/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ],
    );

    {
        let output = format!(
        "startup banner\n{LOGIN_SHELL_PATH_MARKER}\n/Users/tau/.bun/bin:/usr/bin\n{LOGIN_SHELL_AGENT_DIR_MARKER}\n/Users/tau/.config/pi\n{LOGIN_SHELL_END_MARKER}\n"
    );
        assert_eq!(
            parse_login_shell_path(&output),
            Some(OsString::from("/Users/tau/.bun/bin:/usr/bin")),
        );
        assert_eq!(
            parse_login_shell_environment(&output)
                .expect("login shell environment")
                .agent_dir,
            Some(OsString::from("/Users/tau/.config/pi")),
        );
        assert_eq!(parse_login_shell_path("command not found"), None);
        assert_eq!(
            parse_login_shell_path(&format!(
            "{LOGIN_SHELL_PATH_MARKER}  {LOGIN_SHELL_AGENT_DIR_MARKER}  {LOGIN_SHELL_END_MARKER}"
        )),
            None,
        );
    }

    {
        for arguments in LOGIN_SHELL_ARGUMENTS
            .into_iter()
            .chain(CSH_LOGIN_SHELL_ARGUMENTS)
        {
            let last = arguments.last().expect("argument");
            assert!(
                last.ends_with('c'),
                "{last} would not treat the next argument as a command",
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn capturing_stdout_gives_up_on_a_hanging_shell() {
    let mut command = Command::new("sh");
    command.args(["-c", "exec 1>&-; sleep 30"]);
    assert_eq!(capture_stdout(command, Duration::from_millis(200)), None);

    {
        assert_eq!(read_bounded_stdout(&b"12345"[..], 4), None);
        assert_eq!(read_bounded_stdout(&b"1234"[..], 4), Some(b"1234".to_vec()),);
    }
}
