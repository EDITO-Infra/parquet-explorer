/**
 * Lifecycle hook for one visible backend/browser activity trace.
 *
 * App and feature components report high-level browser stages through the
 * methods returned here. The hook also polls the backend trace endpoint and
 * merges those server events into the same ActivityState consumed by
 * ProgressPanel.
 *
 * Keeping this orchestration here prevents every feature from implementing its
 * own polling/timer logic and gives future Analysis requests the same
 * diagnostics behavior as table/map requests.
 */
import { useEffect, useRef, useState } from 'react'
import { getTrace } from '../../lib/api'
import type { ActivityState } from './ProgressPanel'

export function useActivityTrace() {
  const [activity, setActivity] = useState<ActivityState | null>(null)
  const activeTraceId = useRef<string | null>(null)

  // The elapsed clock is browser-local and updates independently of backend
  // polling so the progress panel remains visibly alive during long I/O waits.
  useEffect(() => {
    if (!activity || activity.status !== 'running') return
    const timer = window.setInterval(() => {
      setActivity(current => current?.status === 'running'
        ? { ...current, elapsedMs: performance.now() - current.startedAt }
        : current)
    }, 100)
    return () => window.clearInterval(timer)
  }, [activity?.traceId, activity?.status])

  function beginActivity(traceId: string, title: string, current: string) {
    activeTraceId.current = traceId
    const startedAt = performance.now()
    setActivity({
      traceId,
      title,
      current,
      status: 'running',
      startedAt,
      elapsedMs: 0,
      clientEvents: [],
    })
    void monitorTrace(traceId)
  }

  function addClientActivity(
    traceId: string,
    message: string,
    detail?: string,
    progress?: number,
    durationMs?: number,
  ) {
    setActivity(current => {
      if (!current || current.traceId !== traceId) return current
      const elapsedMs = performance.now() - current.startedAt
      return {
        ...current,
        current: message,
        progress: progress ?? current.progress,
        elapsedMs,
        clientEvents: [...current.clientEvents, {
          message,
          detail,
          elapsedMs,
          durationMs,
          source: 'Browser',
        }],
      }
    })
  }

  function finishActivity(traceId: string, message: string, detail?: string) {
    setActivity(current => {
      if (!current || current.traceId !== traceId) return current
      const elapsedMs = performance.now() - current.startedAt
      return {
        ...current,
        current: message,
        status: 'complete',
        progress: 1,
        elapsedMs,
        clientEvents: [...current.clientEvents, {
          message,
          detail,
          elapsedMs,
          source: 'Browser',
        }],
      }
    })
  }

  function failActivity(traceId: string, message: string) {
    setActivity(current => {
      if (!current || current.traceId !== traceId) return current
      return {
        ...current,
        current: message,
        status: 'error',
        elapsedMs: performance.now() - current.startedAt,
      }
    })
  }

  async function monitorTrace(traceId: string) {
    let seen = false

    // Polling is deliberately best-effort. Diagnostics must never cause the
    // underlying query to fail. The long upper bound supports slow remote reads.
    for (let attempt = 0; attempt < 900; attempt += 1) {
      if (activeTraceId.current !== traceId) return
      try {
        const trace = await getTrace(traceId)
        if (trace) {
          seen = true
          setActivity(current => {
            if (!current || current.traceId !== traceId) return current
            const last = trace.events.at(-1)
            return {
              ...current,
              trace,
              current: trace.status === 'running' && last ? last.message : current.current,
              elapsedMs: Math.max(current.elapsedMs, trace.elapsed_ms),
            }
          })
          if (trace.status !== 'running') return
        }
      } catch {
        // The trace endpoint is observational only; ignore transient failures.
      }

      // If a trace ID was not registered quickly, stop polling rather than
      // creating background traffic for a request that has no backend trace.
      if (!seen && attempt >= 40) return
      await delay(120)
    }
  }

  return {
    activity,
    beginActivity,
    addClientActivity,
    finishActivity,
    failActivity,
  }
}

function delay(ms: number) {
  return new Promise(resolve => window.setTimeout(resolve, ms))
}
