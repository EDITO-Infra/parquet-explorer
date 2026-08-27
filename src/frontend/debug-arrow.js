import { tableFromIPC } from 'apache-arrow'

const base = 'http://localhost:8080/api/v1'
const id = 'd5d07e79-624f-46c6-9042-966ff817b71d'

const response = await fetch(`${base}/datasets/${id}/page`, {
  method: 'POST',
  headers: {
    'content-type': 'application/json'
  },
  body: JSON.stringify({
    offset: 0,
    limit: 3,
    columns: null,
    filters: [],
    sort: []
  })
})

const buffer = await response.arrayBuffer()
const table = tableFromIPC(new Uint8Array(buffer))

console.log('FIELDS')
console.log(table.schema.fields.map((field, i) => [i, field.name, String(field.type)]))

console.log('\nFIRST ROW')
for (let i = 0; i < table.numCols; i++) {
  const field = table.schema.fields[i]
  console.log(i, field.name, table.getChildAt(i)?.get(0))
}
