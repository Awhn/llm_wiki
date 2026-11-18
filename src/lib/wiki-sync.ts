import { Channel, invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { create } from "zustand"
import { fileExists, readFile } from "@/commands/fs"
import { useWikiStore } from "@/stores/wiki-store"
import { normalizePath } from "./path-utils"
import { refreshProjectFileTree } from "./project-file-tree-refresh"
import { withProjectLock } from "./project-mutex"
import type { WikiProject } from "@/types/wiki"

type Phase = "Not Configured" | "Connecting" | "Connected" | "Synced" | "Syncing" | "Offline" | "Authentication Error" | "Conflict" | "Error" | "Backup Required" | "Retry"
interface Fault { kind: Phase; message: string; files: string[] }
interface Config { remote: string; branch: string; enabled: boolean; lastSync: number | null; backup: string | null }
interface Changes { token: string; paths: string[] }
interface Status { server: string; defaultRemote: string; config: Config | null; conflict: Fault | null; changes: Changes }

const initial = {
  phase: "Not Configured" as Phase, detail: "", files: [] as string[], server: "", defaultRemote: "",
  config: null as Config | null, busy: false, ready: false, indexMessage: "",
}
export const useWikiSync = create(() => ({
  ...initial, userId: "", editing: null as string | null, openSettings: false,
}))

function asFault(error: unknown): Fault {
  if (error && typeof error === "object" && "kind" in error && "message" in error) return error as Fault
  return { kind: "Error", message: String(error), files: [] }
}

export async function refreshGitPages(project: WikiProject, changes: Changes, current: () => boolean): Promise<boolean> {
  const root = normalizePath(project.path)
  const { removeSyncedPageEmbedding, wikiPageIdFromPath } = await import("./embedding")
  if (!current()) return false
  await refreshProjectFileTree(root, { projectId: project.id, bumpDataVersion: true, clearDisplayTreeFirst: true })
  let complete = true
  for (const path of changes.paths) {
    if (!current()) return false
    const file = root + "/wiki/" + path
    const pageId = wikiPageIdFromPath(root, file)
    const exists = await fileExists(file)
    const store = useWikiStore.getState()
    const selected = () => current() && useWikiStore.getState().selectedFile === file && useWikiSync.getState().editing !== file
    const embed = store.embeddingConfig
    if (exists) {
      if (selected()) {
        const body = await readFile(file)
        if (selected()) useWikiStore.getState().setFileContent(body)
      }
      if (current() && embed.enabled && !/(?:^|\/)(index|log|overview)\.md$/i.test(path)) {
        try { await invoke("git_sync_embed_page", { projectPath: root, path, cfg: embed }) }
        catch { complete = false }
      }
    } else {
      if (selected()) { store.setSelectedFile(null); store.setFileContent("") }
      try { await removeSyncedPageEmbedding(root, pageId) } catch { complete = false }
    }
  }
  return complete
}

// One controller exists for the open project. Timers and listeners never
// capture the next project's path. A running native transaction finishes under
// its original project lock. Pending work is rechecked after acquiring it.
export class WikiSyncSession {
  private stopped = false
  private working = false
  private timer?: ReturnType<typeof setTimeout>
  private poll?: ReturnType<typeof setInterval>
  private unlisten?: () => void
  private stopFallback?: () => void
  private watchGeneration = 0
  private indexing = false
  private retryAt = 0
  private followUp = false
  constructor(readonly project: WikiProject) {}
  private current = () => !this.stopped && useWikiStore.getState().project?.id === this.project.id &&
    useWikiStore.getState().project?.path === this.project.path
  private call<T>(name: string, args: Record<string, unknown> = {}) {
    return invoke<T>("git_sync_" + name, { projectPath: this.project.path, ...args })
  }
  async start() {
    useWikiSync.setState(initial)
    try {
      const off = await listen<string>("wiki-sync://dirty", e => {
        if (e.payload === this.project.id) this.dirty()
      })
      if (!this.current()) { off(); return }
      this.unlisten = off
      await this.status()
      if (!this.current()) return
      this.poll = setInterval(() => this.auto(), 30_000)
      this.auto()
    } catch (error) { this.report(error) }
  }
  async watchFallback(required: boolean) {
    const generation = ++this.watchGeneration
    this.stopFallback?.()
    this.stopFallback = undefined
    if (!required || !this.current()) return
    const id = crypto.randomUUID()
    const changed = new Channel<null>()
    changed.onmessage = () => this.dirty()
    try {
      await this.call("watch", { id, changed })
      const stop = () => { void this.call("unwatch", { id }).catch(() => {}) }
      if (!this.current() || generation !== this.watchGeneration) stop()
      else this.stopFallback = stop
    } catch { /* the 30-second reconciliation still runs */ }
  }
  stop() {
    this.stopped = true
    clearTimeout(this.timer)
    clearInterval(this.poll)
    this.unlisten?.()
    this.stopFallback?.()
  }
  private report(error: unknown) {
    if (!this.current()) return
    const f = asFault(error)
    this.retryAt = Date.now() + 30_000
    useWikiSync.setState({ phase: f.kind, detail: f.message, files: f.files, ready: true })
  }
  dirty() {
    if (!this.current()) return
    clearTimeout(this.timer)
    this.timer = setTimeout(() => this.auto(), 3000)
  }
  private auto() {
    const { config, phase, userId } = useWikiSync.getState()
    if (!this.current() || !userId || !config?.enabled || Date.now() < this.retryAt ||
      ["Conflict", "Authentication Error", "Error"].includes(phase)) return
    if (this.working) { this.followUp = true; return }
    void this.action("now")
  }
  private async status() {
    const status = await this.call<Status>("status")
    if (!this.current()) return
    const { config, server, defaultRemote, conflict, changes } = status
    useWikiSync.setState({ config, server, defaultRemote, ready: true,
      phase: conflict?.kind ?? (config ? config.lastSync ? "Synced" : "Connected" : "Not Configured"),
      detail: conflict?.message ?? "", files: conflict?.files ?? [] })
    if (changes.token && !conflict && !this.indexing) {
      this.indexing = true
      void (async () => {
        const ok = await refreshGitPages(this.project, changes, this.current)
        if (!this.current()) return
        if (ok) await this.call("acknowledge", { token: changes.token })
        if (this.current()) useWikiSync.setState({ indexMessage: ok ? "" : "wikiSync.indexPending" })
      })().catch(() => {
        if (this.current()) useWikiSync.setState({ indexMessage: "wikiSync.indexPending" })
      }).finally(() => { this.indexing = false })
    }
  }
  async action(name: "connect" | "now" | "enable" | "disconnect", args: Record<string, unknown> = {}) {
    if (this.working || !this.current()) return
    const { editing, userId } = useWikiSync.getState()
    if (editing && (name === "now" || name === "connect")) {
      useWikiSync.setState({ detail: "wikiSync.waitForEdit" })
      return
    }
    this.working = true
    useWikiSync.setState({ busy: true, phase: name === "connect" ? "Connecting" : "Syncing", detail: "" })
    try {
      await withProjectLock(normalizePath(this.project.path), async () => {
        if (!this.current()) return
        if (name === "connect") await this.call(name, { ...args, userId })
        if (name === "now" || name === "connect") {
          const syncArgs = { userId, resolve: false, ...(name === "now" ? args : {}) }
          try { await this.call("now", syncArgs) }
          catch (error) {
            if (asFault(error).kind !== "Retry" || !this.current()) throw error
            await this.call("now", { userId, resolve: false })
          }
        }
        if (name === "enable" || name === "disconnect") await this.call(name, args)
      })
      if (!this.current()) return
      await this.status()
      this.retryAt = 0
    } catch (error) {
      // Even a failed push can follow a successful clone/merge.
      if (!this.current()) return
      try { await this.status() } catch { /* show the original failure */ }
      this.report(error)
    } finally {
      this.working = false
      if (this.current()) {
        useWikiSync.setState({ busy: false })
        if (this.followUp) { this.followUp = false; this.dirty() }
      }
    }
  }
}

let session: WikiSyncSession | null = null
export function activeWikiSync() { return session }
export function startWikiSync(project: WikiProject) {
  session?.stop()
  const next = new WikiSyncSession(project)
  session = next
  void next.start()
  return () => { next.stop(); if (session === next) session = null }
}


// Project switches drain the current filesystem transaction before resetting
// the editor/ingest stores. The dismissed session cannot schedule another.
export async function stopWikiSync() {
  const previous = session
  if (!previous) return
  previous.stop()
  session = null
  await withProjectLock(normalizePath(previous.project.path), async () => {})
}
