let unauthorizedHandler = null

export class ApiError extends Error {
  constructor(message, status, payload) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.payload = payload
  }
}

export function setUnauthorizedHandler(handler) {
  unauthorizedHandler = typeof handler === 'function' ? handler : null
}

function parsePayload(text) {
  if (!text) return null

  try {
    return JSON.parse(text)
  } catch {
    return { message: text }
  }
}

export async function apiRequest(path, options = {}) {
  const { skipAuthRedirect = false, ...fetchOptions } = options
  const headers = { ...(fetchOptions.headers || {}) }

  if (fetchOptions.body && !headers['Content-Type']) {
    headers['Content-Type'] = 'application/json'
  }

  const response = await fetch(path, {
    credentials: 'include',
    ...fetchOptions,
    headers,
  })

  const text = await response.text()
  const payload = parsePayload(text)

  if (!response.ok) {
    const message =
      payload?.error ||
      payload?.message ||
      `${response.status} ${response.statusText}`

    const error = new ApiError(message, response.status, payload)
    if (response.status === 401 && !skipAuthRedirect && unauthorizedHandler) {
      unauthorizedHandler()
    }
    throw error
  }

  return payload
}
