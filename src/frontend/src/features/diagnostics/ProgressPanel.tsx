/**
 * User-facing progress plus expandable technical diagnostics.
 *
 * The panel combines backend trace events with browser-measured activity. A
 * milestone timestamp ("@ 64 s") and a measured duration ("64 s") are different
 * concepts; this component must preserve that distinction when rendering them.
 */
import type { TraceSnapshot } from '../../types'

export interface ActivityEvent {
  message: string
  detail?: string
  elapsedMs: number
  durationMs?: number
  source: 'Browser' | 'Backend'
}

export interface ActivityState {
  traceId: string
  title: string
  current: string
  status: 'running' | 'complete' | 'error'
  progress?: number
  startedAt: number
  elapsedMs: number
  trace?: TraceSnapshot
  clientEvents: ActivityEvent[]
}

export function ProgressPanel({ activity }: { activity: ActivityState | null }) {
  if (!activity) return null

  const serverEvents: ActivityEvent[] = activity.trace?.events.map(event => ({
    message: event.message,
    detail: event.detail ?? undefined,
    elapsedMs: event.elapsed_ms,
    durationMs: event.duration_ms ?? undefined,
    source: 'Backend',
  })) ?? []

  const width = activity.progress == null
    ? undefined
    : `${Math.max(0, Math.min(1, activity.progress)) * 100}%`

  return (
    <section className={`activity-panel activity-${activity.status}`} aria-live="polite">
      <div className="activity-main">
        <div className="activity-copy">
          <span className="activity-status-dot" aria-hidden="true" />
          <div>
            <strong>{activity.title}</strong>
            <span>{activity.status === 'running' ? 'Working' : activity.status === 'complete' ? 'Complete' : 'Needs attention'}</span>
          </div>
        </div>
        <span className="activity-time">{formatDuration(activity.elapsedMs)}</span>
      </div>

      <div className={`activity-track${activity.progress == null && activity.status === 'running' ? ' indeterminate' : ''}`}>
        <div className="activity-fill" style={width ? { width } : undefined} />
        <span className="activity-track-label">{activity.current}</span>
      </div>

      <details className="activity-details">
        <summary>Query details &amp; timings</summary>
        <div className="activity-timeline-wrap">
          <Timeline title="Backend" events={serverEvents} />
          <Timeline title="Browser" events={activity.clientEvents} />
        </div>
      </details>
    </section>
  )
}

function Timeline({ title, events }: { title: string; events: ActivityEvent[] }) {
  return (
    <div className="activity-timeline-group">
      <div className="activity-timeline-title">{title}</div>
      <div className="activity-timeline">
        {events.map((event, index) => {
          const hasMeasuredDuration = event.durationMs != null
          const timing = hasMeasuredDuration
            ? formatDuration(event.durationMs ?? 0)
            : `@ ${formatDuration(event.elapsedMs)}`
          const timingTitle = hasMeasuredDuration
            ? `${event.durationMs} ms measured stage duration · recorded at ${event.elapsedMs} ms`
            : `Milestone recorded ${event.elapsedMs} ms from ${title.toLowerCase()} start; not a stage duration`
          return (
            <div className="activity-event" key={`${event.elapsedMs}-${index}`}>
              <span className="activity-step-number">{index + 1}</span>
              <div>
                <strong>{event.message}</strong>
                {event.detail && <span>{event.detail}</span>}
              </div>
              <span className={`activity-event-time${hasMeasuredDuration ? ' measured' : ' milestone'}`} title={timingTitle}>
                {timing}
              </span>
            </div>
          )
        })}
        {events.length === 0 && <div className="activity-empty">Waiting for {title.toLowerCase()} diagnostics…</div>}
      </div>
    </div>
  )
}

function formatDuration(ms: number) {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`
  if (ms < 10_000) return `${(ms / 1000).toFixed(2)} s`
  return `${(ms / 1000).toFixed(1)} s`
}
