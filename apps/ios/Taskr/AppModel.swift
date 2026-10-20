import Observation

/// The signed-in app's container, same shape as TasksModel. Sign-in,
/// navigation and session expiry are decided in Rust.
@MainActor
@Observable
final class AppModel {
    private(set) var view: AppView = .starting
    @ObservationIgnored private let lane: AppLane

    init(presenter: AppPresenter) {
        // Every view the core shows, in order: the ones while requests run
        // (e.g. the new workspace loading) and the final one.
        let (views, continuation) = AsyncStream.makeStream(of: AppView.self)
        lane = AppLane(presenter: presenter, listener: ViewForwarder(views: continuation))
        Task { [weak self] in
            for await view in views { self?.view = view }
        }
    }

    func send(_ intent: AppIntent) {
        Task { await lane.dispatch(intent) }
    }
}

/// One core call at a time, away from the main actor (calls go to the network).
actor AppLane {
    private let presenter: AppPresenter
    private let listener: ViewForwarder

    init(presenter: AppPresenter, listener: ViewForwarder) {
        self.presenter = presenter
        self.listener = listener
    }

    func dispatch(_ intent: AppIntent) {
        listener.onView(view: presenter.dispatchWith(intent: intent, listener: listener))
    }
}

/// Hands the views from the core's thread to AppModel on the main actor.
final class ViewForwarder: AppViewListener, Sendable {
    private let views: AsyncStream<AppView>.Continuation

    init(views: AsyncStream<AppView>.Continuation) {
        self.views = views
    }

    func onView(view: AppView) {
        views.yield(view)
    }
}
