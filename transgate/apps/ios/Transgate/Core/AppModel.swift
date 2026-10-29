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
        ports.reload = { [weak self] path in
            Task { @MainActor in await self?.start(location: path) }
        }
        Task { [weak self] in
            for await delivery in stream { self?.deliver(delivery) }
        }
    }

    var copy: Copy { Copy(locale: view.locale) }

    /// Starts the core from `location` (D4: `/home`, or a returned auth URL).
    func start(location: String? = nil) async {
        let location = location ?? ports.startLocation
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
        ports.rememberLocale(view.locale)
    }
}

struct Delivery: Sendable {
    var generation: Int
    var view: AppVm
}

/// One presenter and the in-order queue of intents sent to it, worked away from the
/// main thread (dispatch waits for the core's driver thread, microseconds).
final class Lane: @unchecked Sendable {
    private let presenter: TransgatePresenterProtocol
    private let intents: AsyncStream<LaneEvent>.Continuation

    convenience init(generation: Int, config: PresenterConfig, ports: TransgateNativePorts, deliveries: AsyncStream<Delivery>.Continuation) throws {
        try self.init(generation: generation, deliveries: deliveries) { listener in
            try TransgatePresenter(config: config, ports: ports, listener: listener)
        }
    }

    init(generation: Int, deliveries: AsyncStream<Delivery>.Continuation, makePresenter: (ViewListener) throws -> TransgatePresenterProtocol) throws {
        let (stream, continuation) = AsyncStream.makeStream(of: LaneEvent.self)
        intents = continuation
        let listener = Forwarder(events: continuation)
        presenter = try makePresenter(listener)
        let presenter = presenter
        Task.detached(priority: .userInitiated) {
            for await intent in stream {
                do {
                    let view: AppVm
                    switch intent {
                    case .intent(let value): view = try presenter.dispatch(intent: value)
                    case .refresh: view = try presenter.current()
                    }
                    deliveries.yield(Delivery(generation: generation, view: view))
                } catch {
                    // Disposed: a late intent after stop(); nothing to show.
                    Diagnostics.warn("dispatch refused: \(error)")
                }
            }
        }
    }

    func send(_ intent: AppIntent) { intents.yield(.intent(intent)) }

    func stop() {
        intents.finish()
        presenter.dispose()
    }
}

/// Hands the views from the core's listener thread to the model, in order.
private final class Forwarder: ViewListener, Sendable {
    private let events: AsyncStream<LaneEvent>.Continuation

    init(events: AsyncStream<LaneEvent>.Continuation) {
        self.events = events
    }

    func viewChanged(view: AppVm) {
        events.yield(.refresh)
    }
}

private enum LaneEvent: Sendable {
    case intent(AppIntent)
    case refresh
}
