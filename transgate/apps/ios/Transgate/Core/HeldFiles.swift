import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// Reads a held file's duration in seconds; `nil` when it cannot be measured.
/// AVFoundation implements it on a device (see `AVMediaDurationReader`).
protocol MediaDurationReader: Sendable {
    func seconds(of file: URL) -> Double?
}

/// The files the user picked. Core only sees opaque handles (`PickedFile`); the
/// shell keeps the file behind each handle until core releases it. Measuring and
/// the PUT run here, as web does in `platform/media`.
final class HeldFiles: @unchecked Sendable {
    private let lock = NSLock()
    private var files: [UInt64: URL] = [:]
    private var next: UInt64 = 1
    private let reader: MediaDurationReader

    init(reader: MediaDurationReader) {
        self.reader = reader
    }

    /// Holds the picked files and describes them for core's `picked` intent.
    func hold(_ urls: [URL]) -> [PickedFile] {
        urls.map { url in
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64) ?? 0
            let handle = lock.withLock { () -> UInt64 in
                let handle = next
                next += 1
                files[handle] = url
                return handle
            }
            return PickedFile(handle: handle, name: url.lastPathComponent, size: size, mime: MimeTypes.mime(forExtension: url.pathExtension))
        }
    }

    func url(for handle: UInt64) -> URL? { lock.withLock { files[handle] } }

    func measure(_ handle: UInt64) -> Double? {
        url(for: handle).flatMap(reader.seconds(of:))
    }

    func release(_ handle: UInt64) {
        lock.withLock { _ = files.removeValue(forKey: handle) }
    }

    var heldCount: Int { lock.withLock { files.count } }

    /// One PUT of the file itself to the signed URL; `true` only for HTTP 200, never
    /// retried (UPL-013/014). Blocks the calling thread, which Rust runs on a
    /// blocking thread for exactly this reason.
    func upload(_ handle: UInt64, to destination: String, progress: @escaping @Sendable (UInt8) -> Void) -> Bool {
        guard let file = url(for: handle), let target = URL(string: destination) else { return false }
        var request = URLRequest(url: target)
        request.httpMethod = "PUT"
        request.setValue(MimeTypes.mime(forExtension: file.pathExtension), forHTTPHeaderField: "Content-Type")
        let delegate = UploadDelegate(progress: progress)
        let session = URLSession(configuration: .ephemeral, delegate: delegate, delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        // Delegate delivery (not a completion handler) so progress callbacks run on every platform.
        session.uploadTask(with: request, fromFile: file).resume()
        delegate.done.wait()
        return delegate.succeeded
    }
}

private final class UploadDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    let done = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var ok = false
    private let progress: @Sendable (UInt8) -> Void
    private var last: Int = -1

    init(progress: @escaping @Sendable (UInt8) -> Void) { self.progress = progress }

    var succeeded: Bool { lock.withLock { ok } }

    func urlSession(_ session: URLSession, task: URLSessionTask, didSendBodyData bytesSent: Int64, totalBytesSent: Int64, totalBytesExpectedToSend: Int64) {
        // The source reports progress only when the total is known.
        guard totalBytesExpectedToSend > 0 else { return }
        let percent = Int((Double(totalBytesSent) / Double(totalBytesExpectedToSend) * 100).rounded())
        if percent != last {
            last = percent
            progress(UInt8(min(max(percent, 0), 100)))
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        lock.withLock { ok = error == nil && (task.response as? HTTPURLResponse)?.statusCode == 200 }
        done.signal()
    }
}

/// File name extension to MIME type, for the file picker and the PUT header. A
/// transport detail: core never sees extensions, only the `mime` string.
enum MimeTypes {
    private static let table: [String: String] = [
        "mp3": "audio/mpeg", "m4a": "audio/mp4", "wav": "audio/wav", "webm": "audio/webm",
        "aac": "audio/aac", "ogg": "audio/ogg", "flac": "audio/flac",
        "mp4": "video/mp4", "mov": "video/quicktime", "m4v": "video/x-m4v",
    ]

    static func mime(forExtension ext: String) -> String {
        table[ext.lowercased()] ?? "application/octet-stream"
    }
}

/// Core's `picker_accept` string ("audio/mpeg,audio/mp4,.mp3,...") as the pieces a
/// document picker needs: MIME types and file name extensions.
struct PickerAccept: Equatable, Sendable {
    var mimeTypes: [String] = []
    var extensions: [String] = []

    init(_ accept: String) {
        for token in accept.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespaces) }) where !token.isEmpty {
            if token.hasPrefix(".") { extensions.append(String(token.dropFirst())) } else { mimeTypes.append(token) }
        }
    }
}
