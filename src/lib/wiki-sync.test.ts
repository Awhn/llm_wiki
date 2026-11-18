import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { useWikiStore } from "@/stores/wiki-store"
import { loadWikiUserId, saveWikiUserId } from "./project-store"
import { refreshGitPages, useWikiSync, WikiSyncSession } from "./wiki-sync"
import { withProjectLock } from "./project-mutex"

const mock = vi.hoisted(() => ({
  invoke: vi.fn(), off: vi.fn(), listen: vi.fn(), tree: vi.fn(), remove: vi.fn(),
  exists: vi.fn(), read: vi.fn(), store: new Map(), save: vi.fn(),
}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: mock.invoke, Channel: class {} }))
vi.mock("@tauri-apps/api/event", () => ({ listen: mock.listen }))
vi.mock("@tauri-apps/plugin-store", () => ({ load: async () => ({
  get: (key: string) => mock.store.get(key), set: (key: string, value: string) => mock.store.set(key, value), save: mock.save,
}) }))
vi.mock("@/commands/fs", () => ({ fileExists: mock.exists, readFile: mock.read }))
vi.mock("./embedding", () => ({ removeSyncedPageEmbedding: mock.remove, wikiPageIdFromPath: (_: string, p: string) => p.split("/wiki/")[1].slice(0, -3) }))
vi.mock("./project-file-tree-refresh", () => ({ refreshProjectFileTree: mock.tree }))

const project = { id: "A", path: "/alice", name: "Alice" }
const status = {
  config: { enabled: true, remote: "ssh://git@gitea/team/wiki", branch: "team", lastSync: 10 },
  changes: { token: "", paths: [] }, server: "ssh://gitea", conflict: null, defaultRemote: "",
}
let session: WikiSyncSession
const runs = () => mock.invoke.mock.calls.filter(([name]) => name === "git_sync_now").length
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  mock.store.clear()
  mock.listen.mockResolvedValue(mock.off)
  mock.invoke.mockImplementation(async (name: string) => name === "git_sync_status" ? status : undefined)
  useWikiStore.setState({ project, selectedFile: null })
  useWikiSync.setState({ userId: "alice", editing: null })
  session = new WikiSyncSession(project)
})
afterEach(() => { session.stop(); vi.useRealTimers() })

describe("Wiki Sync", () => {
  it("prompts for an absent ID and persists a valid identity across loads", async () => {
    expect(await loadWikiUserId()).toBeNull()
    await expect(saveWikiUserId("../alice")).rejects.toThrow()
    await saveWikiUserId("alice-1")
    expect(mock.save).toHaveBeenCalledOnce()
    expect(await loadWikiUserId()).toBe("alice-1")
  })
  it("coalesces writes for three seconds and polls idle projects every thirty seconds", async () => {
    await session.start()
    await vi.advanceTimersByTimeAsync(0)
    expect(runs()).toBe(1)
    session.dirty()
    await vi.advanceTimersByTimeAsync(2000)
    session.dirty()
    await vi.advanceTimersByTimeAsync(2999)
    expect(runs()).toBe(1)
    await vi.advanceTimersByTimeAsync(1)
    expect(runs()).toBe(2)
    await vi.advanceTimersByTimeAsync(25000)
    expect(runs()).toBe(3)
  })
  it("drops queued Git work and releases listeners on a project switch", async () => {
    let release!: () => void
    const writing = withProjectLock(project.path, () => new Promise<void>(r => { release = r }))
    await session.start()
    session.stop()
    useWikiStore.setState({ project: { ...project, id: "B", path: "/bob" } })
    release()
    await writing
    await vi.advanceTimersByTimeAsync(60000)
    expect(mock.off).toHaveBeenCalledOnce()
    expect(runs()).toBe(0)
  })
  it("holds Git while the editor has a draft", async () => {
    useWikiSync.setState({ editing: "/alice/wiki/local.md" })
    await session.start()
    await vi.advanceTimersByTimeAsync(30000)
    expect(runs()).toBe(0)
    useWikiSync.setState({ editing: null })
    session.dirty()
    await vi.advanceTimersByTimeAsync(3000)
    expect(runs()).toBe(1)
  })
  it("parks authentication failures until manual retry", async () => {
    mock.invoke.mockImplementation(async name => {
      if (name === "git_sync_status") return status
      if (name === "git_sync_now") throw { kind: "Authentication Error", message: "Admin setup required", files: [] }
    })
    await session.start()
    await vi.advanceTimersByTimeAsync(90000)
    expect(runs()).toBe(1)
    expect(useWikiSync.getState().phase).toBe("Authentication Error")
    await session.action("now")
    expect(runs()).toBe(2)
  })
  it("refreshes cloned and merged pages, updates the reader and removes only deleted vectors", async () => {
    useWikiStore.setState({ selectedFile: "/alice/wiki/notes/a.md", embeddingConfig: { ...useWikiStore.getState().embeddingConfig, enabled: true } })
    mock.exists.mockImplementation(async path => !path.endsWith("gone.md"))
    mock.read.mockResolvedValue("updated page")
    expect(await refreshGitPages(project, { token: "x", paths: ["notes/a.md", "sources/gone.md"] }, () => true)).toBe(true)
    expect(mock.tree).toHaveBeenCalledWith(project.path, expect.objectContaining({ bumpDataVersion: true }))
    expect(useWikiStore.getState().fileContent).toBe("updated page")
    expect(mock.invoke).toHaveBeenCalledWith("git_sync_embed_page", expect.objectContaining({ path: "notes/a.md" }))
    expect(mock.remove).toHaveBeenCalledWith(project.path, "sources/gone")
  })
  it("keeps a failed index refresh pending without modifying an active draft", async () => {
    const file = "/alice/wiki/note.md"
    useWikiStore.setState({ selectedFile: file, fileContent: "draft", embeddingConfig: { ...useWikiStore.getState().embeddingConfig, enabled: true } })
    useWikiSync.setState({ editing: file })
    mock.exists.mockResolvedValue(true)
    mock.invoke.mockRejectedValue(new Error("model offline"))
    expect(await refreshGitPages(project, { token: "x", paths: ["note.md"] }, () => true)).toBe(false)
    expect(useWikiStore.getState().fileContent).toBe("draft")
  })
})

describe("Wiki Sync recovery", () => {
  it("coalesces a push race into one retry within a sync operation", async () => {
    let attempts = 0
    mock.invoke.mockImplementation(async name => {
      if (name === "git_sync_status") return status
      if (name === "git_sync_now" && attempts++ === 0) throw { kind: "Retry", message: "Remote advanced", files: [] }
    })
    await session.action("now")
    expect(runs()).toBe(2)
    expect(useWikiSync.getState().phase).toBe("Synced")
  })
  it("retains the change journal when deleting vectors fails", async () => {
    mock.exists.mockResolvedValue(false)
    mock.remove.mockRejectedValueOnce(new Error("index locked"))
    expect(await refreshGitPages(project, { token: "delete", paths: ["gone.md"] }, () => true)).toBe(false)
  })
})
