import SwiftUI

/// Pure view: renders a TaskListView, reports intents. Only the add draft is local.
struct TaskListScreen: View {
    let view: TaskListView?
    let onIntent: (TaskIntent) -> Void

    var body: some View {
        NavigationStack {
            Group {
                if let view {
                    TaskListContent(view: view, onIntent: onIntent)
                } else {
                    ProgressView()
                }
            }
            .navigationTitle("taskr")
        }
        .modifier(TaskrTheme())
    }
}

/// The list without its own navigation, so the signed-in frame can embed it.
struct TaskListContent: View {
    let view: TaskListView
    let onIntent: (TaskIntent) -> Void

    @State private var draft = ""

    var body: some View {
        List {
            Section {
                Picker("Show", selection: scopeBinding(view)) {
                    ForEach(view.tabs, id: \.label) { tab in
                        Text(tab.label).tag(tab.scope)
                    }
                }
                .pickerStyle(.segmented)

                if view.canChange {
                    HStack {
                        TextField("New task", text: $draft)
                            .onSubmit(submit)
                            .submitLabel(.done)
                        Button("Add", action: submit)
                    }
                } else {
                    Text("You can view this list. Ask an owner for editor access to change it.")
                        .foregroundStyle(DesignTokens.current.muted)
                }
            }
            .listRowBackground(DesignTokens.current.surface)

            if let message = view.error ?? view.notice {
                Section {
                    HStack {
                        Text(message)
                            .foregroundStyle(view.error != nil ? DesignTokens.current.error : DesignTokens.current.fg)
                        Spacer()
                        Button("Dismiss") { onIntent(.messageDismissed) }
                    }
                }
                .listRowBackground(view.error != nil ? DesignTokens.current.errorBg : DesignTokens.current.noticeBg)
            }

            Section {
                if let empty = view.emptyMessage {
                    Text(empty).foregroundStyle(DesignTokens.current.muted)
                }
                ForEach(view.rows, id: \.id) { row in
                    TaskRowCell(row: row, columns: view.columns) {
                        onIntent(.toggleRequested(id: row.id))
                    }
                    .disabled(!view.canChange)
                }
            }
            .listRowBackground(DesignTokens.current.surface)
        }
        .scrollContentBackground(.hidden)
        .background(DesignTokens.current.bg)
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
                    .foregroundStyle(row.done ? DesignTokens.current.accent : DesignTokens.current.muted)
                Text(row.title)
                    .strikethrough(row.done)
                    .foregroundStyle(row.done ? DesignTokens.current.muted : DesignTokens.current.fg)
                Spacer()
                ForEach(trailing, id: \.self) { i in
                    Text(row.cells[i]).font(.caption).foregroundStyle(DesignTokens.current.muted)
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
