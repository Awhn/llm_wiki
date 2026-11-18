//! Git plumbing for a single, app-managed wiki worktree. The command adapter
//! serializes calls; this module also runs against bare repositories in tests.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Fault>;

#[derive(Debug, Serialize)]
pub struct Fault {
    pub kind: &'static str,
    pub message: String,
    pub files: Vec<String>,
}
fn fault(kind: &'static str, message: &str) -> Fault {
    Fault {
        kind,
        message: message.into(),
        files: vec![],
    }
}
fn invalid(message: &str) -> Fault {
    fault("Error", message)
}
impl From<io::Error> for Fault {
    fn from(e: io::Error) -> Self {
        invalid(&e.to_string())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub remote: String,
    pub branch: String,
    pub enabled: bool,
    pub last_sync: Option<u64>,
    pub backup: Option<String>,
}
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Changes {
    pub token: String,
    pub paths: BTreeSet<String>,
}
pub fn allowed(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|p| !p.is_empty() && !p.starts_with('.'))
        && !["media/", "raw/"]
            .iter()
            .any(|prefix| path.to_ascii_lowercase().starts_with(prefix))
}
fn validate_id(id: &str) -> Result<()> {
    if id.len() > 64
        || !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(invalid("Use 1–64 letters, digits, dots, underscores or hyphens, starting with a letter or digit."));
    }
    Ok(())
}
pub fn validate_remote(remote: &str, origin: &str) -> Result<()> {
    #[cfg(test)]
    if origin == "bare-test" && Path::new(remote).is_absolute() {
        return Ok(());
    }
    let admin = Url::parse(origin)
        .map_err(|_| invalid("Ask your administrator to configure LLM_WIKI_GITEA_ORIGIN."))?;
    let u = Url::parse(remote)
        .map_err(|_| invalid("Enter the complete SSH or HTTPS repository URL."))?;
    if !matches!(u.scheme(), "ssh" | "https")
        || u.host_str().is_none()
        || u.scheme() != admin.scheme()
        || u.host() != admin.host()
        || u.port_or_known_default() != admin.port_or_known_default()
        || u.password().is_some()
        || (u.scheme() == "https" && !u.username().is_empty())
        || !u
            .username()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || u.query().is_some()
        || u.fragment().is_some()
        || remote.contains(['%', '\\'])
        || remote.chars().any(|c| c.is_control() || c.is_whitespace())
        || u.path().trim_matches('/').split('/').count() < 2
        || !u
            .path()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
    {
        return Err(invalid(
            "Use a credential-free repository URL on the administrator's configured Gitea origin.",
        ));
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<Output> {
    let mut cmd = Command::new("git");
    // Do not inherit repository selection, identity, tracing or alternate
    // transports from a shell that happened to launch the desktop app.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    const SSH: &str = "ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 -o ServerAliveInterval=10 -o ServerAliveCountMax=2";
    cmd.current_dir(dir)
        .stdin(Stdio::null())
        .env_remove("SSH_ASKPASS")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env(
            "GIT_ALLOW_PROTOCOL",
            if cfg!(test) {
                "ssh:https:file"
            } else {
                "ssh:https"
            },
        )
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap_or(dir))
        .env("GIT_SSH_COMMAND", SSH);
    for setting in [
        "core.askPass=",
        "http.followRedirects=false",
        "http.proxy=",
        "http.sslVerify=true",
        "http.lowSpeedLimit=1",
        "http.lowSpeedTime=20",
        "credential.interactive=false",
        "core.hooksPath=/dev/null",
        "commit.gpgSign=false",
        "merge.gpgSign=false",
        "push.followTags=false",
        "submodule.recurse=false",
        "core.fsmonitor=false",
        "merge.autostash=false",
        "gc.auto=0",
    ] {
        cmd.args(["-c", setting]);
    }
    cmd.arg("--literal-pathspecs").args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.output()
        .map_err(|_| invalid("Could not start system Git. Check the Git installation."))
}
fn checked(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = git(dir, args)?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    let message = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
    if [
        "authentication",
        "permission denied",
        "could not read username",
        "host key verification",
        "publickey",
        "403",
        "401",
        "terminal prompts disabled",
        "certificate problem",
    ]
    .iter()
    .any(|s| message.contains(s))
    {
        return Err(fault(
            "Authentication Error",
            "Ask your administrator to check the shared credentials and server trust.",
        ));
    }
    if [
        "could not resolve",
        "couldn't connect",
        "could not connect",
        "connection",
        "timed out",
        "unreachable",
        "error: 500",
        "error: 502",
        "error: 503",
        "error: 504",
        "does not appear to be a git repository",
    ]
    .iter()
    .any(|s| message.contains(s))
    {
        return Err(fault(
            "Offline",
            "Gitea is unavailable. Local files and commits are preserved.",
        ));
    }
    if message.contains("non-fast-forward") || message.contains("fetch first") {
        return Err(fault(
            "Retry",
            "The remote branch advanced. Fetch again before pushing.",
        ));
    }
    Err(invalid(&format!(
        "Git {} failed (exit {}). Check the repository and branch.",
        args[0],
        out.status.code().unwrap_or(-1)
    )))
}
fn string(data: Vec<u8>) -> Result<String> {
    String::from_utf8(data).map_err(|_| invalid("Wiki Sync requires UTF-8 filenames."))
}
fn line(dir: &Path, args: &[&str]) -> Result<String> {
    Ok(string(checked(dir, args)?)?
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}
fn records(dir: &Path, args: &[&str]) -> Result<Vec<String>> {
    Ok(string(checked(dir, args)?)?
        .split_terminator('\0')
        .map(str::to_owned)
        .collect())
}
fn config(dir: &Path, key: &str) -> Result<Option<String>> {
    let out = git(dir, &["config", "--local", "--get", key])?;
    match out.status.code() {
        Some(0) => Ok(Some(string(out.stdout)?.trim_end().to_owned())),
        Some(1) => Ok(None),
        _ => Err(invalid("Could not read the local Git configuration.")),
    }
}
fn set(dir: &Path, key: &str, value: &str) -> Result<()> {
    checked(dir, &["config", "--local", key, value])?;
    Ok(())
}
fn identity(dir: &Path, id: &str) -> Result<()> {
    validate_id(id)?;
    set(dir, "user.name", id)?;
    set(dir, "user.email", &format!("{id}@local.invalid"))
}
fn has_head(dir: &Path) -> Result<bool> {
    Ok(git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success())
}
fn tree(dir: &Path, rev: &str) -> Result<Vec<String>> {
    records(dir, &["ls-tree", "-rz", rev])?
        .into_iter()
        .map(|entry| {
            let (meta, path) = entry
                .split_once('\t')
                .ok_or_else(|| invalid("Invalid Git tree entry."))?;
            if !(meta.starts_with("100644 blob ") || meta.starts_with("100755 blob "))
                || !allowed(path)
            {
                return Err(invalid(
                    "The repository contains files outside the regular Markdown allowlist.",
                ));
            }
            Ok(path.to_owned())
        })
        .collect()
}
pub fn wiki(root: &Path) -> Result<PathBuf> {
    let dir = root.join("wiki");
    if !root.join("schema.md").is_file() || !fs::symlink_metadata(&dir)?.is_dir() {
        return Err(invalid("Select a project with a local wiki directory."));
    }
    Ok(dir)
}
pub fn settings(dir: &Path, origin: &str) -> Result<Option<Settings>> {
    let meta = dir.join(".git");
    if !meta.try_exists()? {
        return Ok(None);
    }
    if !fs::symlink_metadata(&meta)?.is_dir() {
        return Err(invalid("Linked Git worktrees are unsupported."));
    }
    let Some(remote) = config(dir, "llmwiki.remote")? else {
        return Ok(None);
    };
    validate_remote(&remote, origin)?;
    if Path::new(&line(dir, &["rev-parse", "--show-toplevel"])?).canonicalize()?
        != dir.canonicalize()?
    {
        return Err(invalid(
            "The repository worktree must be the project wiki directory.",
        ));
    }
    for args in [
        vec!["remote", "get-url", "--all", "origin"],
        vec!["remote", "get-url", "--push", "--all", "origin"],
    ] {
        if line(dir, &args)? != remote {
            return Err(invalid(
                "The configured Git remote changed. Check the repository settings.",
            ));
        }
    }
    let branch = line(dir, &["symbolic-ref", "--short", "HEAD"])?;
    if config(dir, "llmwiki.branch")?.as_deref() != Some(&branch) {
        return Err(invalid("Switch back to the configured shared branch."));
    }
    Ok(Some(Settings {
        remote,
        branch,
        enabled: config(dir, "llmwiki.enabled")?.as_deref() == Some("true"),
        last_sync: config(dir, "llmwiki.lastSync")?.and_then(|s| s.parse().ok()),
        backup: config(dir, "llmwiki.backup")?,
    }))
}
const JOURNAL: &str = ".git/llm-wiki-changes.json";
pub fn changes(dir: &Path) -> Result<Changes> {
    let path = dir.join(JOURNAL);
    if !path.try_exists()? {
        return Ok(Changes::default());
    }
    let batch: Changes = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| invalid("Unreadable Wiki Sync change journal."))?;
    if batch.paths.iter().any(|p| !allowed(p)) {
        return Err(invalid("Invalid change journal path."));
    }
    Ok(batch)
}
fn remember(dir: &Path, paths: impl IntoIterator<Item = String>) -> Result<()> {
    let mut batch = changes(dir)?;
    batch.paths.extend(paths);
    write_changes(dir, batch)
}
fn write_changes(dir: &Path, mut batch: Changes) -> Result<()> {
    if batch.paths.is_empty() {
        if dir.join(JOURNAL).try_exists()? {
            fs::remove_file(dir.join(JOURNAL))?;
        }
        return Ok(());
    }
    batch.token = Uuid::new_v4().to_string();
    let tmp = dir
        .join(".git")
        .join(format!("llmwiki-{}.tmp", batch.token));
    fs::write(
        &tmp,
        serde_json::to_vec(&batch).map_err(|_| invalid("Could not encode change journal."))?,
    )?;
    fs::rename(tmp, dir.join(JOURNAL))?;
    Ok(())
}
pub fn acknowledge(dir: &Path, token: &str) -> Result<()> {
    if !token.is_empty() && changes(dir)?.token == token {
        fs::remove_file(dir.join(JOURNAL))?;
    }
    Ok(())
}
fn local_pages(dir: &Path) -> Result<Vec<String>> {
    let mut paths = vec![];
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
    {
        let e = entry.map_err(|_| invalid("Could not enumerate the local Wiki for backup."))?;
        let p = e
            .path()
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if e.file_type().is_file() && allowed(&p) {
            paths.push(p);
        }
    }
    Ok(paths)
}
// A process can exit between moving the old Wiki aside and installing the
// prepared clone. The locked record identifies the one directory to restore.
const SWAP_RECORD: &str = ".llm-wiki/wiki-sync-swap";
pub fn recover_clone(root: &Path) -> Result<()> {
    let wiki = root.join("wiki");
    let record = root.join(SWAP_RECORD);
    if wiki.try_exists()? || !record.try_exists()? {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&record)?;
    fs4::FileExt::lock_exclusive(&file)?;
    if wiki.try_exists()? {
        return Ok(());
    }
    let mut id = String::new();
    file.read_to_string(&mut id)?;
    let id =
        Uuid::parse_str(&id).map_err(|_| invalid("Check the project Wiki backup directory."))?;
    let backup = root.join(".llm-wiki").join(format!("wiki-backup-{id}"));
    if !fs::symlink_metadata(&backup)?.is_dir() {
        return Err(invalid("Invalid Wiki backup directory."));
    }
    fs::rename(backup, wiki)?;
    file.set_len(0)?;
    Ok(())
}

pub fn connect(
    dir: &Path,
    origin: &str,
    remote: &str,
    id: &str,
    clone: bool,
    backup: bool,
    mark: &impl Fn(&Path),
) -> Result<()> {
    validate_id(id)?;
    validate_remote(remote, origin)?;
    let nonempty = dir.read_dir()?.next().transpose()?.is_some();
    if clone && nonempty && !backup {
        return Err(fault(
            "Backup Required",
            "The local Wiki contains files. Back it up before cloning.",
        ));
    }
    if !clone && dir.join(".git").try_exists()? {
        if let Some(s) = settings(dir, origin)? {
            if s.remote == remote {
                identity(dir, id)?;
                set(dir, "llmwiki.disconnected", "false")?;
                return set(dir, "llmwiki.enabled", "true");
            }
        }
        return Err(invalid(
            "Existing Git history is preserved. Use Back up and clone to connect this Wiki.",
        ));
    }
    let state_dir = dir.parent().unwrap().join(".llm-wiki");
    fs::create_dir_all(&state_dir)?;
    if !fs::symlink_metadata(&state_dir)?.is_dir() {
        return Err(invalid("Use a local project settings directory."));
    }
    let tmp = state_dir.join(format!("wiki-clone-{}", Uuid::new_v4()));
    fs::create_dir(&tmp)?;
    let result = (|| {
        if line(&tmp, &["ls-remote", "--get-url", remote])? != remote {
            return Err(invalid("Git URL rewriting is disabled for Wiki Sync."));
        }
        let refs = line(&tmp, &["ls-remote", "--symref", "--", remote])?;
        if !clone && !refs.is_empty() {
            return Err(invalid("The remote already has history. Back up and clone it, or clone into a new project."));
        }
        checked(
            &tmp,
            &[
                "clone",
                "--no-checkout",
                "--no-tags",
                "--single-branch",
                "--no-local",
                "--",
                remote,
                ".",
            ],
        )?;
        let branch = line(&tmp, &["symbolic-ref", "--short", "HEAD"])?;
        let populated = has_head(&tmp)?;
        if !populated && !refs.is_empty() {
            return Err(invalid(
                "Ask the administrator to set a valid default branch.",
            ));
        }
        if populated && !clone {
            return Err(invalid(
                "The remote changed while connecting. Clone it instead.",
            ));
        }
        identity(&tmp, id)?;
        set(&tmp, "llmwiki.branch", &branch)?;
        set(&tmp, "llmwiki.remote", remote)?;
        set(&tmp, "llmwiki.enabled", "true")?;
        set(
            &tmp,
            "llmwiki.initial",
            if populated { "false" } else { "true" },
        )?;
        if !clone {
            fs::rename(tmp.join(".git"), dir.join(".git"))?;
            return Ok(());
        }
        let nonempty = dir.read_dir()?.next().transpose()?.is_some();
        if nonempty && !backup {
            return Err(fault(
                "Backup Required",
                "Local files appeared while cloning. Back up before continuing.",
            ));
        }
        let mut paths = local_pages(dir)?;
        if populated {
            paths.extend(tree(&tmp, "HEAD")?);
            checked(&tmp, &["read-tree", "-mu", "HEAD"])?;
        }
        remember(&tmp, paths)?;
        let id = Uuid::new_v4();
        let mut swap = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(dir.parent().unwrap().join(SWAP_RECORD))?;
        fs4::FileExt::lock_exclusive(&swap)?;
        swap.set_len(0)?;
        swap.write_all(id.to_string().as_bytes())?;
        swap.sync_all()?;
        let old = state_dir.join(format!("wiki-backup-{id}"));
        if nonempty {
            set(&tmp, "llmwiki.backup", &old.to_string_lossy())?;
        }
        mark(dir);
        fs::rename(dir, &old)?;
        if let Err(e) = fs::rename(&tmp, dir) {
            fs::rename(&old, dir).map_err(|_| {
                invalid(&format!(
                    "Restore the preserved Wiki from {}",
                    old.display()
                ))
            })?;
            return Err(e.into());
        }
        swap.set_len(0)?;
        if !nonempty {
            let _ = fs::remove_dir(old);
        }
        Ok(())
    })();
    // Only this unique temporary clone is disposable. Backups are never pruned.
    if tmp.exists() {
        let _ = fs::remove_dir_all(tmp);
    }
    result
}
fn unresolved(dir: &Path) -> Result<Vec<String>> {
    records(dir, &["diff", "--name-only", "--diff-filter=U", "-z"])
}
pub fn conflict(dir: &Path) -> Result<Option<Fault>> {
    let files = unresolved(dir)?;
    Ok(
        (!files.is_empty() || dir.join(".git/MERGE_HEAD").exists()).then(|| Fault {
            kind: "Conflict",
            message:
                "Resolve the listed files in the Wiki folder, then retry to complete the merge."
                    .into(),
            files,
        }),
    )
}
fn index_is_allowed(dir: &Path) -> Result<()> {
    for entry in records(dir, &["ls-files", "--stage", "-z"])? {
        let (mode, path) = entry
            .split_once('\t')
            .ok_or_else(|| invalid("Invalid Git index."))?;
        if !(mode.starts_with("100644 ") || mode.starts_with("100755 ")) || !allowed(path) {
            return Err(invalid(
                "The Git index contains files outside the Wiki Markdown allowlist.",
            ));
        }
    }
    Ok(())
}
fn stage(dir: &Path, paths: &[String]) -> Result<()> {
    let mut safe = vec![];
    for path in paths {
        if !allowed(path) {
            continue;
        }
        let file = dir.join(path);
        if file.try_exists()? {
            if !fs::symlink_metadata(&file)?.is_file()
                || file.canonicalize()? != dir.canonicalize()?.join(path)
            {
                return Err(invalid("Wiki Sync cannot stage linked Markdown files."));
            }
        }
        safe.push(path.as_str());
    }
    // Bound argv size on Windows, including for very long paths.
    for path in safe {
        checked(dir, &["add", "-A", "--", path])?;
    }
    index_is_allowed(dir)
}
fn local_changes(dir: &Path) -> Result<Vec<String>> {
    Ok(records(
        dir,
        &[
            "ls-files",
            "--modified",
            "--deleted",
            "--others",
            "--exclude-standard",
            "--deduplicate",
            "-z",
        ],
    )?
    .into_iter()
    .filter(|p| allowed(p))
    .collect())
}
fn commit(dir: &Path, message: &str) -> Result<()> {
    let diff = git(dir, &["diff", "--cached", "--quiet"])?;
    match diff.status.code() {
        Some(0) if !dir.join(".git/MERGE_HEAD").exists() => Ok(()),
        Some(0 | 1) => {
            checked(dir, &["commit", "-m", message])?;
            Ok(())
        }
        _ => Err(invalid("Could not inspect staged changes.")),
    }
}
fn resolve(dir: &Path, files: &[String]) -> Result<()> {
    for path in files {
        if !allowed(path) {
            return Err(invalid("Resolve the unsupported conflict in Git first."));
        }
        let file = dir.join(path);
        if file.try_exists()? {
            if !fs::symlink_metadata(&file)?.is_file() {
                return Err(invalid("Resolve linked files in Git first."));
            }
            if fs::read(&file)?.split(|b| *b == b'\n').any(|l| {
                [b"<<<<<<<", b"=======", b">>>>>>>"]
                    .iter()
                    .any(|m| l.starts_with(*m))
            }) {
                return Err(conflict(dir)?.unwrap());
            }
        }
    }
    stage(dir, files)?;
    commit(dir, "Resolve shared Wiki merge")
}
pub fn sync(
    dir: &Path,
    origin: &str,
    id: &str,
    retry_resolution: bool,
    mark: &impl Fn(&Path),
) -> Result<()> {
    let s = settings(dir, origin)?.ok_or_else(|| invalid("Connect this Wiki first."))?;
    identity(dir, id)?;
    if let Some(c) = conflict(dir)? {
        if !retry_resolution {
            return Err(c);
        }
        resolve(dir, &c.files)?;
    }
    for state in [
        "rebase-apply",
        "rebase-merge",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_START",
    ] {
        if dir.join(".git").join(state).exists() {
            return Err(invalid("Finish the current Git operation before syncing."));
        }
    }
    index_is_allowed(dir)?;
    stage(dir, &local_changes(dir)?)?;
    commit(dir, "Update shared Wiki")?;
    let branch_ref = format!("refs/heads/{}", s.branch);
    let refs = line(dir, &["ls-remote", "--heads", "origin", &branch_ref])?;
    let have_head = has_head(dir)?;
    let before = if have_head {
        Some(line(dir, &["rev-parse", "HEAD"])?)
    } else {
        None
    };
    let mut upstream = None;
    if let Some((remote_head, _)) = refs.split_once('\t') {
        if before.as_deref() == Some(remote_head) {
            upstream = before.clone();
        } else {
            checked(
                dir,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "origin",
                    &branch_ref,
                ],
            )?;
            let fetched = line(dir, &["rev-parse", "FETCH_HEAD"])?;
            let incoming = tree(dir, &fetched)?;
            if !have_head
                || !git(dir, &["merge-base", "--is-ancestor", &fetched, "HEAD"])?
                    .status
                    .success()
            {
                if !local_changes(dir)?.is_empty()
                    || !git(dir, &["diff", "--cached", "--quiet"])?.status.success()
                {
                    return Err(fault(
                        "Retry",
                        "Wiki changed during fetch. The next sync will include the edits.",
                    ));
                }
                let pending = changes(dir)?;
                let diff = |old: &str, new: &str| {
                    records(
                        dir,
                        &["diff", "--name-only", "--no-renames", "-z", old, new],
                    )
                };
                let predicted = match &before {
                    Some(old) => diff(old, &fetched)?,
                    None => incoming,
                };
                // Preserve a conservative change list if the merge is interrupted.
                remember(dir, predicted.clone())?;
                for p in predicted {
                    mark(&dir.join(p));
                }
                if checked(
                    dir,
                    &["merge", "--no-edit", "--no-overwrite-ignore", &fetched],
                )
                .is_err()
                {
                    if let Some(c) = conflict(dir)? {
                        return Err(c);
                    }
                    return Err(invalid(
                        "Git could not merge safely. Check for changes in the working directory.",
                    ));
                }
                if let Some(old) = &before {
                    let actual = diff(old, "HEAD")?;
                    let mut batch = pending;
                    batch.paths.extend(actual);
                    write_changes(dir, batch)?;
                }
            }
            upstream = Some(fetched);
        }
    } else if !refs.is_empty()
        || config(dir, "llmwiki.initial")?.as_deref() != Some("true")
        || !line(dir, &["ls-remote", "origin"])?.is_empty()
    {
        return Err(invalid(
            "The shared branch is missing. Check the default branch on Gitea.",
        ));
    }
    if has_head(dir)? {
        let head = line(dir, &["rev-parse", "HEAD"])?;
        if upstream.as_deref() != Some(&head) {
            // An external Git client may have committed an excluded file
            // and removed it later. Validate every newly published tree.
            let range = upstream
                .map(|rev| format!("{rev}..{head}"))
                .unwrap_or(head.clone());
            for rev in line(dir, &["rev-list", &range])?.lines() {
                tree(dir, rev)?;
            }
            checked(
                dir,
                &[
                    "push",
                    "--no-follow-tags",
                    "--recurse-submodules=no",
                    "origin",
                    &format!("{head}:{branch_ref}"),
                ],
            )?;
        }
        set(dir, "llmwiki.initial", "false")?;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    set(dir, "llmwiki.lastSync", &now.to_string())
}
pub fn enable(dir: &Path, origin: &str, enabled: bool) -> Result<()> {
    settings(dir, origin)?.ok_or_else(|| invalid("Connect this Wiki first."))?;
    set(
        dir,
        "llmwiki.enabled",
        if enabled { "true" } else { "false" },
    )
}
pub fn disconnect(dir: &Path) -> Result<()> {
    // Keep the repository, refs and backup intact; reconnect via backup/clone
    // or explicitly re-enable this managed repository with the same URL.
    set(dir, "llmwiki.enabled", "false")?;
    set(dir, "llmwiki.disconnected", "true")
}
pub fn is_disconnected(dir: &Path) -> Result<bool> {
    if !dir.join(".git").is_dir() {
        return Ok(false);
    }
    Ok(config(dir, "llmwiki.disconnected")?.as_deref() == Some("true"))
}

#[cfg(test)]
mod tests;
