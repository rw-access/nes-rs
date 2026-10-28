const DATABASE_NAME = "nes-rs-rom-cache";
const DATABASE_VERSION = 1;
const STORE_NAME = "roms";
const CACHE_KEY = "latest";
export const MAX_ROM_BYTES = 64 * 1024 * 1024;

function failure(reason) {
  return { ok: false, reason: String(reason || "ROM cache operation failed") };
}

function validateName(name) {
  if (typeof name !== "string" || !name.trim() || name.length > 255) {
    throw new TypeError("ROM name must be a nonempty string of at most 255 characters");
  }
}

// Take ownership synchronously, before openDatabase() or any other await. This
// matters when the caller passes a view over a reusable WASM memory buffer.
function copyBytes(bytes) {
  let view;
  if (bytes instanceof ArrayBuffer) {
    view = new Uint8Array(bytes);
  } else if (bytes instanceof Uint8Array) {
    view = bytes;
  } else {
    throw new TypeError("ROM bytes must be an ArrayBuffer or Uint8Array");
  }
  if (view.byteLength > MAX_ROM_BYTES) {
    throw new RangeError(`ROM exceeds the ${MAX_ROM_BYTES / (1024 * 1024)} MiB cache limit`);
  }
  return view.slice();
}

function errorReason(error) {
  return error?.message || error?.name || "ROM cache operation failed";
}

function openDatabase(indexedDB) {
  return new Promise((resolve, reject) => {
    let request;
    try {
      request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION);
    } catch (error) {
      reject(error);
      return;
    }
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) database.createObjectStore(STORE_NAME);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("Could not open ROM cache"));
    request.onblocked = () => reject(new Error("ROM cache upgrade is blocked by another tab"));
  });
}

function transactionDone(transaction) {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error || new Error("ROM cache transaction failed"));
    transaction.onabort = () => reject(transaction.error || new Error("ROM cache transaction aborted"));
  });
}

function requestResult(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("ROM cache request failed"));
  });
}

function closeDatabase(database) {
  try { database?.close(); } catch (_) { /* best effort */ }
}

function storedBytes(value) {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (value instanceof Uint8Array) return value;
  return null;
}

/** Return { ok: true, name, bytes, savedAt } or { ok: false, reason }. */
export async function loadCachedRom({ indexedDB = globalThis.indexedDB } = {}) {
  if (!indexedDB || typeof indexedDB.open !== "function") return failure("IndexedDB is unavailable");
  let database;
  try {
    database = await openDatabase(indexedDB);
    const transaction = database.transaction(STORE_NAME, "readonly");
    const done = transactionDone(transaction);
    const recordPromise = requestResult(transaction.objectStore(STORE_NAME).get(CACHE_KEY));
    const [record] = await Promise.all([recordPromise, done]);
    if (record == null) return { ok: true, name: null, bytes: null, savedAt: null };

    const bytes = storedBytes(record.bytes);
    if (typeof record.name !== "string" || !record.name.trim() || record.name.length > 255
      || !bytes || bytes.byteLength === 0 || bytes.byteLength > MAX_ROM_BYTES
      || !Number.isFinite(record.savedAt)) {
      return failure("Cached ROM record is invalid");
    }
    return { ok: true, name: record.name, bytes: bytes.slice(), savedAt: record.savedAt };
  } catch (error) {
    return failure(errorReason(error));
  } finally {
    closeDatabase(database);
  }
}

/** Atomically replace the single cached ROM after the caller loads it successfully. */
export async function saveCachedRom(name, bytes, { indexedDB = globalThis.indexedDB, now = Date.now } = {}) {
  let ownedBytes;
  try {
    validateName(name);
    ownedBytes = copyBytes(bytes);
    if (ownedBytes.byteLength === 0) throw new RangeError("ROM bytes cannot be empty");
  } catch (error) {
    return failure(errorReason(error));
  }
  if (!indexedDB || typeof indexedDB.open !== "function") return failure("IndexedDB is unavailable");

  let database;
  try {
    database = await openDatabase(indexedDB);
    const transaction = database.transaction(STORE_NAME, "readwrite");
    const done = transactionDone(transaction);
    const savedAt = now();
    if (!Number.isFinite(savedAt)) throw new TypeError("ROM cache timestamp must be finite");
    const record = { name, bytes: ownedBytes.buffer, savedAt };
    const request = transaction.objectStore(STORE_NAME).put(record, CACHE_KEY);
    await Promise.all([requestResult(request), done]);
    return { ok: true, savedAt };
  } catch (error) {
    return failure(errorReason(error));
  } finally {
    closeDatabase(database);
  }
}

/** Clear the single cached entry. */
export async function clearCachedRom({ indexedDB = globalThis.indexedDB } = {}) {
  if (!indexedDB || typeof indexedDB.open !== "function") return failure("IndexedDB is unavailable");
  let database;
  try {
    database = await openDatabase(indexedDB);
    const transaction = database.transaction(STORE_NAME, "readwrite");
    const done = transactionDone(transaction);
    const request = transaction.objectStore(STORE_NAME).delete(CACHE_KEY);
    await Promise.all([requestResult(request), done]);
    return { ok: true };
  } catch (error) {
    return failure(errorReason(error));
  } finally {
    closeDatabase(database);
  }
}
