import type { RemoteDirectory, Separator } from "./tree";

/** The last browse's listing, which can be tens of megabytes, so it is kept in IndexedDB */
export type Listing = {
  username: string;
  directories: RemoteDirectory[];
  separator: Separator;
  info: BrowseInfo;
};

/** How much the user shares: open folders and files, and those only some users may have */
export type BrowseInfo = {
  directories: number;
  files: number;
  lockedDirectories: number;
  lockedFiles: number;
  bytes: number;
};

const DATABASE = "reseek-browse";
const STORE = "browse";
const KEY = "current";

const open = () =>
  new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });

const run = async <T>(
  mode: IDBTransactionMode,
  act: (store: IDBObjectStore) => IDBRequest<T>
): Promise<T> => {
  const db = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const request = act(db.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
};

/** The listing kept from the last browse, undefined when there is none */
export const readListing = () => run<Listing | undefined>("readonly", (s) => s.get(KEY));

/** Keeps `listing` in place of the last one, or forgets it with null */
export const writeListing = async (listing: Listing | null) => {
  if (listing) await run("readwrite", (s) => s.put(listing, KEY));
  else await run("readwrite", (s) => s.delete(KEY));
};
