// The last simulation batch, kept in the browser so it survives a reload or
// typing another page's address (each is a fresh page load, which empties
// fireStore). IndexedDB rather than localStorage: a batch is tens of MB.

import type { SimulateNResponse } from './types'

const DATABASE = 'tflop'
const STORE = 'simulations'
const KEY = 'latest-batch'

function available(): boolean {
  return typeof indexedDB !== 'undefined'
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1)
    request.onupgradeneeded = () => request.result.createObjectStore(STORE)
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

async function withStore<T>(
  mode: IDBTransactionMode,
  use: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const database = await openDatabase()
  try {
    return await new Promise<T>((resolve, reject) => {
      const transaction = database.transaction(STORE, mode)
      const request = use(transaction.objectStore(STORE))
      transaction.oncomplete = () => resolve(request.result)
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
    })
  } finally {
    database.close()
  }
}

/** Keeps `batch` as the one to restore. Failures (e.g. no space) are only logged. */
export async function saveBatch(batch: SimulateNResponse): Promise<void> {
  if (!available()) return
  try {
    await withStore('readwrite', (store) => store.put(batch, KEY))
  } catch (error) {
    console.warn("Couldn't keep the simulation for the next page load.", error)
  }
}

/** The last saved batch, or null if there is none or it can't be read. */
export async function loadBatch(): Promise<SimulateNResponse | null> {
  if (!available()) return null
  try {
    return ((await withStore('readonly', (store) => store.get(KEY))) as SimulateNResponse | undefined) ?? null
  } catch (error) {
    console.warn("Couldn't read the saved simulation.", error)
    return null
  }
}

export async function forgetBatch(): Promise<void> {
  if (!available()) return
  try {
    await withStore('readwrite', (store) => store.delete(KEY))
  } catch {
    // Nothing to restore either way.
  }
}
