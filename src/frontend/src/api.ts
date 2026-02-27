import type { DatasetRef, GeoEligibility, QueryResponse, SchemaResponse } from './types'

const API_BASE = import.meta.env.VITE_API_URL ?? "";

async function parseJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const body = await response.text()
    throw new Error(body || `Request failed with ${response.status}`)
  }
  return response.json() as Promise<T>
}

export async function listDatasets(): Promise<DatasetRef[]> {
  return parseJson(await fetch(`${API_BASE}/api/datasets`))
}

export async function registerDataset(id: string, uri: string): Promise<DatasetRef> {
  return parseJson(
    await fetch(`${API_BASE}/api/datasets`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, uri })
    })
  )
}

export async function getSchema(dataset: string): Promise<SchemaResponse> {
  return parseJson(await fetch(`${API_BASE}/api/schema?dataset=${encodeURIComponent(dataset)}`))
}

export async function getPreview(dataset: string, limit = 100): Promise<QueryResponse> {
  return parseJson(
    await fetch(`${API_BASE}/api/preview?dataset=${encodeURIComponent(dataset)}&limit=${encodeURIComponent(limit)}`)
  )
}

export async function runQuery(dataset: string, sql: string, limit = 1000): Promise<QueryResponse> {
  return parseJson(
    await fetch(`${API_BASE}/api/query?dataset=${encodeURIComponent(dataset)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sql, limit, offset: 0 })
    })
  )
}

export async function getGeoEligibility(dataset: string, geomColumn = 'geom'): Promise<GeoEligibility> {
  return parseJson(
    await fetch(
      `${API_BASE}/api/geo/eligible?dataset=${encodeURIComponent(dataset)}&geom_column=${encodeURIComponent(geomColumn)}`
    )
  )
}
