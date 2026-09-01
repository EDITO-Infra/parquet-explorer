/** URL entry control for opening a new remote Parquet dataset. */
import type { FormEvent } from 'react'
export function OpenFileBar({
  uri,
  opening,
  onUriChange,
  onOpen,
}: {
  uri: string
  opening: boolean
  onUriChange: (uri: string) => void
  onOpen: (event: FormEvent) => void
}) {
  return (
    <form className="open-bar" onSubmit={onOpen}>
      <div className="open-input-wrap">
        <span className="open-input-label">Parquet URL</span>
        <input
          value={uri}
          onChange={event => onUriChange(event.target.value)}
          placeholder="https://example.com/data.parquet"
          aria-label="Parquet URL"
        />
      </div>
      <button disabled={opening || !uri.trim()}>
        {opening ? 'Opening…' : 'Open file'}
      </button>
    </form>
  )
}
