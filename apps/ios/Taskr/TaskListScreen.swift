import SwiftUI

/// Pure view: renders a TaskListView, reports intents. Only the add draft is local.
struct TaskListScreen: View {
    let view: TaskListView?
    let onIntent: (TaskIntent) -> Void

    @State private var draft = ""

    var body: some View {
        NavigationStack {
            Group {
                if let view {
                    content(view)
                } else {
                    ProgressView()
                }
            }
            .navigationTitle("taskr")
        }
    }

    private func content(_ view: TaskListView) -> some View {
        List {
            Section {
                Picker("Show", selection: scopeBinding(view)) {
                    ForEach(view.tabs, id: \.label) { tab in
                        Text(tab.label).tag(tab.scope)
                    }
                }
                .pickerStyle(.segmented)

                HStack {
                    TextField("New task", text: $draft)
                        .onSubmit(submit)
                        .submitLabel(.done)
                    Button("Add", action: submit)
                }
            }

            if let message = view.error ?? view.notice {
                Section {
                    HStack {
                        Text(message)
                            .foregroundStyle(view.error != nil ? Color.red : Color.secondary)
                        Spacer()
                        Button("Dismiss") { onIntent(.messageDismissed) }
                    }
                }
            }

            Section {
                if let empty = view.emptyMessage {
                    Text(empty).foregroundStyle(.secondary)
                }
                ForEach(view.rows, id: \.id) { row in
                    TaskRowCell(row: row, columns: view.columns) {
                        onIntent(.toggleRequested(id: row.id))
                    }
                }
            }
        }
        .searchable(text: filterBinding(view), prompt: "Filter")
    }

    private func scopeBinding(_ view: TaskListView) -> Binding<TaskScope> {
        Binding(
            get: { view.tabs.first(where: \.selected)?.scope ?? .open },
            set: { onIntent(.scopeChanged(scope: $0)) }
        )
    }

    private func filterBinding(_ view: TaskListView) -> Binding<String> {
        Binding(get: { view.filter }, set: { onIntent(.filterChanged(text: $0)) })
    }

    private func submit() {
        onIntent(.addSubmitted(title: draft))
        draft = ""
    }
}

private struct TaskRowCell: View {
    let row: TaskRowView
    let columns: [ColumnView]
    let onToggle: () -> Void

    private var trailing: [Int] {
        columns.indices.filter { columns[$0].align == .right && !columns[$0].wide }
    }

    var body: some View {
        Button(action: onToggle) {
            HStack {
                Image(systemName: row.done ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(row.done ? Color.green : Color.secondary)
                Text(row.title)
                    .strikethrough(row.done)
                    .foregroundStyle(row.done ? Color.secondary : Color.primary)
                Spacer()
                ForEach(trailing, id: \.self) { i in
                    Text(row.cells[i]).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(row.title), \(row.done ? "done" : "open")")
    }
}

// Previews call into the real Rust core: the same fixture screens the TUI,
// web and Android previews are checked against.
#Preview("Sample") {
    TaskListScreen(view: previewScreens().first { $0.name == "sample" }?.view) { _ in }
}

#Preview("Empty") {
    TaskListScreen(view: previewScreens().first { $0.name == "empty" }?.view) { _ in }
}

#Preview("Error") {
    TaskListScreen(view: previewScreens().first { $0.name == "error" }?.view) { _ in }
}
