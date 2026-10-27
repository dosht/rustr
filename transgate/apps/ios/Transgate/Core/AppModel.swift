import Foundation
import Observation

/// The signed-in app's container: owns the presenter, forwards intents in order and
/// publishes core's view. It decides nothing; sign-in, routes, layouts and every
/// text are core's (`AppVm`, `translate()`).
///
/// A custom-scheme return (Google, magic link, recovery) starts the presenter anew
/// from the returned location, exactly as a page load does on the web: core accepts
/// `Started` once per presenter and consumes `?code=` from it.
@MainActor
@Observable
final class AppModel {
    private(set) var view: AppVm = .restoring(locale: "en")
    /// A technical failure to start the core (not a user message; shown raw).
    private(set) var startError: String?

    @ObservationIgnored private let settings: AppSettings
    @ObservationIgnored private let ports: TransgateNativePorts
    @ObservationIgnored private let files: HeldFiles
    @ObservationIgnored private let userAgent: String
    @ObservationIgnored private var viewport: Viewport
    @ObservationIgnored private var visible = true
    @ObservationIgnored private var lane: Lane?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private let deliveries: AsyncStream<Delivery>.Continuation

    init(settings: AppSettings, ports: TransgateNativePorts, files: HeldFiles, userAgent: String, width: UInt32) {
        self.settings = settings
        self.ports = ports
        self.files = files
        self.userAgent = userAgent
        self.viewport = Viewport(touch: true, width: width)
        let (stream, continuation) = AsyncStream.makeStream(of: Delivery.self)
        deliveries = continuation
        Task { [weak self] in
            for await delivery in stream { self?.deliver(delivery) }
        }
    }

    var copy: Copy { Copy(locale: view.locale) }

    /// Starts the core from `location` (D4: `/home`, or a returned auth URL).
    func start(location: String = "/home") async {
        generation += 1
        let mine = generation
        let previous = lane
        lane = nil
        previous?.stop()
        startError = nil
        let config = settings.presenterConfig(userAgent: userAgent)
        let ports = ports
        let deliveries = deliveries
        let viewport = viewport
        do {
            // The constructor blocks until the driver thread is up; keep it off the main actor.
            let lane = try await Task.detached { () throws -> Lane in
                let presenterLane = try Lane(generation: mine, config: config, ports: ports, deliveries: deliveries)
                presenterLane.send(.started(url: location, viewport: viewport, utcOffsetMinutes: Int16(TimeZone.current.secondsFromGMT() / 60)))
                return presenterLane
            }.value
            guard mine == generation else { return lane.stop() }
            self.lane = lane
            lane.send(.visibilityChanged(visible: visible))
        } catch {
            startError = "\(error)"
        }
    }

    func send(_ intent: AppIntent) {
        lane?.send(intent)
    }

    /// The document picker returned files: hold them and tell core.
    func picked(_ urls: [URL]) {
        send(.upload(.picked(files: files.hold(urls))))
    }

    func setVisible(_ value: Bool) {
        guard value != visible else { return }
        visible = value
        send(.visibilityChanged(visible: value))
    }

    func setWidth(_ width: UInt32) {
        guard width != viewport.width else { return }
        viewport = Viewport(touch: true, width: width)
        send(.viewportChanged(viewport: viewport))
    }

    /// A URL opened through the app's custom scheme (D7); anything else is ignored.
    func handleReturn(_ url: URL) {
        guard let location = AuthReturn.startLocation(for: url, scheme: settings.callbackScheme) else { return }
        Task { await start(location: location) }
    }

    func stop() {
        generation += 1
        lane?.stop()
        lane = nil
    }

    private func deliver(_ delivery: Delivery) {
        guard delivery.generation == generation, delivery.view != view else { return }
        view = delivery.view
    }
}

struct Delivery: Sendable {
    var generation: Int
    var view: AppVm
}

/// One presenter and the in-order queue of intents sent to it, worked away from the
/// main thread (dispatch waits for the core's driver thread, microseconds).
private final class Lane: @unchecked Sendable {
    private let presenter: TransgatePresenter
    private let intents: AsyncStream<AppIntent>.Continuation

    init(generation: Int, config: PresenterConfig, ports: TransgateNativePorts, deliveries: AsyncStream<Delivery>.Continuation) throws {
        let listener = Forwarder(generation: generation, deliveries: deliveries)
        presenter = try TransgatePresenter(config: config, ports: ports, listener: listener)
        let (stream, continuation) = AsyncStream.makeStream(of: AppIntent.self)
        intents = continuation
        let presenter = presenter
        Task.detached(priority: .userInitiated) {
            for await intent in stream {
                do {
                    deliveries.yield(Delivery(generation: generation, view: try presenter.dispatch(intent: intent)))
                } catch {
                    // Disposed: a late intent after stop(); nothing to show.
                    Diagnostics.warn("dispatch refused: \(error)")
                }
            }
        }
    }

    func send(_ intent: AppIntent) { intents.yield(intent) }

    func stop() {
        intents.finish()
        presenter.dispose()
    }
}

/// Hands the views from the core's listener thread to the model, in order.
private final class Forwarder: ViewListener, Sendable {
    private let generation: Int
    private let deliveries: AsyncStream<Delivery>.Continuation

    init(generation: Int, deliveries: AsyncStream<Delivery>.Continuation) {
        self.generation = generation
        self.deliveries = deliveries
    }

    func viewChanged(view: AppVm) {
        deliveries.yield(Delivery(generation: generation, view: view))
    }
}
