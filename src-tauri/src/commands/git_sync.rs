use super::file_sync::{is_content_change_event, mark_app_write_path, record_git_changes};
use llm_wiki_sync::{self as git, Changes, Fault, Settings};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, Weak},
};
use tauri::{ipc::Channel, State};

static LOCKS: LazyLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = LazyLock::new(Default::default);
type Watches = Mutex<HashMap<String, notify::RecommendedWatcher>>;
#[derive(Default)]
pub struct GitWatches(Watches);
fn error(msg: &str) -> Fault {
    Fault {
        kind: "Error",
        message: msg.into(),
        files: vec![],
    }
}
fn origin() -> String {
    std::env::var("LLM_WIKI_GITEA_ORIGIN").unwrap_or_default()
}

async fn run<T: Send + 'static>(
    project_path: String,
    f: impl FnOnce(&Path) -> git::Result<T> + Send + 'static,
) -> git::Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = Path::new(&project_path).canonicalize()?;
        let lock = {
            let mut locks = LOCKS
                .lock()
                .map_err(|_| error("Wiki Sync lock unavailable."))?;
            locks.retain(|_, v| v.strong_count() > 0);
            match locks.get(&root).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(Mutex::new(()));
                    locks.insert(root.clone(), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        let _guard = lock
            .lock()
            .map_err(|_| error("Wiki Sync lock unavailable."))?;
        f(&git::wiki(&root)?)
    })
    .await
    .map_err(|_| error("The Wiki Sync worker stopped."))?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    config: Option<Settings>,
    conflict: Option<Fault>,
    changes: Changes,
    server: String,
    default_remote: String,
}
#[tauri::command]
pub async fn git_sync_status(project_path: String) -> git::Result<Status> {
    run(project_path, |dir| {
        let server = origin();
        let config = if git::is_disconnected(dir)? {
            None
        } else {
            git::settings(dir, &server)?
        };
        let mut changes = Changes::default();
        let mut conflict = None;
        if config.is_some() {
            conflict = git::conflict(dir)?;
            changes = git::changes(dir)?;
            if !changes.paths.is_empty() {
                record_git_changes(dir, &changes.paths).map_err(|msg| error(&msg))?;
            }
        }
        let default_remote = std::env::var("LLM_WIKI_GITEA_REPOSITORY")
            .ok()
            .filter(|url| git::validate_remote(url, &server).is_ok())
            .unwrap_or_default();
        Ok(Status {
            config,
            conflict,
            changes,
            server,
            default_remote,
        })
    })
    .await
}
#[tauri::command]
pub async fn git_sync_connect(
    project_path: String,
    remote: String,
    user_id: String,
    clone: bool,
    backup: bool,
) -> git::Result<()> {
    run(project_path, move |dir| {
        git::connect(
            dir,
            &origin(),
            &remote,
            &user_id,
            clone,
            backup,
            &mark_app_write_path,
        )
    })
    .await
}
#[tauri::command]
pub async fn git_sync_now(project_path: String, user_id: String, resolve: bool) -> git::Result<()> {
    run(project_path, move |dir| {
        if git::is_disconnected(dir)? {
            return Err(error("Connect this Wiki first."));
        }
        git::sync(dir, &origin(), &user_id, resolve, &mark_app_write_path)
    })
    .await
}
#[tauri::command]
pub async fn git_sync_enable(project_path: String, enabled: bool) -> git::Result<()> {
    run(project_path, move |dir| {
        git::enable(dir, &origin(), enabled)
    })
    .await
}
#[tauri::command]
pub async fn git_sync_disconnect(project_path: String) -> git::Result<()> {
    run(project_path, git::disconnect).await
}
#[tauri::command]
pub async fn git_sync_acknowledge(project_path: String, token: String) -> git::Result<()> {
    run(project_path, move |dir| git::acknowledge(dir, &token)).await
}

#[tauri::command]
pub async fn git_sync_embed_page(
    project_path: String,
    path: String,
    mut cfg: super::search::SearchEmbeddingConfig,
) -> git::Result<()> {
    if !git::allowed(&path) {
        return Err(error("Invalid shared Wiki page."));
    }
    cfg.internal_sync = true;
    super::page_embedding::embed_wiki_page(&project_path, &format!("wiki/{path}"), cfg, false)
        .await
        .map(|_| ())
        .map_err(|_| error("Could not index the page with the configured internal model."))
}

// The source watcher emits Wiki change events before suppressing app writes.
// When source monitoring is off, subscribe only to the connected Wiki.
#[tauri::command]
pub fn git_sync_watch(
    project_path: String,
    id: String,
    changed: Channel<()>,
    state: State<GitWatches>,
) -> git::Result<()> {
    let dir = git::wiki(Path::new(&project_path))?;
    let base = dir.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(e) = result {
            if is_content_change_event(&e.kind) && e.paths.iter().any(|p| is_wiki_page(&base, p)) {
                let _ = changed.send(());
            }
        }
    })
    .map_err(|_| error("Could not start the Wiki watcher."))?;
    watcher
        .watch(&dir, RecursiveMode::Recursive)
        .map_err(|_| error("Could not watch the Wiki directory."))?;
    state
        .0
        .lock()
        .map_err(|_| error("Wiki watcher unavailable."))?
        .insert(id, watcher);
    Ok(())
}
pub(super) fn is_wiki_page(wiki: &Path, path: &Path) -> bool {
    path.strip_prefix(wiki)
        .ok()
        .and_then(Path::to_str)
        .is_some_and(|p| git::allowed(&p.replace(std::path::MAIN_SEPARATOR, "/")))
}
#[tauri::command]
pub fn git_sync_unwatch(id: String, state: State<GitWatches>) -> git::Result<()> {
    state
        .0
        .lock()
        .map_err(|_| error("Wiki watcher unavailable."))?
        .remove(&id);
    Ok(())
}
