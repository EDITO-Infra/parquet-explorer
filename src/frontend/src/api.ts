import { RecordBatch, RecordBatchReader } from 'apache-arrow'
import type { DatasetInfo, PageRequest, SpatialRequest } from './types'

const API_BASE = (import.meta.env.VITE_API_URL ?? '').replace(/\/$/, '')
const V1 = `${API_BASE}/api/v1`

async function expectOk(response: Response): Promise<Response> {
  if (response.ok) return response
  const contentType = response.headers.get('content-type') ?? ''
  if (contentType.includes('application/json')) {
    const body = await response.json().catch(() => ({})) as { error?: string }
    throw new Error(body.error ?? `Request failed (${response.status})`)
  }
  const text = await response.text()
  throw new Error(text || `Request failed (${response.status})`)
}

export async function openDataset(uri: string, name?: string): Promise<DatasetInfo> {
  const response = await expectOk(await fetch(`${V1}/datasets/open`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ uri, name: name || undefined }),
  }))
  return response.json() as Promise<DatasetInfo>
}

export async function getMetadata(datasetId: string): Promise<DatasetInfo> {
  const response = await expectOk(await fetch(`${V1}/datasets/${encodeURIComponent(datasetId)}/metadata`))
  return response.json() as Promise<DatasetInfo>
}

export async function closeDataset(datasetId: string): Promise<void> {
  await expectOk(await fetch(`${V1}/datasets/${encodeURIComponent(datasetId)}`, { method: 'DELETE' }))
}

export async function* pageBatches(datasetId: string, request: PageRequest): AsyncGenerator<RecordBatch> {
  yield* arrowRequest(`${V1}/datasets/${encodeURIComponent(datasetId)}/page`, request)
}

export async function* spatialBatches(datasetId: string, request: SpatialRequest): AsyncGenerator<RecordBatch> {
  yield* arrowRequest(`${V1}/datasets/${encodeURIComponent(datasetId)}/spatial`, request)
}

async function* arrowRequest(
  url: string,
  payload: unknown,
): AsyncGenerator<RecordBatch> {
  const response = await expectOk(await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(payload),
  }))

  if (!response.body) {
    throw new Error('Server returned an empty Arrow response')
  }

  const reader = await RecordBatchReader.from(streamBytes(response.body))

  for await (const batch of reader) {
    yield batch
  }
}

async function* streamBytes(
  stream: ReadableStream<Uint8Array>,
): AsyncGenerator<Uint8Array> {
  const reader = stream.getReader()

  try {
    while (true) {
      const { value, done } = await reader.read()

      if (done) return
      if (value) yield value
    }
  } finally {
    reader.releaseLock()
  }
}

export function exportUrl(datasetId: string): string {
  return `${V1}/datasets/${encodeURIComponent(datasetId)}/export`
}

export async function downloadSubset(datasetId: string, format: 'parquet' | 'arrow', columns?: string[]): Promise<void> {
  const response = await expectOk(await fetch(exportUrl(datasetId), {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ format, columns }),
  }))
  const blob = await response.blob()
  const disposition = response.headers.get('content-disposition') ?? ''
  const filename = disposition.match(/filename="?([^";]+)"?/)?.[1] ?? `subset.${format === 'parquet' ? 'parquet' : 'arrow'}`
  const href = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = href
  anchor.download = filename
  anchor.click()
  URL.revokeObjectURL(href)
}
