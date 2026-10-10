import os

/// The telemetry port, fulfilled by the unified logging system.
final class OSLogSink: LogSink {
    private let logger = Logger(subsystem: "dev.taskr.ios", category: "core")

    func log(level: LogLevel, event: String, fields: [LogField]) {
        let message = ([event] + fields.map { "\($0.key)=\($0.value)" }).joined(separator: " ")
        switch level {
        case .debug: logger.debug("\(message, privacy: .public)")
        case .info: logger.info("\(message, privacy: .public)")
        case .warn: logger.warning("\(message, privacy: .public)")
        case .error: logger.error("\(message, privacy: .public)")
        }
    }
}
