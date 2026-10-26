// Picked and dropped Files stay in the page. The presenter only ever sees an
// opaque handle plus the facts core filters on (name, size, MIME type).

import type { FileHandle } from "$lib/core/platform";

/** What a pick or drop intent carries for one held File. */
export interface HeldFile {
  handle: FileHandle;
  name: string;
  size: number;
  mime: string;
}

export class FileStore {
  #next: FileHandle = 1;
  readonly #files = new Map<FileHandle, File>();

  /** Holds every File in order; handles are never reused within a page. */
  hold(files: Iterable<File>): HeldFile[] {
    const held: HeldFile[] = [];
    for (const file of files) {
      const handle = this.#next++;
      this.#files.set(handle, file);
      held.push({ handle, name: file.name, size: file.size, mime: file.type });
    }
    return held;
  }

  get(handle: FileHandle): File | undefined {
    return this.#files.get(handle);
  }

  forget(handle: FileHandle): void {
    this.#files.delete(handle);
  }

  /** Dispose and session loss: nothing from the previous session is kept. */
  clear(): void {
    this.#files.clear();
  }

  get size(): number {
    return this.#files.size;
  }
}
