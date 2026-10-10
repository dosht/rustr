import Observation

/// iOS container: holds the latest view and moves core calls off the main
/// thread. The state machine itself is in Rust.
@MainActor
@Observable
final class TasksModel {
    private(set) var view: TaskListView?
    @ObservationIgnored private let lane: CoreLane

    init(presenter: TaskListPresenter) {
        lane = CoreLane(presenter: presenter)
    }

    func send(_ intent: TaskIntent) {
        Task { view = await lane.dispatch(intent) }
    }
}

/// Runs one core call at a time, away from the main actor (calls may touch disk).
actor CoreLane {
    private let presenter: TaskListPresenter

    init(presenter: TaskListPresenter) {
        self.presenter = presenter
    }

    func dispatch(_ intent: TaskIntent) -> TaskListView {
        presenter.dispatch(intent: intent)
    }
}
