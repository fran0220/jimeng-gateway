import { useCallback, useEffect, useMemo, useState } from 'react'

const POLL_MS = 5000

const TABS = [
  { key: 'dashboard', label: 'Dashboard' },
  { key: 'tasks', label: 'Tasks' },
  { key: 'sessions', label: 'Sessions' },
  { key: 'logs', label: 'Logs' },
]

async function apiRequest(path, options = {}) {
  const headers = { ...(options.headers || {}) }
  if (options.body && !headers['Content-Type']) {
    headers['Content-Type'] = 'application/json'
  }

  const response = await fetch(path, {
    ...options,
    headers,
  })

  const text = await response.text()
  const payload = text ? JSON.parse(text) : null
  if (!response.ok) {
    throw new Error(payload?.error || payload?.message || `${response.status} ${response.statusText}`)
  }
  return payload
}

function StatusBadge({ value }) {
  return <span className={`status-badge status-${value || 'unknown'}`}>{value || 'unknown'}</span>
}

function StatCard({ title, value, hint }) {
  return (
    <div className="card stat-card">
      <div className="stat-title">{title}</div>
      <div className="stat-value">{value}</div>
      {hint ? <div className="stat-hint">{hint}</div> : null}
    </div>
  )
}

function DashboardPage({ stats, health, tasks, refresh }) {
  const recentTasks = useMemo(() => (tasks || []).slice(0, 8), [tasks])

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>Gateway Health</h2>
        <button onClick={refresh}>Refresh</button>
      </div>
      <div className="stats-grid">
        <StatCard title="Total" value={stats?.total ?? 0} />
        <StatCard title="Running" value={stats?.running ?? 0} hint={`${stats?.queued ?? 0} queued`} />
        <StatCard title="Succeeded" value={stats?.succeeded ?? 0} />
        <StatCard title="Failed" value={stats?.failed ?? 0} />
      </div>

      <div className="card">
        <div className="row between">
          <h3>Runtime</h3>
          <StatusBadge value={health?.ok ? 'healthy' : 'degraded'} />
        </div>
        <div className="kv-grid">
          <div>Gateway Version</div>
          <strong>{health?.gateway_version || 'n/a'}</strong>
          <div>Sessions</div>
          <strong>{health?.sessions?.healthy ?? 0} / {health?.sessions?.total ?? 0}</strong>
          <div>Container</div>
          <strong>{health?.container?.state || health?.container?.status || 'unknown'}</strong>
        </div>
      </div>

      <div className="card">
        <div className="row between">
          <h3>Recent Tasks</h3>
          <small>{recentTasks.length} items</small>
        </div>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>ID</th>
                <th>Status</th>
                <th>Prompt</th>
                <th>Queue</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {recentTasks.map((task) => (
                <tr key={task.id}>
                  <td className="mono">{task.id.slice(0, 8)}</td>
                  <td><StatusBadge value={task.status} /></td>
                  <td title={task.prompt}>{task.prompt || '-'}</td>
                  <td>{task.queue_position ? `${task.queue_position}/${task.queue_total || '-'}` : '-'}</td>
                  <td>{task.updated_at?.replace('T', ' ').slice(0, 19) || '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  )
}

function TasksPage({ tasks, refresh, onCancel, onRetry, loading, statusFilter, setStatusFilter }) {
  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>Task Queue</h2>
        <div className="row">
          <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)}>
            <option value="">All</option>
            <option value="queued">queued</option>
            <option value="submitting">submitting</option>
            <option value="polling">polling</option>
            <option value="downloading">downloading</option>
            <option value="succeeded">succeeded</option>
            <option value="failed">failed</option>
            <option value="cancelled">cancelled</option>
          </select>
          <button onClick={refresh}>{loading ? 'Refreshing...' : 'Refresh'}</button>
        </div>
      </div>

      <div className="card">
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>ID</th>
                <th>Status</th>
                <th>Model</th>
                <th>Queue</th>
                <th>ETA</th>
                <th>Error</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {tasks.map((task) => {
                const canCancel = ['queued', 'submitting', 'polling'].includes(task.status)
                const canRetry = ['failed', 'cancelled', 'succeeded'].includes(task.status)
                return (
                  <tr key={task.id}>
                    <td className="mono" title={task.id}>{task.id.slice(0, 8)}</td>
                    <td><StatusBadge value={task.status} /></td>
                    <td>{task.model}</td>
                    <td>{task.queue_position ? `${task.queue_position}/${task.queue_total || '-'}` : '-'}</td>
                    <td>{task.queue_eta || '-'}</td>
                    <td title={task.error_message || ''}>{task.error_kind || '-'}</td>
                    <td>
                      <div className="row">
                        <button disabled={!canCancel} onClick={() => onCancel(task.id)}>Cancel</button>
                        <button disabled={!canRetry} onClick={() => onRetry(task.id)}>Retry</button>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  )
}

function SessionsPage({ sessions, refresh, onAdd, onRemove, onToggle, onTest }) {
  const [label, setLabel] = useState('')
  const [sessionId, setSessionId] = useState('')

  const submit = async (event) => {
    event.preventDefault()
    if (!sessionId.trim()) return
    await onAdd({ label: label.trim(), session_id: sessionId.trim() })
    setSessionId('')
  }

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>Session Pool</h2>
        <button onClick={refresh}>Refresh</button>
      </div>

      <form className="card form-grid" onSubmit={submit}>
        <h3>Add Session</h3>
        <label>
          Label
          <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="optional" />
        </label>
        <label>
          Session ID
          <input value={sessionId} onChange={(e) => setSessionId(e.target.value)} required placeholder="paste jimeng sessionid" />
        </label>
        <div className="row">
          <button type="submit">Add</button>
        </div>
      </form>

      <div className="card">
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Label</th>
                <th>Status</th>
                <th>Active</th>
                <th>Success/Fail</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session) => (
                <tr key={session.id}>
                  <td>{session.label || '-'}</td>
                  <td>
                    <StatusBadge value={session.enabled ? (session.healthy ? 'healthy' : 'unhealthy') : 'disabled'} />
                  </td>
                  <td>{session.active_tasks}</td>
                  <td>{session.success_count}/{session.fail_count}</td>
                  <td>
                    <div className="row">
                      <button onClick={() => onToggle(session.id, !session.enabled)}>{session.enabled ? 'Disable' : 'Enable'}</button>
                      <button onClick={() => onTest(session.id)}>Test</button>
                      <button onClick={() => onRemove(session.id)}>Remove</button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  )
}

function LogsPage({ logs, refresh }) {
  const [query, setQuery] = useState('')
  const [lines, setLines] = useState(200)

  const filtered = useMemo(() => {
    if (!query.trim()) return logs
    const needle = query.toLowerCase()
    return logs.filter((line) => line.toLowerCase().includes(needle))
  }, [logs, query])

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>Container Logs</h2>
        <div className="row">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="search logs"
          />
          <input
            type="number"
            min="50"
            max="1000"
            value={lines}
            onChange={(e) => setLines(Number(e.target.value || 200))}
          />
          <button onClick={() => refresh(lines)}>Refresh</button>
        </div>
      </div>

      <div className="card logs-panel">
        <div className="row between">
          <small>{filtered.length} lines</small>
          <small>Auto refresh: 3s</small>
        </div>
        <pre>{filtered.join('\n') || 'No logs yet.'}</pre>
      </div>
    </section>
  )
}

export default function App() {
  const [tab, setTab] = useState('dashboard')
  const [stats, setStats] = useState(null)
  const [health, setHealth] = useState(null)
  const [tasks, setTasks] = useState([])
  const [sessions, setSessions] = useState([])
  const [logs, setLogs] = useState([])
  const [statusFilter, setStatusFilter] = useState('')
  const [loadingTasks, setLoadingTasks] = useState(false)
  const [error, setError] = useState('')

  const handleError = useCallback((err) => {
    setError(err.message || String(err))
    setTimeout(() => setError(''), 5000)
  }, [])

  const loadStats = useCallback(async () => {
    try {
      const [statsPayload, healthPayload] = await Promise.all([
        apiRequest('/api/v1/stats'),
        apiRequest('/api/v1/health'),
      ])
      setStats(statsPayload)
      setHealth(healthPayload)
    } catch (err) {
      handleError(err)
    }
  }, [handleError])

  const loadTasks = useCallback(async (filter = statusFilter) => {
    try {
      setLoadingTasks(true)
      const qs = new URLSearchParams({ limit: '200' })
      if (filter) qs.set('status', filter)
      const payload = await apiRequest(`/api/v1/tasks?${qs.toString()}`)
      setTasks(payload.tasks || [])
    } catch (err) {
      handleError(err)
    } finally {
      setLoadingTasks(false)
    }
  }, [handleError, statusFilter])

  const loadSessions = useCallback(async () => {
    try {
      const payload = await apiRequest('/api/v1/sessions')
      setSessions(payload.sessions || [])
    } catch (err) {
      handleError(err)
    }
  }, [handleError])

  const loadLogs = useCallback(async (lines = 200) => {
    try {
      const payload = await apiRequest(`/api/v1/logs?lines=${lines}`)
      setLogs(payload.logs || [])
    } catch (err) {
      handleError(err)
    }
  }, [handleError])

  const cancelTask = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/tasks/${id}/cancel`, { method: 'POST' })
      await loadTasks()
      await loadStats()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadStats, loadTasks])

  const retryTask = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/tasks/${id}/retry`, { method: 'POST' })
      await loadTasks()
      await loadStats()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadStats, loadTasks])

  const addSession = useCallback(async (body) => {
    try {
      await apiRequest('/api/v1/sessions', {
        method: 'POST',
        body: JSON.stringify(body),
      })
      await loadSessions()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadSessions])

  const removeSession = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/sessions/${id}`, { method: 'DELETE' })
      await loadSessions()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadSessions])

  const toggleSession = useCallback(async (id, enabled) => {
    try {
      await apiRequest(`/api/v1/sessions/${id}`, {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      })
      await loadSessions()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadSessions])

  const testSession = useCallback(async (id) => {
    try {
      const payload = await apiRequest(`/api/v1/sessions/${id}/test`, { method: 'POST' })
      if (!payload.ok) {
        throw new Error(payload.message || 'Session test failed')
      }
      await loadSessions()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadSessions])

  useEffect(() => {
    loadStats()
    loadTasks()
    loadSessions()
    loadLogs()
  }, [loadLogs, loadSessions, loadStats, loadTasks])

  useEffect(() => {
    const id = setInterval(() => {
      loadStats()
      loadTasks()
      loadSessions()
    }, POLL_MS)
    return () => clearInterval(id)
  }, [loadSessions, loadStats, loadTasks])

  useEffect(() => {
    const id = setInterval(() => loadLogs(), 3000)
    return () => clearInterval(id)
  }, [loadLogs])

  useEffect(() => {
    loadTasks(statusFilter)
  }, [loadTasks, statusFilter])

  return (
    <div className="app-shell">
      <header className="app-header">
        <h1>Jimeng Gateway</h1>
        <p>Session rotation, async queue, and live operational telemetry</p>
      </header>

      <nav className="tab-bar">
        {TABS.map((item) => (
          <button
            key={item.key}
            className={item.key === tab ? 'active' : ''}
            onClick={() => setTab(item.key)}
          >
            {item.label}
          </button>
        ))}
      </nav>

      {error ? <div className="error-banner">{error}</div> : null}

      {tab === 'dashboard' ? (
        <DashboardPage stats={stats} health={health} tasks={tasks} refresh={loadStats} />
      ) : null}

      {tab === 'tasks' ? (
        <TasksPage
          tasks={tasks}
          refresh={loadTasks}
          onCancel={cancelTask}
          onRetry={retryTask}
          loading={loadingTasks}
          statusFilter={statusFilter}
          setStatusFilter={setStatusFilter}
        />
      ) : null}

      {tab === 'sessions' ? (
        <SessionsPage
          sessions={sessions}
          refresh={loadSessions}
          onAdd={addSession}
          onRemove={removeSession}
          onToggle={toggleSession}
          onTest={testSession}
        />
      ) : null}

      {tab === 'logs' ? <LogsPage logs={logs} refresh={loadLogs} /> : null}
    </div>
  )
}
