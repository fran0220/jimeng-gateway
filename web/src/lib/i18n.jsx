import { createContext, useCallback, useContext, useMemo, useState } from 'react'

const STORAGE_KEY = 'jimeng-gateway:lang'

const dictionaries = {
  en: {
    'common.refresh': 'Refresh',
    'common.refreshing': 'Refreshing...',
    'common.actions': 'Actions',
    'common.model': 'Model',
    'common.error': 'Error',
    'common.lines': '{count} lines',
    'common.na': 'n/a',
    'common.dash': '-',

    'tabs.dashboard': 'Dashboard',
    'tabs.tasks': 'Tasks',
    'tabs.sessions': 'Sessions',
    'tabs.keys': 'API Keys',

    'header.title': 'Jimeng Gateway',
    'header.subtitle': 'Session rotation, async queue, and live operational telemetry',
    'header.user': 'User: {username}',

    'auth.loginIntro': 'Secure admin access',
    'auth.loginTitle': 'Sign In',
    'auth.loginSubtitle': 'Use your admin credentials to continue.',
    'auth.username': 'Username',
    'auth.password': 'Password',
    'auth.login': 'Log In',
    'auth.loggingIn': 'Logging in...',
    'auth.logout': 'Log Out',
    'auth.invalidCredentials': 'Invalid username or password.',
    'auth.loginErrorDefault': 'Unable to sign in. Please try again.',
    'auth.sessionExpired': 'Session expired. Please sign in again.',
    'auth.checking': 'Checking session...',
    'auth.userFallback': 'admin',

    'dashboard.gatewayHealth': 'Gateway Health',
    'dashboard.total': 'Total',
    'dashboard.running': 'Running',
    'dashboard.succeeded': 'Succeeded',
    'dashboard.failed': 'Failed',
    'dashboard.queuedHint': '{count} queued',
    'dashboard.runtime': 'Runtime',
    'dashboard.gatewayVersion': 'Gateway Version',
    'dashboard.sessions': 'Sessions',
    'dashboard.uptime': 'System Status',
    'dashboard.recentTasks': 'Recent Tasks',
    'dashboard.items': '{count} items',

    'tasks.title': 'Task Queue',
    'tasks.filterAll': 'All',
    'tasks.eta': 'ETA',
    'tasks.cancel': 'Cancel',
    'tasks.retry': 'Retry',
    'tasks.details': 'Task Details',
    'tasks.close': 'Close',
    'tasks.download': 'Download',
    'tasks.copyUrl': 'Copy URL',
    'tasks.copied': 'Copied',
    'tasks.noVideo': 'No video yet',
    'tasks.prompt': 'Prompt',
    'tasks.videoUrl': 'Video URL',
    'tasks.duration': 'Duration',
    'tasks.ratio': 'Aspect Ratio',
    'tasks.model': 'Model',
    'tasks.createdAt': 'Created',
    'tasks.startedAt': 'Started',
    'tasks.finishedAt': 'Finished',
    'tasks.errorMessage': 'Error Detail',
    'tasks.session': 'Session',
    'tasks.historyId': 'History Record ID',

    'sessions.title': 'Session Pool',
    'sessions.add': 'Add Session',
    'sessions.label': 'Label',
    'sessions.labelOptional': 'optional',
    'sessions.sessionId': 'Session ID',
    'sessions.sessionIdPlaceholder': 'paste jimeng sessionid',
    'sessions.active': 'Active',
    'sessions.successFail': 'Success/Fail',
    'sessions.enable': 'Enable',
    'sessions.disable': 'Disable',
    'sessions.test': 'Test',
    'sessions.remove': 'Remove',
    'sessions.addButton': 'Add',

    'keys.title': 'API Key Management',
    'keys.create': 'Create API Key',
    'keys.createButton': 'Generate Key',
    'keys.name': 'Name',
    'keys.namePlaceholder': 'e.g. automation-client',
    'keys.rateLimit': 'Rate Limit (/min)',
    'keys.dailyQuota': 'Daily Quota (0 = unlimited)',
    'keys.scopes': 'Scopes (comma separated)',
    'keys.scopesPlaceholder': 'video:create,task:read,task:cancel',
    'keys.fullKey': 'API Key',
    'keys.prefix': 'Prefix',
    'keys.lastUsed': 'Last Used',
    'keys.created': 'Created',
    'keys.expires': 'Expires',
    'keys.never': 'Never',
    'keys.regenerate': 'Regenerate',
    'keys.delete': 'Delete',
    'keys.copy': 'Copy',
    'keys.copied': 'Copied',
    'keys.enable': 'Enable',
    'keys.disable': 'Disable',
    'keys.createdNotice': 'API key created successfully.',
    'keys.regeneratedNotice': 'API key regenerated successfully.',
    'keys.empty': 'No API keys yet.',

    'table.id': 'ID',
    'table.status': 'Status',
    'table.prompt': 'Prompt',
    'table.queue': 'Queue',
    'table.updated': 'Updated',
    'table.name': 'Name',
    'table.scopes': 'Scopes',

    'status.healthy': 'healthy',
    'status.degraded': 'degraded',
    'status.unknown': 'unknown',
    'status.queued': 'queued',
    'status.submitting': 'submitting',
    'status.polling': 'polling',
    'status.downloading': 'downloading',
    'status.succeeded': 'succeeded',
    'status.failed': 'failed',
    'status.cancelled': 'cancelled',
    'status.enabled': 'enabled',
    'status.disabled': 'disabled',
    'status.unhealthy': 'unhealthy',
    'status.auth': 'auth',
    'status.timeout': 'timeout',

    'errors.requestFailed': 'Request failed.',
    'errors.sessionTestFailed': 'Session test failed',
  },
  zh: {
    'common.refresh': '刷新',
    'common.refreshing': '刷新中...',
    'common.actions': '操作',
    'common.model': '模型',
    'common.error': '错误',
    'common.lines': '{count} 行',
    'common.na': '暂无',
    'common.dash': '-',

    'tabs.dashboard': '仪表盘',
    'tabs.tasks': '任务',
    'tabs.sessions': '会话',
    'tabs.keys': 'API 密钥',

    'header.title': 'Jimeng Gateway',
    'header.subtitle': '会话轮转、异步队列与实时运维遥测',
    'header.user': '用户：{username}',

    'auth.loginIntro': '管理员安全登录',
    'auth.loginTitle': '登录',
    'auth.loginSubtitle': '请输入管理员账号与密码继续使用。',
    'auth.username': '用户名',
    'auth.password': '密码',
    'auth.login': '登录',
    'auth.loggingIn': '登录中...',
    'auth.logout': '退出登录',
    'auth.invalidCredentials': '用户名或密码错误。',
    'auth.loginErrorDefault': '登录失败，请稍后重试。',
    'auth.sessionExpired': '会话已过期，请重新登录。',
    'auth.checking': '正在检查登录状态...',
    'auth.userFallback': '管理员',

    'dashboard.gatewayHealth': '网关状态',
    'dashboard.total': '总任务数',
    'dashboard.running': '运行中',
    'dashboard.succeeded': '成功',
    'dashboard.failed': '失败',
    'dashboard.queuedHint': '{count} 个排队中',
    'dashboard.runtime': '运行时',
    'dashboard.gatewayVersion': '网关版本',
    'dashboard.sessions': '会话池',
    'dashboard.uptime': '系统状态',
    'dashboard.recentTasks': '最近任务',
    'dashboard.items': '{count} 条',

    'tasks.title': '任务队列',
    'tasks.filterAll': '全部',
    'tasks.eta': '预计耗时',
    'tasks.cancel': '取消',
    'tasks.retry': '重试',
    'tasks.details': '任务详情',
    'tasks.close': '关闭',
    'tasks.download': '下载',
    'tasks.copyUrl': '复制链接',
    'tasks.copied': '已复制',
    'tasks.noVideo': '暂无视频',
    'tasks.prompt': '提示词',
    'tasks.videoUrl': '视频地址',
    'tasks.duration': '时长',
    'tasks.ratio': '宽高比',
    'tasks.model': '模型',
    'tasks.createdAt': '创建时间',
    'tasks.startedAt': '开始时间',
    'tasks.finishedAt': '完成时间',
    'tasks.errorMessage': '错误详情',
    'tasks.session': '会话',
    'tasks.historyId': '历史记录 ID',

    'sessions.title': '会话池',
    'sessions.add': '添加会话',
    'sessions.label': '标签',
    'sessions.labelOptional': '可选',
    'sessions.sessionId': '会话 ID',
    'sessions.sessionIdPlaceholder': '粘贴 jimeng sessionid',
    'sessions.active': '活跃任务',
    'sessions.successFail': '成功/失败',
    'sessions.enable': '启用',
    'sessions.disable': '禁用',
    'sessions.test': '测试',
    'sessions.remove': '移除',
    'sessions.addButton': '添加',

    'keys.title': 'API 密钥管理',
    'keys.create': '创建 API 密钥',
    'keys.createButton': '生成密钥',
    'keys.name': '名称',
    'keys.namePlaceholder': '例如 automation-client',
    'keys.rateLimit': '速率限制（每分钟）',
    'keys.dailyQuota': '每日配额（0 表示不限）',
    'keys.scopes': '权限范围（逗号分隔）',
    'keys.scopesPlaceholder': 'video:create,task:read,task:cancel',
    'keys.fullKey': 'API 密钥',
    'keys.prefix': '前缀',
    'keys.lastUsed': '最后使用',
    'keys.created': '创建时间',
    'keys.expires': '过期时间',
    'keys.never': '永不过期',
    'keys.regenerate': '重置密钥',
    'keys.delete': '删除',
    'keys.copy': '复制',
    'keys.copied': '已复制',
    'keys.enable': '启用',
    'keys.disable': '禁用',
    'keys.createdNotice': 'API 密钥创建成功。',
    'keys.regeneratedNotice': 'API 密钥重置成功。',
    'keys.empty': '暂无 API 密钥。',

    'table.id': 'ID',
    'table.status': '状态',
    'table.prompt': '提示词',
    'table.queue': '队列',
    'table.updated': '更新时间',
    'table.name': '名称',
    'table.scopes': '权限范围',

    'status.healthy': '健康',
    'status.degraded': '降级',
    'status.unknown': '未知',
    'status.queued': '排队中',
    'status.submitting': '提交中',
    'status.polling': '轮询中',
    'status.downloading': '下载中',
    'status.succeeded': '成功',
    'status.failed': '失败',
    'status.cancelled': '已取消',
    'status.enabled': '启用',
    'status.disabled': '已禁用',
    'status.unhealthy': '不健康',
    'status.auth': '认证错误',
    'status.timeout': '超时',

    'errors.requestFailed': '请求失败。',
    'errors.sessionTestFailed': '会话测试失败',
  },
}

function interpolate(template, params = {}) {
  return template.replace(/\{(\w+)\}/g, (_, key) => String(params[key] ?? `{${key}}`))
}

function detectInitialLanguage() {
  if (typeof window === 'undefined') return 'en'

  const stored = window.localStorage.getItem(STORAGE_KEY)
  if (stored === 'en' || stored === 'zh') {
    return stored
  }

  return window.navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en'
}

const I18nContext = createContext(null)

export function I18nProvider({ children }) {
  const [language, setLanguageState] = useState(detectInitialLanguage)

  const setLanguage = useCallback((nextLanguage) => {
    const normalized = nextLanguage === 'zh' ? 'zh' : 'en'
    setLanguageState(normalized)

    if (typeof window !== 'undefined') {
      window.localStorage.setItem(STORAGE_KEY, normalized)
    }
  }, [])

  const t = useCallback((key, params) => {
    const dict = dictionaries[language] || dictionaries.en
    const fallback = dictionaries.en
    const template = dict[key] || fallback[key] || key
    return interpolate(template, params)
  }, [language])

  const value = useMemo(() => ({ language, setLanguage, t }), [language, setLanguage, t])

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

export function useI18n() {
  const context = useContext(I18nContext)
  if (!context) {
    throw new Error('useI18n must be used inside I18nProvider')
  }
  return context
}

export function useT() {
  return useI18n().t
}
