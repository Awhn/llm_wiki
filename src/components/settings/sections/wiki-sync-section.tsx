import { useEffect, useState, type ReactNode } from "react"
import { Button } from "@/components/ui/button"
import { openPathInProject } from "@/commands/fs"
import { loadWikiUserId, saveWikiUserId, validWikiUserId } from "@/lib/project-store"
import { activeWikiSync, startWikiSync, useWikiSync } from "@/lib/wiki-sync"
import { useWikiStore } from "@/stores/wiki-store"

const inputClass = "w-full rounded-md border bg-background px-3 py-2 text-sm"
function IdentityForm({ initial = "", first = false }: { initial?: string; first?: boolean }) {
  const [id, setId] = useState(initial)
  const [error, setError] = useState("")
  return <form className="space-y-2" onSubmit={async e => {
    e.preventDefault()
    try {
      await saveWikiUserId(id)
      useWikiSync.setState({ userId: id })
      setError("")
    } catch (err) { setError(String(err)) }
  }}>
    <label className="block text-sm">User ID
      <input className={inputClass} autoFocus={first} value={id} maxLength={64} required
        autoComplete="off" onChange={e => setId(e.target.value)} />
    </label>
    <p className="text-xs text-muted-foreground">Your Wiki commits use this name. Start with a letter or digit. Dots, underscores and hyphens are allowed.</p>
    <Button type="submit" disabled={!validWikiUserId(id) || (!first && id === initial)}>{first ? "Continue" : "Save user ID"}</Button>
    {error && <p role="alert">{error}</p>}
  </form>
}

export function WikiIdentityGate({ children }: { children: ReactNode }) {
  const userId = useWikiSync(s => s.userId)
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState("")
  useEffect(() => {
    let active = true
    loadWikiUserId().then(id => {
      if (active && id) useWikiSync.setState({ userId: id })
    }).catch(() => { if (active) setError("Could not read the saved user ID.") })
      .finally(() => { if (active) setLoaded(true) })
    return () => { active = false }
  }, [])
  if (!loaded) return null
  if (userId) return children
  return <main className="grid h-screen place-items-center bg-background p-6 text-foreground">
    <div className="w-full max-w-sm space-y-4">
      <h1 className="text-xl font-semibold">Welcome to LLM Wiki</h1>
      {error && <p role="alert">{error}</p>}
      <IdentityForm first />
    </div>
  </main>
}

export function WikiSyncMonitor() {
  const project = useWikiStore(s => s.project)
  const sourceWatcher = useWikiStore(s => s.sourceWatchConfig.enabled)
  const view = useWikiStore(s => s.activeView)
  const { config, ready, server, phase, detail } = useWikiSync()
  useEffect(() => { if (project) return startWikiSync(project) }, [project])
  useEffect(() => {
    const worker = activeWikiSync()
    void worker?.watchFallback(Boolean(config?.enabled && !sourceWatcher))
    return () => { void worker?.watchFallback(false) }
  }, [project, config?.enabled, sourceWatcher])
  if (!ready || !project || view === "settings") return null
  const offer = !config && Boolean(server)
  if (!offer && !["Conflict", "Authentication Error", "Error", "Offline"].includes(phase)) return null
  return <button className="fixed bottom-3 right-3 z-40 max-w-sm rounded-md border bg-background px-3 py-2 text-left text-sm shadow"
    title={detail} onClick={() => {
      useWikiSync.setState({ openSettings: true })
      useWikiStore.getState().setActiveView("settings")
    }}>{offer ? "Connect shared Wiki…" : "Wiki Sync · " + phase}</button>
}

export function WikiSyncSection() {
  const state = useWikiSync()
  const project = useWikiStore(s => s.project)
  const [remote, setRemote] = useState(state.defaultRemote)
  const [error, setError] = useState("")
  useEffect(() => { setRemote(state.defaultRemote) }, [state.defaultRemote, project?.id])
  const { config, phase, detail, files, busy } = state
  const run = activeWikiSync()
  const connect = (clone: boolean, backup = false) => void run?.action("connect", { remote, clone, backup })
  return <section className="space-y-6">
    <h2 className="text-xl font-semibold">Wiki Sync</h2>
    <IdentityForm key={state.userId} initial={state.userId} />
    <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
      <dt>Server</dt><dd className="break-all">{state.server || "Ask your administrator to configure the Gitea server."}</dd>
      <dt>Status</dt><dd aria-live="polite">{phase}</dd>
      {config && <>
        <dt>Repository</dt><dd className="break-all">{config.remote} · {config.branch}</dd>
        <dt>Last sync</dt><dd>{config.lastSync ? new Date(config.lastSync * 1000).toLocaleString() : "Pending"}</dd>
        {config.backup && <><dt>Local backup</dt><dd className="break-all">{config.backup}</dd></>}
      </>}
    </dl>
    {detail && <p role="status" className="text-sm">{detail}</p>}
    {state.indexMessage && <p role="status" className="text-sm">{state.indexMessage}</p>}
    {files.length > 0 && <ul className="list-inside list-disc text-sm">{files.map(f => <li key={f}>wiki/{f}</li>)}</ul>}
    <fieldset disabled={busy || !state.ready} className="space-y-3">
      {!config ? <>
        <label className="block text-sm">Repository URL
          <input className={inputClass} value={remote} placeholder="ssh://git@gitea.internal/team/wiki.git"
            onChange={e => setRemote(e.target.value)} />
        </label>
        <div className="flex flex-wrap gap-2">
          <Button disabled={!remote} onClick={() => connect(true)}>Clone existing repository</Button>
          <Button variant="outline" disabled={!remote} onClick={() => connect(false)}>Connect local Wiki</Button>
          {phase === "Backup Required" && <Button variant="outline" onClick={() => connect(true, true)}>Back up local Wiki and clone</Button>}
        </div>
      </> : <>
        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={config.enabled}
          onChange={e => void run?.action("enable", { enabled: e.target.checked })} />Auto sync · every 30 seconds</label>
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => void run?.action("now", { resolve: phase === "Conflict" })}>
            {phase === "Conflict" ? "Retry after resolution" : "Sync now"}
          </Button>
          <Button variant="outline" onClick={() => { if (project) void openPathInProject(project.path, "wiki").catch(e => setError(String(e))) }}>Open Wiki folder</Button>
          <Button variant="outline" onClick={() => void run?.action("disconnect")}>Disconnect</Button>
        </div>
      </>}
    </fieldset>
    {error && <p role="alert">{error}</p>}
  </section>
}
