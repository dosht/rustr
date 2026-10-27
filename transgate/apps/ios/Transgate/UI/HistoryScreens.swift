import SwiftUI

/// All transcripts (WEB-064): core's `JobListVm` decides the rows, their status, tone and
/// available actions, paging and the delete confirmation.
struct HistoryScreen: View {
    let vm: JobListVm
    let copy: Copy
    let send: (AppIntent) -> Void
    let onNew: () -> Void

    private func jobs(_ intent: JobsIntent) { send(.jobs(intent)) }

    var body: some View {
        Group {
            if vm.initialLoading && vm.rows.isEmpty {
                VStack(spacing: 12) { ProgressView(); Text(copy.text(Labels.loadingJobs)).foregroundStyle(tokens.muted) }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = copy.text(vm.error) {
                VStack(spacing: 12) {
                    Text(error).multilineTextAlignment(.center)
                    if let detail = vm.errorDetail { Text(detail).font(.caption).foregroundStyle(tokens.muted) }
                    PrimaryButton(title: copy.text(Labels.tryAgain)) { jobs(.refresh) }
                }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if vm.rows.isEmpty {
                VStack(spacing: 8) {
                    Text(copy.text(Labels.noTranscripts)).font(.headline)
                    Text(copy.text(Labels.noTranscriptsHint)).foregroundStyle(tokens.muted)
                    PrimaryButton(title: copy.text(Labels.newTranscription), action: onNew)
                }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                list
            }
        }
        .screenBackground()
        .confirmationDialog(copy.text(Labels.deleteFileTitle), isPresented: coreFlag(vm.confirmDelete != nil) { if !$0 { jobs(.deleteCanceled) } }, titleVisibility: .visible) {
            Button(copy.text(Labels.yes), role: .destructive) { jobs(.deleteConfirmed) }
            Button(copy.text(Labels.no), role: .cancel) { jobs(.deleteCanceled) }
        } message: { Text(copy.text(Labels.deleteFileBody)) }
    }

    private var list: some View {
        List {
            ForEach(vm.rows, id: \.id) { row in
                Button { send(.navigate(route: .home(.job(row.id)))) } label: { card(row) }
                    .buttonStyle(.plain)
                    .listRowBackground(tokens.surface)
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) { jobs(.deleteRequested(job: row.id)) } label: { Text(copy.text(Labels.delete)) }
                    }
                    .onAppear { if row.id == vm.rows.last?.id && vm.canLoadMore { jobs(.loadMore) } }
            }
            if vm.loadingMore { HStack { Spacer(); ProgressView(); Spacer() }.listRowBackground(Color.clear) }
            if let end = copy.text(vm.endOfList) { Text(end).font(.caption).foregroundStyle(tokens.muted).frame(maxWidth: .infinity).listRowBackground(Color.clear) }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .refreshable { jobs(.refresh) }
    }

    private func card(_ row: JobRow) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Image(systemName: row.isTranslation ? "character.bubble" : "mic").foregroundStyle(tokens.muted).accessibilityHidden(true)
                Text(row.displayName ?? copy.text(vm.untitled)).fontWeight(.semibold).lineLimit(1)
                Spacer()
                ToneBadge(text: copy.text(row.statusLabel), tone: row.tone, spinner: row.spinner)
            }
            Text([row.dateText, row.duration, copy.text(row.speakers), row.language.map { "\(row.flag ?? "") \($0)" }].compactMap { $0 }.joined(separator: " · "))
                .font(.caption).foregroundStyle(tokens.muted)
            HStack(spacing: 16) {
                if row.canDownload {
                    Button { jobs(.downloadRequested(job: row.id)) } label: { Image(systemName: "square.and.arrow.down") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.download)).disabled(row.downloading)
                }
                if row.canFeedback {
                    Button { send(.feedback(.opened(job: row.id))) } label: { Image(systemName: "star.bubble") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.feedback))
                }
            }.buttonStyle(.borderless)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
    }
}

/// The recent-transcripts drawer content (WEB-064): new transcript, all transcripts, recents.
struct RecentSheet: View {
    let vm: SignedInVm
    let copy: Copy
    let send: (AppIntent) -> Void
    let close: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Button(copy.text(Labels.newTranscript)) { send(.navigate(route: vm.newTranscriptRoute)); close() }
                Button(copy.text(Labels.allTranscripts)) { send(.navigate(route: .home(.allTranscripts))); close() }
                Section(copy.text(Labels.recent)) {
                    if vm.jobs.recent.isEmpty { Text(copy.text(vm.jobs.recentEmpty)).foregroundStyle(tokens.muted) }
                    ForEach(vm.jobs.recent, id: \.id) { row in
                        Button { send(.navigate(route: .home(.job(row.id)))); close() } label: {
                            VStack(alignment: .leading) {
                                Text(row.fileName ?? copy.text(vm.jobs.untitled)).fontWeight(row.selected ? .semibold : .regular).lineLimit(1)
                                if let duration = row.duration { Text(duration).font(.caption).foregroundStyle(tokens.muted) }
                            }
                        }
                    }
                }
            }
            .navigationTitle(copy.text(Labels.recent))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(copy.text(Labels.close), action: close) } }
        }
    }
}
