import { useState, type FormEvent } from "react"
import { useTranslation } from "react-i18next"
import { open } from "@tauri-apps/plugin-dialog"
import { invoke } from "@tauri-apps/api/core"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { openProject } from "@/commands/fs"
import { saveWikiUserId, validWikiUserId } from "@/lib/project-store"
import { useWikiSync } from "@/lib/wiki-sync"
import type { WikiProject } from "@/types/wiki"

const errorKeys: Record<string, string> = {
  "Missing Origin": "wikiSync.askAdmin",
  "Invalid Remote": "wikiSync.invalidRemote",
  "Destination Exists": "wikiSync.folderExists",
  "Authentication Error": "wikiSync.joinAuthError",
  Offline: "wikiSync.joinOffline",
}

export function JoinProjectDialog({ onClose, onJoined }: {
  onClose: () => void
  onJoined: (project: WikiProject) => Promise<void>
}) {
  const { t } = useTranslation()
  const [id, setId] = useState(() => useWikiSync.getState().userId)
  const [remote, setRemote] = useState("")
  const [localPath, setLocalPath] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const canJoin = validWikiUserId(id) && Boolean(remote.trim())

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (busy || !canJoin) return
    setBusy(true)
    setError("")
    try {
      let path = localPath
      if (!path) {
        const parent = await open({ directory: true, multiple: false, title: t("project.parentDir") })
        if (!parent) return
        path = await invoke<string>("git_sync_join", { parent, remote: remote.trim(), userId: id.normalize("NFC") })
        setLocalPath(path)
      }
      const userId = await saveWikiUserId(id)
      useWikiSync.setState({ userId })
      await onJoined(await openProject(path))
      onClose()
    } catch (err) {
      if (err && typeof err === "object" && "kind" in err && "message" in err) {
        const key = errorKeys[String(err.kind)]
        setError(key ? t(key) : String(err.message))
      } else {
        setError(String(err))
      }
    } finally {
      setBusy(false)
    }
  }

  return <Dialog open onOpenChange={next => { if (!next && !busy) onClose() }}>
    <DialogContent className="sm:max-w-md" showCloseButton={!busy}>
      <DialogHeader>
        <DialogTitle>{t("wikiSync.joinProject")}</DialogTitle>
        <DialogDescription>{t("wikiSync.joinHint")}</DialogDescription>
      </DialogHeader>
      <form className="grid gap-4" onSubmit={submit}>
        <fieldset className="grid gap-4" disabled={busy || Boolean(localPath)}>
          <div className="grid gap-1.5">
            <Label htmlFor="join-wiki-id">{t("wikiSync.userId")}</Label>
            <Input id="join-wiki-id" value={id} required autoComplete="off" aria-describedby="join-id-hint"
              onChange={event => { setId(event.target.value); setError("") }} />
            <p id="join-id-hint" className="text-xs text-muted-foreground">{t("wikiSync.idHint")}</p>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="join-wiki-remote">{t("wikiSync.url")}</Label>
            <Input id="join-wiki-remote" value={remote} required spellCheck={false} autoComplete="off"
              placeholder="ssh://git@gitea.internal/team/wiki.git"
              onChange={event => { setRemote(event.target.value); setError("") }} />
          </div>
        </fieldset>
        {localPath && <p className="break-all text-sm">{localPath}</p>}
        {error && <p role="alert" className="break-words text-sm text-destructive">{error}</p>}
        <DialogFooter>
          <Button type="button" variant="outline" disabled={busy} onClick={onClose}>{t("project.cancel")}</Button>
          <Button type="submit" disabled={busy || !canJoin}>
            {busy ? t("wikiSync.phase.Connecting") : localPath ? t("welcome.openProject") : t("wikiSync.joinAction")}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}
