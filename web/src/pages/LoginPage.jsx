import { useState } from 'react'
import { useI18n, useT } from '../lib/i18n'

export default function LoginPage({ onLogin, loading, error }) {
  const t = useT()
  const { language, setLanguage } = useI18n()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')

  const submit = async (event) => {
    event.preventDefault()
    if (!username.trim() || !password) return
    await onLogin({ username: username.trim(), password })
  }

  return (
    <div className="app-shell login-shell">
      <header className="app-header login-header">
        <div className="header-row">
          <div>
            <h1>{t('header.title')}</h1>
            <p>{t('auth.loginIntro')}</p>
          </div>
          <button className="lang-switch" onClick={() => setLanguage(language === 'zh' ? 'en' : 'zh')}>
            {language === 'zh' ? 'EN' : '中'}
          </button>
        </div>
      </header>

      <section className="login-stage">
        <form className="card login-card form-grid" onSubmit={submit}>
          <h2>{t('auth.loginTitle')}</h2>
          <p className="login-subtitle">{t('auth.loginSubtitle')}</p>

          {error ? <div className="error-banner login-error">{error}</div> : null}

          <label>
            {t('auth.username')}
            <input
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="username"
              required
            />
          </label>

          <label>
            {t('auth.password')}
            <input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="current-password"
              required
            />
          </label>

          <div className="row">
            <button type="submit" disabled={loading || !username.trim() || !password}>
              {loading ? t('auth.loggingIn') : t('auth.login')}
            </button>
          </div>
        </form>
      </section>
    </div>
  )
}
