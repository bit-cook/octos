import { useState, useEffect, useCallback } from 'react'
import { api } from '../api'
import { useToast } from '../components/Toast'
import type { SiteStatus } from '../types'

export default function SitesPage() {
  const [sites, setSites] = useState<SiteStatus[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const { toast } = useToast()

  const refresh = useCallback(async () => {
    try { setSites(await api.listSites()) }
    catch (e: any) { toast(e.message, 'error') }
    finally { setLoading(false) }
  }, [toast])

  useEffect(() => { refresh(); const iv = setInterval(refresh, 10000); return () => clearInterval(iv) }, [refresh])

  const handleStart = async (id: string) => { try { await api.startSite(id); toast('Site started', 'success'); refresh() } catch (e: any) { toast(e.message, 'error') } }
  const handleStop = async (id: string) => { try { await api.stopSite(id); toast('Site stopped', 'success'); refresh() } catch (e: any) { toast(e.message, 'error') } }
  const handleDelete = async (id: string, name: string) => {
    if (!confirm(`Delete site "${name}"?`)) return
    try { await api.deleteSite(id); toast('Site deleted', 'success'); refresh() } catch (e: any) { toast(e.message, 'error') }
  }

  if (loading) return <p className="text-gray-500">Loading sites...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-white">Sites</h1>
          <p className="text-sm text-gray-500 mt-1">{sites.length} site{sites.length !== 1 ? 's' : ''} &middot; {sites.filter(s => s.running).length} running</p>
        </div>
        <div className="flex gap-2">
          <button onClick={() => api.reloadTunnel().then(() => toast('Tunnel reloaded', 'success')).catch((e: any) => toast(e.message, 'error'))} className="px-3 py-2 text-sm bg-white/5 text-gray-400 hover:text-gray-200 hover:bg-white/10 rounded-lg border border-gray-700/50 transition">Reload Tunnel</button>
          <button onClick={() => setShowCreate(true)} className="px-4 py-2 text-sm bg-accent text-white hover:bg-accent/80 rounded-lg font-medium transition">+ New Site</button>
        </div>
      </div>

      {sites.length === 0 ? (
        <div className="text-center py-16 text-gray-500"><p className="text-lg mb-2">No sites yet</p><p className="text-sm">Click "+ New Site" to create one.</p></div>
      ) : (
        <div className="space-y-3">
          {sites.map(site => (
            <div key={site.id} className="bg-surface rounded-xl border border-gray-700/50 p-4 flex items-center gap-4">
              <div className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${site.running ? 'bg-green-500 shadow-[0_0_6px_rgba(34,197,94,0.5)]' : 'bg-gray-600'}`} />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium text-white">{site.name}</p>
                <p className="text-xs text-gray-500 mt-0.5">
                  <a href={site.url} target="_blank" rel="noopener" className="text-accent hover:underline">{site.url}</a>
                  {' '}&middot; <a href={site.local_url} target="_blank" rel="noopener" className="hover:text-gray-300">:{site.port}</a>
                  {' '}&middot; {site.running ? 'running' : 'stopped'}
                </p>
              </div>
              <div className="flex gap-2 flex-shrink-0">
                {site.running
                  ? <button onClick={() => handleStop(site.id)} className="px-3 py-1.5 text-xs bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 border border-amber-500/20 rounded-lg transition">Stop</button>
                  : <button onClick={() => handleStart(site.id)} className="px-3 py-1.5 text-xs bg-green-500/10 text-green-400 hover:bg-green-500/20 border border-green-500/20 rounded-lg transition">Start</button>
                }
                <button onClick={() => handleDelete(site.id, site.name)} className="px-3 py-1.5 text-xs bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/20 rounded-lg transition">Delete</button>
              </div>
            </div>
          ))}
        </div>
      )}

      {showCreate && <CreateSiteModal onClose={() => setShowCreate(false)} onCreated={() => { setShowCreate(false); refresh() }} />}
    </div>
  )
}

function CreateSiteModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const [name, setName] = useState('')
  const [subdomain, setSubdomain] = useState('')
  const [title, setTitle] = useState('')
  const [creating, setCreating] = useState(false)
  const { toast } = useToast()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim() || !subdomain.trim()) { toast('Name and subdomain required', 'error'); return }
    setCreating(true)
    try {
      const site = await api.createSite({ name: name.trim(), subdomain: subdomain.trim(), title: title.trim() || undefined })
      toast(`Site created: ${site.url}`, 'success')
      onCreated()
    } catch (e: any) { toast(e.message, 'error') }
    finally { setCreating(false) }
  }

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={onClose}>
      <div className="bg-surface rounded-2xl border border-gray-700/50 p-6 w-[420px] max-w-[90vw]" onClick={e => e.stopPropagation()}>
        <h2 className="text-lg font-bold text-white mb-5">New Site</h2>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs text-gray-400 mb-1.5">Site Name</label>
            <input value={name} onChange={e => setName(e.target.value)} placeholder="my-site" autoFocus className="w-full px-3 py-2 bg-bg border border-gray-700/50 rounded-lg text-sm text-white placeholder-gray-600 outline-none focus:border-accent" />
          </div>
          <div>
            <label className="block text-xs text-gray-400 mb-1.5">Subdomain</label>
            <input value={subdomain} onChange={e => setSubdomain(e.target.value)} placeholder="my-site" className="w-full px-3 py-2 bg-bg border border-gray-700/50 rounded-lg text-sm text-white placeholder-gray-600 outline-none focus:border-accent" />
            <p className="text-[10px] text-gray-600 mt-1">https://{subdomain || '...'}.octos-cloud.org</p>
          </div>
          <div>
            <label className="block text-xs text-gray-400 mb-1.5">Title (optional)</label>
            <input value={title} onChange={e => setTitle(e.target.value)} placeholder="My Site" className="w-full px-3 py-2 bg-bg border border-gray-700/50 rounded-lg text-sm text-white placeholder-gray-600 outline-none focus:border-accent" />
          </div>
          <div className="flex gap-3 justify-end pt-2">
            <button type="button" onClick={onClose} className="px-4 py-2 text-sm text-gray-400 hover:text-gray-200 bg-white/5 hover:bg-white/10 rounded-lg transition">Cancel</button>
            <button type="submit" disabled={creating} className="px-4 py-2 text-sm bg-accent text-white hover:bg-accent/80 rounded-lg font-medium transition disabled:opacity-50">{creating ? 'Creating...' : 'Create'}</button>
          </div>
        </form>
      </div>
    </div>
  )
}
