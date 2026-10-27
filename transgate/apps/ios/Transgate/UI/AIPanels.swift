import SwiftUI

/// AI consent, analysis and chat under a transcript (WEB-070). Core's `ConsentPanelVm`,
/// `AnalysisPanelVm` and `ChatPanelVm` decide whether each panel exists, its state, its
/// text and every available action.
struct AIPanels: View {
    let vm: SignedInVm
    let copy: Copy
    let send: (AppIntent) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if vm.consent.gate == .required {
                ConsentCard(vm: vm.consent, copy: copy, send: send)
            }
            if let analysis = vm.analysis { AnalysisCard(vm: analysis, copy: copy, send: send) }
            if let chat = vm.chat { ChatCard(vm: chat, copy: copy, send: send) }
        }
    }
}

struct ConsentCard: View {
    let vm: ConsentPanelVm
    let copy: Copy
    let send: (AppIntent) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(copy.text(Labels.aiConsentTitle)).font(.headline)
            Text(copy.text(Labels.aiProcessing)).font(.subheadline)
            Text(copy.text(Labels.aiFeatures)).font(.subheadline.weight(.semibold))
            ForEach([Labels.aiSummaries, Labels.aiHighlights, Labels.aiChatFeature], id: \.key) { feature in
                Text("• " + copy.text(feature)).font(.subheadline)
            }
            if let url = URL(string: vm.privacyPolicyUrl) { Link(copy.text(Labels.aiPrivacy), destination: url).frame(minHeight: 44) }
            if let error = copy.text(vm.loadError) ?? copy.text(vm.actionError) { Banner(text: error, kind: .error, dismissLabel: copy.text(Labels.dismiss), dismiss: { send(.consent(.errorDismissed)) }) }
            HStack {
                SecondaryButton(title: copy.text(Labels.aiDecline), enabled: vm.busy == nil) { send(.consent(.declineRequested)) }
                PrimaryButton(title: copy.text(Labels.aiAccept), busy: vm.busy == .enable) { send(.consent(.enableRequested)) }
            }
        }
        .padding(14)
        .background(tokens.noticeBg, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tokens.border))
    }
}

struct AnalysisCard: View {
    let vm: AnalysisPanelVm
    let copy: Copy
    let send: (AppIntent) -> Void

    private func act(_ intent: AnalysisIntent) { send(.analysis(intent)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(copy.text(Labels.aiAnalysis)).font(.headline)
                Spacer()
                if vm.hasData {
                    Button { act(.copyRequested) } label: { Image(systemName: "doc.on.doc") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.copyToClipboard))
                    Menu {
                        ForEach([DocumentFormat.docx, .pdf, .txt, .md], id: \.self) { format in
                            Button(String(describing: format).uppercased()) { act(.exportRequested(format: format)) }
                        }
                    } label: {
                        if vm.exportBusy != nil { ProgressView() } else { Image(systemName: "square.and.arrow.up") }
                    }
                    .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.export))
                }
            }
            if let error = copy.text(vm.exportError) { Banner(text: error, kind: .error, dismissLabel: copy.text(Labels.dismiss), dismiss: { act(.exportErrorDismissed) }) }
            switch vm.state {
            case .idle: EmptyView()
            case .checkingCache, .loading:
                if let progress = vm.progress {
                    ProgressView(value: Double(progress.percentage), total: 100).tint(tokens.progress)
                    Text(copy.text(progress.stage)).font(.caption).foregroundStyle(tokens.muted)
                } else {
                    HStack { ProgressView(); Text(copy.text(Labels.analyzing)).foregroundStyle(tokens.muted) }
                }
            case .ready:
                if vm.noTranscript { Text(copy.text(Labels.noTranscriptContent)).foregroundStyle(tokens.muted) }
                if let summary = vm.summary { Text(summary) }
                if case .generic(let generic)? = vm.analysis, let highlights = generic.highlights, !highlights.isEmpty {
                    Text(copy.text(Labels.keyHighlights)).font(.subheadline.weight(.semibold))
                    ForEach(highlights, id: \.self) { Text("• \($0)") }
                }
            case .interrupted, .cancelled, .error:
                Text(copy.text(Labels.analysisIncomplete)).font(.subheadline.weight(.semibold))
                if let hint = copy.text(vm.errorHint) { Text(hint).foregroundStyle(tokens.muted) }
                if let detail = vm.errorDetail { Text(detail).font(.caption).foregroundStyle(tokens.muted) }
                if vm.canRetry { SecondaryButton(title: copy.text(Labels.retry)) { act(.retryRequested) } }
            }
        }
        .padding(14)
        .background(tokens.surface, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tokens.border))
        .onAppear { act(.shown) }
        .onDisappear { act(.hidden) }
    }
}

struct ChatCard: View {
    let vm: ChatPanelVm
    let copy: Copy
    let send: (AppIntent) -> Void
    @State private var draft = ""

    private func act(_ intent: ChatIntent) { send(.chat(intent)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(copy.text(Labels.askAi)).font(.headline)
                Spacer()
                if !vm.lines.isEmpty {
                    Button { act(.copyChatRequested) } label: { Image(systemName: "doc.on.doc") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.copyChat))
                    Button { act(.downloadRequested) } label: { Image(systemName: "square.and.arrow.down") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.downloadChat))
                }
            }
            if vm.loadingHistory { ProgressView().accessibilityLabel(copy.text(Labels.loading)) }
            if vm.showWelcome {
                Text(copy.text(Labels.chatStart)).font(.subheadline.weight(.semibold))
                Text(copy.text(Labels.chatWelcome)).font(.subheadline).foregroundStyle(tokens.muted)
            }
            ForEach(Array(vm.lines.enumerated()), id: \.offset) { index, line in message(UInt64(index), line) }
            if vm.typing { Text(copy.text(Labels.chatThinking)).font(.caption).foregroundStyle(tokens.muted) }
            if let error = vm.error {
                Banner(text: [copy.text(error.message) ?? copy.text(Labels.chatFailed), error.detail].compactMap { $0 }.joined(separator: "\n"), kind: .error,
                       dismissLabel: copy.text(Labels.dismiss), dismiss: { act(.errorDismissed) })
            }
            if let notice = copy.text(vm.notice) { Banner(text: notice, dismissLabel: copy.text(Labels.dismiss), dismiss: { act(.noticeDismissed) }) }
            if let warning = copy.text(vm.syncWarning) { Banner(text: warning) }
            if vm.enabled {
                HStack(alignment: .bottom) {
                    TextField(copy.text(Labels.chatPlaceholder), text: $draft, axis: .vertical).lineLimit(1...5)
                        .padding(10).background(tokens.bg, in: RoundedRectangle(cornerRadius: 8))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(tokens.border))
                        .disabled(vm.inputLocked)
                    Button {
                        act(.sendRequested(text: draft)); draft = ""
                    } label: { Image(systemName: "paperplane.fill").frame(minWidth: 44, minHeight: 44) }
                        .disabled(!vm.canSend || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityLabel(copy.text(Labels.sendMessage))
                }
            }
        }
        .padding(14)
        .background(tokens.surface, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tokens.border))
    }

    private func message(_ index: UInt64, _ line: ChatLineVm) -> some View {
        VStack(alignment: line.role == .user ? .trailing : .leading, spacing: 4) {
            Text(line.content)
                .padding(10)
                .background(line.role == .user ? tokens.cursorBg : tokens.bg, in: RoundedRectangle(cornerRadius: 10))
                .frame(maxWidth: .infinity, alignment: line.role == .user ? .trailing : .leading)
            if line.showActions {
                HStack(spacing: 4) {
                    Button { act(.copyMessageRequested(index: index)) } label: { Image(systemName: "doc.on.doc") }.accessibilityLabel(copy.text(Labels.copyMessage))
                    if line.role == .assistant {
                        Button { act(.rateRequested(index: index, rating: .like)) } label: { Image(systemName: "hand.thumbsup") }.accessibilityLabel(copy.text(Labels.helpful))
                        Button { act(.rateRequested(index: index, rating: .dislike)) } label: { Image(systemName: "hand.thumbsdown") }.accessibilityLabel(copy.text(Labels.notHelpful))
                        Button { act(.retryRequested(index: index)) } label: { Image(systemName: "arrow.clockwise") }.accessibilityLabel(copy.text(Labels.retry))
                    }
                }.buttonStyle(.borderless).foregroundStyle(tokens.muted).frame(minHeight: 44)
            }
        }
    }
}
