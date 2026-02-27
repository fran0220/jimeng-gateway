import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ApiError, apiRequest, setUnauthorizedHandler } from './lib/api'
import { I18nProvider, useI18n, useT } from './lib/i18n'
import LoginPage from './pages/LoginPage'

const POLL_MS = 5000
const TAB_KEYS = ['dashboard', 'tasks', 'sessions', 'keys']
const TASK_STATUS_FILTERS = ['queued', 'submitting', 'polling', 'downloading', 'succeeded', 'failed', 'cancelled']

function formatDateTime(value) {
  if (!value) return '-'
  return value.replace('T', ' ').slice(0, 19)
}

function resolveCurrentUser(payload) {
  if (!payload || typeof payload !== 'object') return null
  if (payload.user && typeof payload.user === 'object') return payload.user
  return payload
}

function StatusBadge({ value }) {
  const t = useT()
  const normalized = (value || 'unknown').toLowerCase()
  const translated = t(`status.${normalized}`)
  const label = translated === `status.${normalized}` ? value || t('status.unknown') : translated

  return <span className={`status-badge status-${normalized}`}>{label}</span>
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

function DashboardPage({ stats, health, tasks, refresh, onSelectTask }) {
  const t = useT()
  const recentTasks = useMemo(() => (tasks || []).slice(0, 8), [tasks])

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>{t('dashboard.gatewayHealth')}</h2>
        <button onClick={() => refresh()}>{t('common.refresh')}</button>
      </div>
      <div className="stats-grid">
        <StatCard title={t('dashboard.total')} value={stats?.total ?? 0} />
        <StatCard
          title={t('dashboard.running')}
          value={stats?.running ?? 0}
          hint={t('dashboard.queuedHint', { count: stats?.queued ?? 0 })}
        />
        <StatCard title={t('dashboard.succeeded')} value={stats?.succeeded ?? 0} />
        <StatCard title={t('dashboard.failed')} value={stats?.failed ?? 0} />
      </div>

      <div className="card">
        <div className="row between">
          <h3>{t('dashboard.runtime')}</h3>
          <StatusBadge value={health?.ok ? 'healthy' : 'degraded'} />
        </div>
        <div className="kv-grid">
          <div>{t('dashboard.gatewayVersion')}</div>
          <strong>{health?.gateway_version || t('common.na')}</strong>
          <div>{t('dashboard.sessions')}</div>
          <strong>{health?.sessions?.healthy ?? 0} / {health?.sessions?.total ?? 0}</strong>
        </div>
      </div>

      <div className="card">
        <div className="row between">
          <h3>{t('dashboard.recentTasks')}</h3>
          <small>{t('dashboard.items', { count: recentTasks.length })}</small>
        </div>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t('table.id')}</th>
                <th>{t('table.status')}</th>
                <th>{t('table.prompt')}</th>
                <th>{t('table.queue')}</th>
                <th>{t('table.updated')}</th>
              </tr>
            </thead>
            <tbody>
              {recentTasks.map((task) => (
                <tr key={task.id}>
                  <td>
                    <span className="mono task-id-link" onClick={() => onSelectTask(task.id)}>
                      {task.id.slice(0, 8)}
                    </span>
                  </td>
                  <td><StatusBadge value={task.status} /></td>
                  <td className="prompt-cell" title={task.prompt}>{task.prompt || t('common.dash')}</td>
                  <td>{task.queue_position != null ? `${task.queue_position}/${task.queue_total || '-'}` : '-'}</td>
                  <td>{formatDateTime(task.updated_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  )
}

function TasksPage({ tasks, refresh, onCancel, onRetry, loading, statusFilter, setStatusFilter, onSelectTask }) {
  const t = useT()

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>{t('tasks.title')}</h2>
        <div className="row">
          <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}>
            <option value="">{t('tasks.filterAll')}</option>
            {TASK_STATUS_FILTERS.map((status) => (
              <option key={status} value={status}>{t(`status.${status}`)}</option>
            ))}
          </select>
          <button onClick={() => refresh(statusFilter)}>{loading ? t('common.refreshing') : t('common.refresh')}</button>
        </div>
      </div>

      <div className="card">
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t('table.id')}</th>
                <th>{t('table.status')}</th>
                <th>{t('common.model')}</th>
                <th>{t('table.prompt')}</th>
                <th>{t('table.queue')}</th>
                <th>{t('tasks.eta')}</th>
                <th>{t('tasks.videoUrl')}</th>
                <th>{t('common.error')}</th>
                <th>{t('common.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {tasks.map((task) => {
                const canCancel = ['queued', 'submitting', 'polling'].includes(task.status)
                const canRetry = ['failed', 'cancelled', 'succeeded'].includes(task.status)
                return (
                  <tr key={task.id}>
                    <td>
                      <span className="mono task-id-link" onClick={() => onSelectTask(task.id)}>
                        {task.id.slice(0, 8)}
                      </span>
                    </td>
                    <td><StatusBadge value={task.status} /></td>
                    <td>{task.model || t('common.dash')}</td>
                    <td className="prompt-cell" title={task.prompt}>{task.prompt || t('common.dash')}</td>
                    <td>{task.queue_position != null ? `${task.queue_position}/${task.queue_total || '-'}` : '-'}</td>
                    <td>{task.queue_eta || t('common.dash')}</td>
                    <td>
                      {task.video_url ? (
                        <a className="task-video-link" href={task.video_url} target="_blank" rel="noreferrer">▶</a>
                      ) : t('common.dash')}
                    </td>
                    <td title={task.error_message || ''}>{task.error_kind || t('common.dash')}</td>
                    <td>
                      <div className="row">
                        <button disabled={!canCancel} onClick={() => onCancel(task.id)}>{t('tasks.cancel')}</button>
                        <button disabled={!canRetry} onClick={() => onRetry(task.id)}>{t('tasks.retry')}</button>
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

function TaskDetailModal({ taskId, onClose }) {
  const t = useT()
  const [task, setTask] = useState(null)
  const [loading, setLoading] = useState(true)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!taskId) return
    setLoading(true)
    apiRequest(`/api/v1/tasks/${taskId}`)
      .then((payload) => setTask(payload.task || payload))
      .catch(() => setTask(null))
      .finally(() => setLoading(false))
  }, [taskId])

  const copyVideoUrl = async () => {
    if (!task?.video_url || !navigator?.clipboard?.writeText) return
    await navigator.clipboard.writeText(task.video_url)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  if (!taskId) return null

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-card" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{t('tasks.details')}</h2>
          <button onClick={onClose}>{t('tasks.close')}</button>
        </div>
        <div className="modal-body">
          {loading ? (
            <p>{t('common.refreshing')}</p>
          ) : !task ? (
            <p>{t('common.dash')}</p>
          ) : (
            <>
              <dl className="task-detail-grid">
                <dt>{t('table.id')}</dt>
                <dd className="mono">{task.id}</dd>

                <dt>{t('table.status')}</dt>
                <dd><StatusBadge value={task.status} /></dd>

                <dt>{t('tasks.model')}</dt>
                <dd>{task.model || t('common.dash')}</dd>

                <dt>{t('tasks.duration')}</dt>
                <dd>{task.duration ? `${task.duration}s` : t('common.dash')}</dd>

                <dt>{t('tasks.ratio')}</dt>
                <dd>{task.ratio || t('common.dash')}</dd>

                <dt>{t('tasks.session')}</dt>
                <dd className="mono">{task.session_pool_id ? task.session_pool_id.slice(0, 8) : t('common.dash')}</dd>

                <dt>{t('tasks.historyId')}</dt>
                <dd className="mono">{task.history_record_id || t('common.dash')}</dd>

                <dt>{t('table.queue')}</dt>
                <dd>
                  {task.queue_position != null
                    ? `${task.queue_position}/${task.queue_total || '-'} (${task.queue_eta || '-'})`
                    : t('common.dash')}
                </dd>

                <dt>{t('tasks.createdAt')}</dt>
                <dd>{formatDateTime(task.created_at)}</dd>

                <dt>{t('tasks.startedAt')}</dt>
                <dd>{formatDateTime(task.started_at)}</dd>

                <dt>{t('tasks.finishedAt')}</dt>
                <dd>{formatDateTime(task.finished_at)}</dd>
              </dl>

              <h3 style={{ marginTop: '1rem', marginBottom: '0.4rem' }}>{t('tasks.prompt')}</h3>
              <pre className="task-prompt">{task.prompt || t('common.dash')}</pre>

              {task.video_url ? (
                <div style={{ marginTop: '1rem' }}>
                  <h3 style={{ marginBottom: '0.4rem' }}>{t('tasks.videoUrl')}</h3>
                  <div className="row">
                    <a className="task-video-link" href={task.video_url} target="_blank" rel="noreferrer">
                      {t('tasks.download')} ▶
                    </a>
                    <button onClick={copyVideoUrl}>{copied ? t('tasks.copied') : t('tasks.copyUrl')}</button>
                  </div>
                </div>
              ) : task.status === 'succeeded' ? null : (
                <p style={{ marginTop: '0.8rem', color: 'var(--muted)' }}>{t('tasks.noVideo')}</p>
              )}

              {task.error_message ? (
                <div style={{ marginTop: '1rem' }}>
                  <h3 style={{ marginBottom: '0.4rem' }}>{t('tasks.errorMessage')}</h3>
                  <pre className="task-prompt" style={{ color: 'var(--err)' }}>{task.error_message}</pre>
                  {task.error_kind ? (
                    <small style={{ color: 'var(--muted)' }}>Kind: {task.error_kind}</small>
                  ) : null}
                </div>
              ) : null}
            </>
          )}
        </div>
      </div>
    </div>
  )
}

function SessionsPage({ sessions, refresh, onAdd, onRemove, onToggle, onTest }) {
  const t = useT()
  const [label, setLabel] = useState('')
  const [sessionId, setSessionId] = useState('')

  const submit = async (event) => {
    event.preventDefault()
    if (!sessionId.trim()) return
    await onAdd({ label: label.trim(), session_id: sessionId.trim() })
    setLabel('')
    setSessionId('')
  }

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>{t('sessions.title')}</h2>
        <button onClick={() => refresh()}>{t('common.refresh')}</button>
      </div>

      <form className="card form-grid" onSubmit={submit}>
        <h3>{t('sessions.add')}</h3>
        <label>
          {t('sessions.label')}
          <input value={label} onChange={(event) => setLabel(event.target.value)} placeholder={t('sessions.labelOptional')} />
        </label>
        <label>
          {t('sessions.sessionId')}
          <input
            value={sessionId}
            onChange={(event) => setSessionId(event.target.value)}
            required
            placeholder={t('sessions.sessionIdPlaceholder')}
          />
        </label>
        <div className="row">
          <button type="submit">{t('sessions.addButton')}</button>
        </div>
      </form>

      <div className="card">
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t('sessions.label')}</th>
                <th>{t('table.status')}</th>
                <th>{t('sessions.active')}</th>
                <th>{t('sessions.successFail')}</th>
                <th>{t('common.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session) => (
                <tr key={session.id}>
                  <td>{session.label || t('common.dash')}</td>
                  <td>
                    <StatusBadge value={session.enabled ? (session.healthy ? 'healthy' : 'unhealthy') : 'disabled'} />
                  </td>
                  <td>{session.active_tasks}</td>
                  <td>{session.success_count}/{session.fail_count}</td>
                  <td>
                    <div className="row">
                      <button onClick={() => onToggle(session.id, !session.enabled)}>
                        {session.enabled ? t('sessions.disable') : t('sessions.enable')}
                      </button>
                      <button onClick={() => onTest(session.id)}>{t('sessions.test')}</button>
                      <button onClick={() => onRemove(session.id)}>{t('sessions.remove')}</button>
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

function KeysPage({
  keys,
  refresh,
  loading,
  onCreate,
  onToggle,
  onRegenerate,
  onDelete,
}) {
  const t = useT()
  const [name, setName] = useState('')
  const [rateLimit, setRateLimit] = useState('60')
  const [dailyQuota, setDailyQuota] = useState('0')
  const [scopes, setScopes] = useState('')
  const [copiedId, setCopiedId] = useState(null)

  const submit = async (event) => {
    event.preventDefault()
    if (!name.trim()) return

    const parsedRate = Number.parseInt(rateLimit, 10)
    const parsedQuota = Number.parseInt(dailyQuota, 10)
    const parsedScopes = scopes
      .split(',')
      .map((scope) => scope.trim())
      .filter(Boolean)

    const ok = await onCreate({
      name: name.trim(),
      rate_limit: Number.isFinite(parsedRate) && parsedRate > 0 ? parsedRate : 60,
      daily_quota: Number.isFinite(parsedQuota) && parsedQuota >= 0 ? parsedQuota : 0,
      scopes: parsedScopes.length > 0 ? parsedScopes : undefined,
    })

    if (!ok) return

    setName('')
    setRateLimit('60')
    setDailyQuota('0')
    setScopes('')
  }

  const copyKey = async (id, key) => {
    if (!key || !navigator?.clipboard?.writeText) return
    await navigator.clipboard.writeText(key)
    setCopiedId(id)
    setTimeout(() => setCopiedId(null), 1500)
  }

  return (
    <section className="page-grid">
      <div className="section-head">
        <h2>{t('keys.title')}</h2>
        <button onClick={() => refresh()}>{loading ? t('common.refreshing') : t('common.refresh')}</button>
      </div>

      <form className="card form-grid" onSubmit={submit}>
        <h3>{t('keys.create')}</h3>
        <label>
          {t('keys.name')}
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            required
            placeholder={t('keys.namePlaceholder')}
          />
        </label>
        <div className="keys-form-row">
          <label>
            {t('keys.rateLimit')}
            <input
              type="number"
              min="1"
              value={rateLimit}
              onChange={(event) => setRateLimit(event.target.value)}
            />
          </label>
          <label>
            {t('keys.dailyQuota')}
            <input
              type="number"
              min="0"
              value={dailyQuota}
              onChange={(event) => setDailyQuota(event.target.value)}
            />
          </label>
        </div>
        <label>
          {t('keys.scopes')}
          <input
            value={scopes}
            onChange={(event) => setScopes(event.target.value)}
            placeholder={t('keys.scopesPlaceholder')}
          />
        </label>
        <div className="row">
          <button type="submit">{t('keys.createButton')}</button>
        </div>
      </form>

      <div className="card">
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>{t('table.name')}</th>
                <th>{t('keys.fullKey')}</th>
                <th>{t('table.status')}</th>
                <th>{t('keys.rateLimit')}</th>
                <th>{t('keys.dailyQuota')}</th>
                <th>{t('table.scopes')}</th>
                <th>{t('keys.lastUsed')}</th>
                <th>{t('keys.expires')}</th>
                <th>{t('keys.created')}</th>
                <th>{t('common.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {keys.length === 0 ? (
                <tr>
                  <td colSpan={10}>{t('keys.empty')}</td>
                </tr>
              ) : null}
              {keys.map((keyItem) => (
                <tr key={keyItem.id}>
                  <td>{keyItem.name}</td>
                  <td className="mono">
                    {keyItem.raw_key || keyItem.key_prefix}
                    {keyItem.raw_key ? (
                      <button className="copy-btn" onClick={() => copyKey(keyItem.id, keyItem.raw_key)}>
                        {copiedId === keyItem.id ? t('keys.copied') : t('keys.copy')}
                      </button>
                    ) : null}
                  </td>
                  <td><StatusBadge value={keyItem.enabled ? 'enabled' : 'disabled'} /></td>
                  <td>{keyItem.rate_limit}</td>
                  <td>{keyItem.daily_quota}</td>
                  <td>{(keyItem.scopes || []).join(', ') || t('common.dash')}</td>
                  <td>{formatDateTime(keyItem.last_used_at)}</td>
                  <td>{formatDateTime(keyItem.expires_at) === '-' ? t('keys.never') : formatDateTime(keyItem.expires_at)}</td>
                  <td>{formatDateTime(keyItem.created_at)}</td>
                  <td>
                    <div className="row">
                      <button onClick={() => onToggle(keyItem.id, !keyItem.enabled)}>
                        {keyItem.enabled ? t('keys.disable') : t('keys.enable')}
                      </button>
                      <button onClick={() => onRegenerate(keyItem.id)}>{t('keys.regenerate')}</button>
                      <button onClick={() => onDelete(keyItem.id)}>{t('keys.delete')}</button>
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

function AppContent() {
  const t = useT()
  const { language, setLanguage } = useI18n()
  const [tab, setTab] = useState('dashboard')
  const [authStatus, setAuthStatus] = useState('checking')
  const [authSubmitting, setAuthSubmitting] = useState(false)
  const [authError, setAuthError] = useState('')
  const [currentUser, setCurrentUser] = useState(null)
  const [stats, setStats] = useState(null)
  const [health, setHealth] = useState(null)
  const [tasks, setTasks] = useState([])
  const [sessions, setSessions] = useState([])
  const [keys, setKeys] = useState([])
  const [statusFilter, setStatusFilter] = useState('')
  const [loadingTasks, setLoadingTasks] = useState(false)
  const [loadingKeys, setLoadingKeys] = useState(false)
  const [error, setError] = useState('')
  const [selectedTaskId, setSelectedTaskId] = useState(null)
  const errorTimerRef = useRef(null)

  const clearOperationalData = useCallback(() => {
    setStats(null)
    setHealth(null)
    setTasks([])
    setSessions([])
    setKeys([])
  }, [])

  const handleUnauthorized = useCallback(() => {
    setCurrentUser(null)
    setAuthStatus('unauthenticated')
    setAuthError(t('auth.sessionExpired'))
    setTab('dashboard')
    setError('')
    clearOperationalData()
  }, [clearOperationalData, t])

  useEffect(() => {
    setUnauthorizedHandler(handleUnauthorized)
    return () => setUnauthorizedHandler(null)
  }, [handleUnauthorized])

  useEffect(() => () => {
    if (errorTimerRef.current) {
      clearTimeout(errorTimerRef.current)
    }
  }, [])

  const pushBanner = useCallback((message) => {
    if (errorTimerRef.current) {
      clearTimeout(errorTimerRef.current)
    }
    setError(message)
    errorTimerRef.current = setTimeout(() => setError(''), 5000)
  }, [])

  const handleError = useCallback((err) => {
    if (err instanceof ApiError && err.status === 401) {
      return
    }

    pushBanner(err?.message || t('errors.requestFailed'))
  }, [pushBanner, t])

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

  const loadTasks = useCallback(async (filter = '') => {
    try {
      setLoadingTasks(true)
      const qs = new URLSearchParams({ limit: '200' })
      if (filter) {
        qs.set('status', filter)
      }
      const payload = await apiRequest(`/api/v1/tasks?${qs.toString()}`)
      setTasks(payload.tasks || [])
    } catch (err) {
      handleError(err)
    } finally {
      setLoadingTasks(false)
    }
  }, [handleError])

  const loadSessions = useCallback(async () => {
    try {
      const payload = await apiRequest('/api/v1/sessions')
      setSessions(payload.sessions || [])
    } catch (err) {
      handleError(err)
    }
  }, [handleError])

  const loadKeys = useCallback(async () => {
    try {
      setLoadingKeys(true)
      const payload = await apiRequest('/api/v1/keys')
      setKeys(payload.keys || [])
    } catch (err) {
      handleError(err)
    } finally {
      setLoadingKeys(false)
    }
  }, [handleError])

  const checkAuth = useCallback(async () => {
    setAuthStatus('checking')
    try {
      const payload = await apiRequest('/auth/me', { skipAuthRedirect: true })
      setCurrentUser(resolveCurrentUser(payload))
      setAuthError('')
      setAuthStatus('authenticated')
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setCurrentUser(null)
        setAuthStatus('unauthenticated')
        return
      }
      setCurrentUser(null)
      setAuthError(err?.message || t('auth.loginErrorDefault'))
      setAuthStatus('unauthenticated')
    }
  }, [t])

  useEffect(() => {
    checkAuth()
  }, [checkAuth])

  const login = useCallback(async ({ username, password }) => {
    setAuthSubmitting(true)
    setAuthError('')

    try {
      await apiRequest('/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username, password }),
        skipAuthRedirect: true,
      })

      const payload = await apiRequest('/auth/me', { skipAuthRedirect: true })
      setCurrentUser(resolveCurrentUser(payload) || { username })
      setAuthStatus('authenticated')
      setTab('dashboard')
      setStatusFilter('')
      return true
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setAuthError(t('auth.invalidCredentials'))
        return false
      }
      setAuthError(err?.message || t('auth.loginErrorDefault'))
      return false
    } finally {
      setAuthSubmitting(false)
    }
  }, [t])

  const logout = useCallback(async () => {
    try {
      await apiRequest('/auth/logout', { method: 'POST', skipAuthRedirect: true })
    } catch {
      // Always clear local auth state even if remote logout call fails.
    } finally {
      setCurrentUser(null)
      setAuthStatus('unauthenticated')
      setAuthError('')
      setTab('dashboard')
      clearOperationalData()
    }
  }, [clearOperationalData])

  const cancelTask = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/tasks/${id}/cancel`, { method: 'POST' })
      await loadTasks(statusFilter)
      await loadStats()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadStats, loadTasks, statusFilter])

  const retryTask = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/tasks/${id}/retry`, { method: 'POST' })
      await loadTasks(statusFilter)
      await loadStats()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadStats, loadTasks, statusFilter])

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
        throw new Error(payload.message || t('errors.sessionTestFailed'))
      }
      await loadSessions()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadSessions, t])

  const createKey = useCallback(async (body) => {
    try {
      await apiRequest('/api/v1/keys', {
        method: 'POST',
        body: JSON.stringify(body),
      })
      pushBanner(t('keys.createdNotice'))
      await loadKeys()
      return true
    } catch (err) {
      handleError(err)
      return false
    }
  }, [handleError, loadKeys, pushBanner, t])

  const toggleKey = useCallback(async (id, enabled) => {
    try {
      await apiRequest(`/api/v1/keys/${id}`, {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      })
      await loadKeys()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadKeys])

  const regenerateKey = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/keys/${id}/regenerate`, { method: 'POST' })
      pushBanner(t('keys.regeneratedNotice'))
      await loadKeys()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadKeys, pushBanner, t])

  const deleteKey = useCallback(async (id) => {
    try {
      await apiRequest(`/api/v1/keys/${id}`, { method: 'DELETE' })
      await loadKeys()
    } catch (err) {
      handleError(err)
    }
  }, [handleError, loadKeys])

  // Initial data load on auth
  useEffect(() => {
    if (authStatus !== 'authenticated') return
    loadStats()
    loadTasks(statusFilter)
    loadSessions()
    loadKeys()
  }, [authStatus]) // eslint-disable-line react-hooks/exhaustive-deps

  // Reload tasks when filter changes
  useEffect(() => {
    if (authStatus !== 'authenticated') return
    loadTasks(statusFilter)
  }, [authStatus, loadTasks, statusFilter])

  // Tab-aware polling: only poll data for the active tab
  useEffect(() => {
    if (authStatus !== 'authenticated') return

    const id = setInterval(() => {
      loadStats()
      if (tab === 'tasks' || tab === 'dashboard') loadTasks(statusFilter)
      if (tab === 'sessions') loadSessions()
      if (tab === 'keys') loadKeys()
    }, POLL_MS)
    return () => clearInterval(id)
  }, [authStatus, tab, loadKeys, loadSessions, loadStats, loadTasks, statusFilter])

  const tabs = useMemo(() => TAB_KEYS.map((key) => ({ key, label: t(`tabs.${key}`) })), [t])
  const username = currentUser?.username || currentUser?.name || t('auth.userFallback')

  if (authStatus === 'checking') {
    return (
      <div className="app-shell">
        <div className="card auth-checking">{t('auth.checking')}</div>
      </div>
    )
  }

  if (authStatus !== 'authenticated') {
    return <LoginPage onLogin={login} loading={authSubmitting} error={authError} />
  }

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="header-row">
          <div>
            <h1>{t('header.title')}</h1>
            <p>{t('header.subtitle')}</p>
          </div>
          <div className="header-actions">
            <button className="lang-switch" onClick={() => setLanguage(language === 'zh' ? 'en' : 'zh')}>
              {language === 'zh' ? 'EN' : '中'}
            </button>
            <span className="user-chip">{t('header.user', { username })}</span>
            <button onClick={logout}>{t('auth.logout')}</button>
          </div>
        </div>
      </header>

      <nav className="tab-bar">
        {tabs.map((item) => (
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
        <DashboardPage stats={stats} health={health} tasks={tasks} refresh={loadStats} onSelectTask={setSelectedTaskId} />
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
          onSelectTask={setSelectedTaskId}
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

      {tab === 'keys' ? (
        <KeysPage
          keys={keys}
          refresh={loadKeys}
          loading={loadingKeys}
          onCreate={createKey}
          onToggle={toggleKey}
          onRegenerate={regenerateKey}
          onDelete={deleteKey}
        />
      ) : null}

      <TaskDetailModal taskId={selectedTaskId} onClose={() => setSelectedTaskId(null)} />
    </div>
  )
}

export default function App() {
  return (
    <I18nProvider>
      <AppContent />
    </I18nProvider>
  )
}
