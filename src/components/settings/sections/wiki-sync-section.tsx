import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { openPathInProject } from "@/commands/fs"
import { saveWikiUserId, validWikiUserId } from "@/lib/project-store"
import { activeWikiSync, startWikiSync, useWikiSync } from "@/lib/wiki-sync"
import { useWikiStore } from "@/stores/wiki-store"

const inputClass = "w-full rounded-md border bg-background px-3 py-2 text-sm"
function IdentityForm({ initial }: { initial: string }) {
  const { t } = useTranslation()
  const [id, setId] = useState(initial)
  const [error, setError] = useState("")
  return <form className="space-y-2" onSubmit={async e => {
    e.preventDefault()
    try {
      const userId = await saveWikiUserId(id)
      useWikiSync.setState({ userId })
      setError("")
    } catch (err) { setError(String(err)) }
  }}>
    <label className="block text-sm">{t("wikiSync.userId")}
      <input className={inputClass} value={id} required
        autoComplete="off" onChange={e => setId(e.target.value)} />
    </label>
    <p className="text-xs text-muted-foreground">{t("wikiSync.idHint")}</p>
    <Button type="submit" disabled={!validWikiUserId(id) || id.normalize("NFC") === initial}>{t("wikiSync.saveId")}</Button>
    {error && <p role="alert">{error}</p>}
  </form>
}

export function WikiSyncMonitor() {
  const { t } = useTranslation()
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
    title={detail === "wikiSync.waitForEdit" ? t("wikiSync.waitForEdit") : detail} onClick={() => {
      useWikiSync.setState({ openSettings: true })
      useWikiStore.getState().setActiveView("settings")
    }}>{offer ? t("wikiSync.offer") : `${t("wikiSync.title")} · ${t(`wikiSync.phase.${phase}`)}`}</button>
}

export function WikiSyncSection() {
  const { t } = useTranslation()
  const state = useWikiSync()
  const project = useWikiStore(s => s.project)
  const [remote, setRemote] = useState(state.defaultRemote)
  const [error, setError] = useState("")
  useEffect(() => { setRemote(state.defaultRemote) }, [state.defaultRemote, project?.id])
  const { config, phase, detail, files, busy } = state
  const run = activeWikiSync()
  const connect = (clone: boolean, backup = false) => void run?.action("connect", { remote, clone, backup })
  return <section className="space-y-6">
    <h2 className="text-xl font-semibold">{t("wikiSync.title")}</h2>
    <IdentityForm key={state.userId} initial={state.userId} />
    <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
      <dt>{t("wikiSync.server")}</dt><dd className="break-all">{state.server || t("wikiSync.askAdmin")}</dd>
      <dt>{t("wikiSync.status")}</dt><dd aria-live="polite">{t(`wikiSync.phase.${phase}`)}</dd>
      {config && <>
        <dt>{t("wikiSync.repository")}</dt><dd className="break-all">{config.remote} · {config.branch}</dd>
        <dt>{t("wikiSync.lastSync")}</dt><dd>{config.lastSync ? new Date(config.lastSync * 1000).toLocaleString() : t("wikiSync.pending")}</dd>
        {config.backup && <><dt>{t("wikiSync.backup")}</dt><dd className="break-all">{config.backup}</dd></>}
      </>}
    </dl>
    {detail && <p role="status" className="text-sm">{detail === "wikiSync.waitForEdit" ? t("wikiSync.waitForEdit") : detail}</p>}
    {state.indexMessage && <p role="status" className="text-sm">{t(state.indexMessage)}</p>}
    {files.length > 0 && <ul className="list-inside list-disc text-sm">{files.map(f => <li key={f}>wiki/{f}</li>)}</ul>}
    <fieldset disabled={busy || !state.ready} className="space-y-3">
      {!config ? <>
        <label className="block text-sm">{t("wikiSync.url")}
          <input className={inputClass} value={remote} placeholder="ssh://git@gitea.internal/team/wiki.git"
            onChange={e => setRemote(e.target.value)} />
        </label>
        <div className="flex flex-wrap gap-2">
          <Button disabled={!remote || !state.userId} onClick={() => connect(true)}>{t("wikiSync.clone")}</Button>
          <Button variant="outline" disabled={!remote || !state.userId} onClick={() => connect(false)}>{t("wikiSync.connect")}</Button>
          {phase === "Backup Required" && <Button variant="outline" onClick={() => connect(true, true)}>{t("wikiSync.backupClone")}</Button>}
        </div>
      </> : <>
        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={config.enabled}
          onChange={e => void run?.action("enable", { enabled: e.target.checked })} />{t("wikiSync.auto")}</label>
        <div className="flex flex-wrap gap-2">
          <Button disabled={!state.userId} onClick={() => void run?.action("now", { resolve: phase === "Conflict" })}>
            {phase === "Conflict" ? t("wikiSync.retry") : t("wikiSync.now")}
          </Button>
          <Button variant="outline" onClick={() => { if (project) void openPathInProject(project.path, "wiki").catch(e => setError(String(e))) }}>{t("wikiSync.open")}</Button>
          <Button variant="outline" onClick={() => void run?.action("disconnect")}>{t("wikiSync.disconnect")}</Button>
        </div>
      </>}
    </fieldset>
    {error && <p role="alert">{error}</p>}
  </section>
}
