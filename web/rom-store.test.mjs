import test from "node:test";
import assert from "node:assert/strict";
import { clearCachedRom, loadCachedRom, MAX_ROM_BYTES, saveCachedRom } from "./rom-store.js";

function clone(value) {
  return structuredClone(value);
}

class MockTransaction {
  constructor(database, mode) {
    this.database = database;
    this.mode = mode;
    this.error = null;
    this.pending = null;
  }

  objectStore() {
    return {
      get: (key) => this._request(() => this.database.records.get(key), null),
      put: (value, key) => this._request(() => value, { type: "put", key, value: clone(value) }),
      delete: (key) => this._request(() => undefined, { type: "delete", key }),
    };
  }

  _request(getResult, change) {
    const request = { result: undefined, error: null };
    this.pending = change;
    queueMicrotask(() => {
      const failure = this.database.nextFailure;
      this.database.nextFailure = null;
      if (failure) {
        this.error = new Error(failure);
        request.error = this.error;
        request.onerror?.();
        this.onerror?.();
        this.onabort?.();
        return;
      }
      const value = getResult();
      request.result = value === undefined ? undefined : clone(value);
      request.onsuccess?.();
      if (this.pending?.type === "put") this.database.records.set(this.pending.key, this.pending.value);
      if (this.pending?.type === "delete") this.database.records.delete(this.pending.key);
      this.oncomplete?.();
    });
    return request;
  }
}

class MockDatabase {
  constructor(owner) {
    this.owner = owner;
    this.records = owner.records;
    this.objectStoreNames = { contains: (name) => name === "roms" };
  }
  createObjectStore() {}
  transaction(_name, mode) { return new MockTransaction(this.owner, mode); }
  close() {}
}

class MockIndexedDB {
  constructor() {
    this.records = new Map();
    this.nextFailure = null;
    this.openError = null;
  }
  open(_name, _version) {
    const request = { result: null, error: null };
    queueMicrotask(() => {
      if (this.openError) {
        request.error = new Error(this.openError);
        request.onerror?.();
      } else {
        request.result = new MockDatabase(this);
        request.onupgradeneeded?.();
        request.onsuccess?.();
      }
    });
    return request;
  }
}

test("save and load replace one record and return independent bytes", async () => {
  const indexedDB = new MockIndexedDB();
  const first = await saveCachedRom("first.nes", new Uint8Array([1, 2, 3]), { indexedDB, now: () => 10 });
  assert.deepEqual(first, { ok: true, savedAt: 10 });
  const second = await saveCachedRom("second.nes", new Uint8Array([4, 5]), { indexedDB, now: () => 20 });
  assert.deepEqual(second, { ok: true, savedAt: 20 });
  assert.equal(indexedDB.records.size, 1);

  const loaded = await loadCachedRom({ indexedDB });
  assert.equal(loaded.ok, true);
  assert.equal(loaded.name, "second.nes");
  assert.equal(loaded.savedAt, 20);
  assert.deepEqual([...loaded.bytes], [4, 5]);
  loaded.bytes[0] = 99;
  assert.deepEqual([...(await loadCachedRom({ indexedDB })).bytes], [4, 5]);
});

test("save clones a sliced view synchronously before asynchronous IndexedDB work", async () => {
  const indexedDB = new MockIndexedDB();
  const backing = new Uint8Array([77, 10, 20, 88]);
  const saving = saveCachedRom("slice.nes", backing.subarray(1, 3), { indexedDB });
  backing.fill(0);
  const result = await saving;
  assert.equal(result.ok, true);
  assert.deepEqual([...(await loadCachedRom({ indexedDB })).bytes], [10, 20]);
});

test("failed transaction leaves the prior ROM intact", async () => {
  const indexedDB = new MockIndexedDB();
  await saveCachedRom("good.nes", new Uint8Array([1, 2, 3]), { indexedDB });
  indexedDB.nextFailure = "QuotaExceededError";
  const result = await saveCachedRom("replacement.nes", new Uint8Array([4, 5]), { indexedDB });
  assert.equal(result.ok, false);
  assert.match(result.reason, /QuotaExceededError/);
  const loaded = await loadCachedRom({ indexedDB });
  assert.equal(loaded.name, "good.nes");
  assert.deepEqual([...loaded.bytes], [1, 2, 3]);
});

test("unavailable, open failure, invalid size, and corrupt records are safe results", async () => {
  assert.equal((await loadCachedRom({ indexedDB: null })).ok, false);
  assert.equal((await saveCachedRom("game.nes", new Uint8Array([1]), { indexedDB: null })).ok, false);
  const indexedDB = new MockIndexedDB();
  indexedDB.openError = "storage denied";
  assert.match((await loadCachedRom({ indexedDB })).reason, /storage denied/);
  indexedDB.openError = null;
  const oversized = await saveCachedRom("large.nes", new Uint8Array(MAX_ROM_BYTES + 1), { indexedDB });
  assert.equal(oversized.ok, false);
  assert.equal(indexedDB.records.size, 0);
  indexedDB.records.set("latest", { name: "broken.nes", bytes: "bad", savedAt: NaN });
  const corrupt = await loadCachedRom({ indexedDB });
  assert.equal(corrupt.ok, false);
  assert.match(corrupt.reason, /invalid/);
  assert.equal(indexedDB.records.has("latest"), true);
});

test("clear removes the cached entry and reports unavailable storage", async () => {
  const indexedDB = new MockIndexedDB();
  await saveCachedRom("game.nes", new Uint8Array([1]), { indexedDB });
  assert.deepEqual(await clearCachedRom({ indexedDB }), { ok: true });
  assert.equal((await loadCachedRom({ indexedDB })).name, null);
  assert.equal((await clearCachedRom({ indexedDB: null })).ok, false);
});
