use super::*;

struct Team {
    root: PathBuf,
    url: String,
    a: PathBuf,
    b: PathBuf,
}
impl Team {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("llm-wiki-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        checked(
            &root,
            &["init", "--bare", "--initial-branch=team", "remote.git"],
        )
        .unwrap();
        let a = root.join("a/wiki");
        let b = root.join("b/wiki");
        for dir in [&a, &b] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.parent().unwrap().join("schema.md"), "local schema").unwrap();
        }
        Self {
            url: root.join("remote.git").to_str().unwrap().to_owned(),
            root,
            a,
            b,
        }
    }
    fn attach(&self, dir: &Path, id: &str, clone: bool, backup: bool) -> Result<()> {
        connect(dir, "bare-test", &self.url, id, clone, backup, &|_| {})
    }
    fn publish(&self) {
        fs::write(self.a.join("overview.md"), "base\n").unwrap();
        self.attach(&self.a, "alice", false, false).unwrap();
        self.sync_a().unwrap();
    }
    fn sync_a(&self) -> Result<()> {
        sync(&self.a, "bare-test", "alice", false, &|_| {})
    }
    fn sync_b(&self) -> Result<()> {
        sync(&self.b, "bare-test", "bob", false, &|_| {})
    }
}
impl Drop for Team {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn validates_identity_and_pins_endpoint() {
    for s in ["a/b", "a b", ".bad", "", "-c"] {
        assert!(validate_id(s).is_err());
    }
    assert!(validate_id("ahn-1").is_ok());
    let origin = "https://gitea.internal:8443";
    for url in [
        "https://foreign/team/wiki",
        "https://gitea.internal/team/wiki",
        "https://user:token@gitea.internal:8443/team/wiki",
        "file:///tmp/team/wiki",
        "ssh://gitea.internal:8443/team/wiki",
    ] {
        assert!(validate_remote(url, origin).is_err());
    }
    assert!(validate_remote("https://gitea.internal:8443/team/wiki.git", origin).is_ok());
}
#[test]
fn clone_pull_merge_author_and_deletion() {
    let t = Team::new();
    t.publish();
    t.attach(&t.b, "bob", true, false).unwrap();
    assert_eq!(line(&t.b, &["branch", "--show-current"]).unwrap(), "team");
    assert_eq!(
        fs::read_to_string(t.b.join("overview.md")).unwrap(),
        "base\n"
    );
    let old = changes(&t.b).unwrap();
    assert!(old.paths.contains("overview.md"));
    acknowledge(&t.b, &old.token).unwrap();
    fs::write(t.a.join("A.md"), "from A").unwrap();
    fs::write(t.b.join("B.md"), "from B").unwrap();
    t.sync_a().unwrap();
    t.sync_b().unwrap();
    t.sync_a().unwrap();
    assert_eq!(
        line(&t.b, &["log", "-1", "--format=%an <%ae>", "--", "A.md"]).unwrap(),
        "alice <alice@local.invalid>"
    );
    assert_eq!(fs::read_to_string(t.a.join("B.md")).unwrap(), "from B");
    let raw = t.b.parent().unwrap().join("raw/sources/doc.md");
    fs::create_dir_all(raw.parent().unwrap()).unwrap();
    fs::write(&raw, "private").unwrap();
    fs::remove_file(t.a.join("A.md")).unwrap();
    t.sync_a().unwrap();
    t.sync_b().unwrap();
    assert!(!t.b.join("A.md").exists() && raw.exists());
    assert!(changes(&t.b).unwrap().paths.contains("A.md"));
}
#[test]
fn conflict_keeps_both_histories_and_requires_resolution() {
    let t = Team::new();
    t.publish();
    t.attach(&t.b, "bob", true, false).unwrap();
    fs::write(t.a.join("overview.md"), "alice\n").unwrap();
    fs::write(t.b.join("overview.md"), "bob\n").unwrap();
    t.sync_a().unwrap();
    let f = t.sync_b().unwrap_err();
    assert_eq!(f.kind, "Conflict");
    assert_eq!(f.files, ["overview.md"]);
    assert_eq!(line(&t.b, &["show", "HEAD:overview.md"]).unwrap(), "bob");
    assert_eq!(
        line(&t.b, &["show", "MERGE_HEAD:overview.md"]).unwrap(),
        "alice"
    );
    assert_eq!(
        sync(&t.b, "bare-test", "bob", true, &|_| {})
            .unwrap_err()
            .kind,
        "Conflict"
    );
    fs::write(t.b.join("overview.md"), "both\n").unwrap();
    sync(&t.b, "bare-test", "bob", true, &|_| {}).unwrap();
    t.sync_a().unwrap();
    assert_eq!(line(&t.a, &["show", "HEAD:overview.md"]).unwrap(), "both");
}
#[test]
fn offline_keeps_commit_and_recovers() {
    let t = Team::new();
    t.publish();
    fs::rename(&t.url, t.root.join("down")).unwrap();
    fs::write(t.a.join("new.md"), "offline").unwrap();
    assert_eq!(t.sync_a().unwrap_err().kind, "Offline");
    assert_eq!(line(&t.a, &["show", "HEAD:new.md"]).unwrap(), "offline");
    fs::rename(t.root.join("down"), &t.url).unwrap();
    t.sync_a().unwrap();
    assert_eq!(
        line(Path::new(&t.url), &["show", "team:new.md"]).unwrap(),
        "offline"
    );
}
#[test]
fn clone_and_connect_preserve_existing_data() {
    let t = Team::new();
    t.publish();
    fs::write(t.b.join("private.md"), "keep").unwrap();
    assert_eq!(
        t.attach(&t.b, "bob", true, false).unwrap_err().kind,
        "Backup Required"
    );
    assert!(t.attach(&t.b, "bob", false, false).is_err() && !t.b.join(".git").exists());
    fs::rename(&t.url, t.root.join("down")).unwrap();
    assert!(t.attach(&t.b, "bob", true, true).is_err());
    assert!(t.b.join("private.md").is_file());
    fs::rename(t.root.join("down"), &t.url).unwrap();
    t.attach(&t.b, "bob", true, true).unwrap();
    let backup = settings(&t.b, "bare-test")
        .unwrap()
        .unwrap()
        .backup
        .unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&backup).join("private.md")).unwrap(),
        "keep"
    );
    assert!(changes(&t.b).unwrap().paths.contains("private.md"));
}
#[test]
fn excludes_local_data_and_rejects_unsafe_history() {
    let t = Team::new();
    t.publish();
    for path in [
        "raw/sources/a.md",
        ".llm-wiki/key.md",
        "wiki/.cache/db.md",
        "wiki/media/img.png",
        "wiki/key.json",
    ] {
        let p = t.a.parent().unwrap().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "secret").unwrap();
    }
    fs::write(t.a.join("notes [1].md"), "safe").unwrap();
    t.sync_a().unwrap();
    assert_eq!(tree(&t.a, "HEAD").unwrap(), ["notes [1].md", "overview.md"]);
    checked(&t.a, &["add", "--", "key.json"]).unwrap();
    assert!(t.sync_a().is_err());
    checked(&t.a, &["commit", "-m", "private"]).unwrap();
    checked(&t.a, &["rm", "key.json"]).unwrap();
    checked(&t.a, &["commit", "-m", "remove"]).unwrap();
    assert!(t.sync_a().is_err());
    assert_eq!(
        line(Path::new(&t.url), &["rev-list", "--count", "team"]).unwrap(),
        "2"
    );
}

#[test]
fn interrupted_directory_swap_recovers_on_open() {
    let team = Team::new();
    fs::write(team.b.join("private.md"), "keep").unwrap();
    let root = team.b.parent().unwrap();
    let id = Uuid::new_v4();
    fs::create_dir(root.join(".llm-wiki")).unwrap();
    fs::write(root.join(SWAP_RECORD), id.to_string()).unwrap();
    fs::rename(
        &team.b,
        root.join(".llm-wiki").join(format!("wiki-backup-{id}")),
    )
    .unwrap();
    recover_clone(root).unwrap();
    assert_eq!(
        fs::read_to_string(team.b.join("private.md")).unwrap(),
        "keep"
    );
}

#[test]
fn refuses_remote_non_markdown_before_checkout() {
    let t = Team::new();
    t.publish();
    fs::write(t.a.join("index.db"), "private").unwrap();
    checked(&t.a, &["add", "--", "index.db"]).unwrap();
    checked(&t.a, &["commit", "-m", "unmanaged client"]).unwrap();
    checked(&t.a, &["push", "origin", "HEAD:refs/heads/team"]).unwrap();
    fs::write(t.b.join("keep.md"), "keep").unwrap();
    assert!(t.attach(&t.b, "bob", true, true).is_err());
    assert_eq!(fs::read_to_string(t.b.join("keep.md")).unwrap(), "keep");
    assert!(!t.b.join(".git").exists());
}

#[test]
fn locally_redirected_push_url_is_rejected_before_network_use() {
    let team = Team::new();
    team.publish();
    set(
        &team.a,
        "remote.origin.pushurl",
        "https://example.invalid/team/wiki",
    )
    .unwrap();
    assert_eq!(team.sync_a().unwrap_err().kind, "Error");
}

#[test]
fn explicit_reconnection_preserves_history_and_resumes_paused_repo() {
    let team = Team::new();
    team.publish();
    let before = line(&team.a, &["rev-parse", "HEAD"]).unwrap();
    enable(&team.a, "bare-test", false).unwrap();
    assert!(!settings(&team.a, "bare-test").unwrap().unwrap().enabled);
    disconnect(&team.a).unwrap();
    assert!(is_disconnected(&team.a).unwrap());
    team.attach(&team.a, "bob", false, false).unwrap();
    assert!(!is_disconnected(&team.a).unwrap());
    assert!(settings(&team.a, "bare-test").unwrap().unwrap().enabled);
    assert_eq!(line(&team.a, &["rev-parse", "HEAD"]).unwrap(), before);
}

#[test]
fn rejects_a_markdown_symlink_before_checkout() {
    let team = Team::new();
    team.publish();
    let blob = line(&team.a, &["rev-parse", "HEAD:overview.md"]).unwrap();
    checked(
        &team.a,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},linked.md"),
        ],
    )
    .unwrap();
    checked(&team.a, &["commit", "-m", "link from another client"]).unwrap();
    checked(&team.a, &["push", "origin", "HEAD:refs/heads/team"]).unwrap();
    assert!(team.attach(&team.b, "bob", true, false).is_err());
    assert!(!team.b.join(".git").exists());
}
