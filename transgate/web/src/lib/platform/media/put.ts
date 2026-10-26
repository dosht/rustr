// UPL-013/014: one PUT of the File itself to the signed URL the Rust client got
// from /api/init-upload. Only HTTP 200 counts; never retried here.

export interface PutOutcome {
  ok: boolean;
  /** 0 when the request never completed (network error, abort, timeout). */
  status: number;
}

export function putFile(
  file: File,
  url: string,
  onProgress: (percent: number) => void,
  createRequest: () => XMLHttpRequest = () => new XMLHttpRequest(),
): Promise<PutOutcome> {
  return new Promise((resolve) => {
    const failed = () => resolve({ ok: false, status: 0 });
    const request = createRequest();
    try {
      request.open("PUT", url);
      request.setRequestHeader("Content-Type", file.type || "application/octet-stream");
    } catch {
      failed();
      return;
    }
    request.upload.onprogress = (event) => {
      // The source reports progress only when the total is known.
      if (event.total) onProgress(Math.round((event.loaded / event.total) * 100));
    };
    request.onload = () => resolve({ ok: request.status === 200, status: request.status });
    request.onerror = failed;
    request.onabort = failed;
    request.ontimeout = failed;
    request.send(file);
  });
}
