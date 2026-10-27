import SwiftUI
import UniformTypeIdentifiers

/// The new-transcript screen (WEB-056/060): pick files or add a YouTube video, choose
/// languages and speaker recognition, start. Core's `UploadQueueVm` decides the rows,
/// their chips and sizes, which languages are offered, whether start is enabled and
/// every alert; the picker is the only native piece.
struct NewTranscriptScreen: View {
    let vm: UploadQueueVm
    let youtube: Bool
    let copy: Copy
    let send: (AppIntent) -> Void
    let picked: ([URL]) -> Void

    @State private var picking = false
    @State private var language: LanguageSlot?

    private func upload(_ intent: UploadIntent) { send(.upload(intent)) }

    var body: some View {
        ZStack(alignment: .bottom) {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    ForEach(Array(vm.alerts.enumerated()), id: \.offset) { _, alert in alertCard(alert) }
                    if !vm.maxFilesReached {
                        SecondaryButton(title: copy.text(Labels.uploadFiles)) { picking = true }
                        if youtube { SecondaryButton(title: copy.text(Labels.addFromYouTube)) { upload(.youTubeDialogOpened) } }
                    }
                    if vm.rows.isEmpty {
                        VStack(spacing: 6) {
                            if vm.initializing { ProgressView().accessibilityLabel(copy.text(Labels.loading)) }
                            else {
                                Text(copy.text(Labels.noFiles)).font(.headline)
                                Text(copy.text(Labels.noFilesHint)).font(.subheadline).foregroundStyle(tokens.muted).multilineTextAlignment(.center)
                            }
                        }.frame(maxWidth: .infinity).padding(.vertical, 40)
                    } else {
                        Text(copy.text(Labels.files)).font(.headline)
                        ForEach(vm.rows, id: \.id) { row in fileRow(row) }
                        configuration
                        Color.clear.frame(height: 80)
                    }
                }
                .padding(16)
            }
            if !vm.rows.isEmpty { startBar }
        }
        .screenBackground()
        .sheet(isPresented: $picking) {
            DocumentPicker(accept: PickerAccept(vm.pickerAccept)) { urls in picking = false; if !urls.isEmpty { picked(urls) } }
        }
        .sheet(item: $language) { slot in languageSheet(slot) }
        .sheet(isPresented: coreFlag(vm.youtubeDialog.open) { if !$0 { upload(.youTubeDialogClosed) } }) { youTubeSheet }
    }

    // MARK: Pieces

    private func fileRow(_ row: FileRow) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                VStack(alignment: .leading) {
                    Text(row.name).lineLimit(1).fontWeight(.medium)
                    Text([row.phoneSize, copy.text(row.durationText)].compactMap { $0 }.joined(separator: " · ")).font(.caption).foregroundStyle(tokens.muted)
                }
                Spacer()
                Text(copy.text(row.phoneLabel)).font(.subheadline).foregroundStyle(tokens.muted)
                Button { upload(.removed(file: row.id)) } label: { Image(systemName: "trash") }
                    .foregroundStyle(tokens.error).frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.delete))
            }
            if case .uploading(let progress) = row.chip {
                ProgressView(value: Double(progress), total: 100).tint(tokens.progress).accessibilityLabel(row.name)
            }
        }
        .padding(12)
        .background(tokens.surface, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tokens.border))
    }

    private var configuration: some View {
        VStack(alignment: .leading, spacing: 12) {
            Toggle(copy.text(Labels.translation), isOn: coreFlag(vm.mode == .translation) { upload(.tabChanged(mode: $0 ? .translation : .transcription)) })
            languageRow(copy.text(Labels.audioLanguage), current: sourceName, slot: .audio)
            if vm.mode == .translation { languageRow(copy.text(Labels.translationLanguage), current: targetName, slot: .target) }
            Toggle(isOn: coreFlag(vm.speakers) { upload(.speakersToggled(enabled: $0)) }) {
                VStack(alignment: .leading) {
                    Text(copy.text(Labels.speakerRecognition))
                    Text(copy.text(Labels.speakerRecognitionHint)).font(.caption).foregroundStyle(tokens.muted)
                }
            }
        }
        .tint(tokens.accent)
    }

    private var sourceLanguages: [LanguageOption] { vm.mode == .translation ? vm.sourceLanguages : vm.available }

    private var sourceName: String {
        let value: String? = vm.mode == .translation ? vm.sourceLanguage : { if case .language(let code)? = vm.language { return code } else { return nil } }()
        if let value, let option = sourceLanguages.first(where: { $0.language == value }) { return "\(option.flag) \(option.name)" }
        if vm.mode != .translation, case .autoDetect? = vm.language { return copy.text(vm.autoDetectLabel) }
        return copy.text(Labels.chooseLanguage)
    }

    private var targetName: String {
        if let value = vm.targetLanguage, let option = vm.targetLanguages.first(where: { $0.language == value }) { return "\(option.flag) \(option.name)" }
        return copy.text(Labels.chooseLanguage)
    }

    private func languageRow(_ title: String, current: String, slot: LanguageSlot) -> some View {
        Button { language = slot } label: {
            HStack { Text(title).foregroundStyle(tokens.muted); Spacer(); Text(current) }
                .padding(12).frame(minHeight: 44)
                .background(tokens.surface, in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(tokens.border))
        }
        .accessibilityLabel("\(title): \(current)")
    }

    private var startBar: some View {
        PrimaryButton(title: copy.text(startLabel), busy: vm.transcribing, enabled: !vm.startDisabled) { upload(.startPressed) }
            .padding(16)
            .background(tokens.surface.ignoresSafeArea(edges: .bottom))
    }

    private var startLabel: Label {
        if vm.transcribing { return Labels.starting }
        if !vm.filesReady { return Labels.preparingFiles }
        return vm.mode == .translation ? Labels.startTranslating : Labels.startTranscribing
    }

    @ViewBuilder private func alertCard(_ alert: Alert) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            switch alert {
            case .balance:
                Text(copy.text(Labels.notEnoughBalance)).font(.headline)
                Text(copy.text(Labels.notEnoughBalanceBody))
            case .language:
                Text(copy.text(Labels.languageRequired)).font(.headline)
                Text(copy.text(Labels.languageRequiredBody))
            case .processingTimeout:
                Text(copy.text(Labels.processingTimeout)).font(.headline)
                Text(copy.text(Labels.processingTimeoutBody))
                Text(copy.text(Labels.processingTimeoutHint))
            case .youTube:
                Text(copy.text(Labels.youTubeFailed)).font(.headline)
                if let error = copy.text(vm.youtubeError) { Text(error) }
            }
            HStack { Spacer(); Button(copy.text(Labels.ok)) { upload(.dismissAlert(alert: alert)) }.frame(minHeight: 44) }
        }
        .padding(12)
        .background(tokens.noticeBg, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tokens.border))
        .accessibilityElement(children: .contain)
    }

    private var youTubeSheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(copy.text(Labels.youTubeTitle)).font(.headline)
            Text(copy.text(Labels.youTubeBody)).font(.subheadline).foregroundStyle(tokens.muted)
            LabeledField(label: copy.text(Labels.youTubeUrl), text: coreText(vm.youtubeDialog.url) { upload(.youTubeUrlChanged(url: $0)) },
                         keyboard: .URL, contentType: .URL, enabled: !vm.youtubeDialog.busy)
            if let error = copy.text(vm.youtubeDialog.error) { Banner(text: error, kind: .error) }
            HStack {
                SecondaryButton(title: copy.text(Labels.cancel), enabled: !vm.youtubeDialog.busy) { upload(.youTubeDialogClosed) }
                PrimaryButton(title: copy.text(vm.youtubeDialog.busy ? Labels.adding : Labels.addVideo), busy: vm.youtubeDialog.busy) { upload(.youTubeSubmitted) }
            }
            Spacer()
        }
        .padding(20)
        .presentationDetents([.medium])
        .interactiveDismissDisabled(vm.youtubeDialog.busy)
        .screenBackground()
    }

    private func languageSheet(_ slot: LanguageSlot) -> some View {
        let isTarget = slot == .target
        let options = isTarget ? vm.targetLanguages : sourceLanguages
        let preferred = isTarget ? vm.preferredTargets : vm.phonePreferred
        return LanguageSheet(
            title: copy.text(isTarget ? Labels.translationLanguage : Labels.audioLanguage),
            options: options, preferred: preferred,
            autoDetect: isTarget || vm.mode == .translation || !vm.autoDetectOffered ? nil : copy.text(vm.autoDetectLabel),
            searchPlaceholder: copy.text(Labels.searchLanguages), empty: copy.text(Labels.noLanguages),
            choose: { code in
                language = nil
                switch (slot, code) {
                case (.target, let code): upload(.targetLanguageChosen(language: code))
                case (.audio, let code) where vm.mode == .translation: upload(.sourceLanguageChosen(language: code))
                case (.audio, nil): upload(.languageChosen(choice: .autoDetect))
                case (.audio, let code?): upload(.languageChosen(choice: .language(code)))
                }
            })
    }
}

enum LanguageSlot: String, Identifiable { case audio, target; var id: String { rawValue } }

/// A searchable language list. The search text is a view mechanic; which languages exist
/// and which are preferred is core's.
struct LanguageSheet: View {
    let title: String
    let options: [LanguageOption]
    let preferred: [LanguageOption]
    let autoDetect: String?
    let searchPlaceholder: String
    let empty: String
    let choose: (String?) -> Void
    @State private var search = ""

    private var matches: [LanguageOption] {
        search.isEmpty ? options : options.filter { $0.name.localizedCaseInsensitiveContains(search) }
    }

    var body: some View {
        NavigationStack {
            List {
                if search.isEmpty {
                    if let autoDetect { Button(autoDetect) { choose(nil) } }
                    ForEach(preferred, id: \.language) { option in Button("\(option.flag) \(option.name)") { choose(option.language) } }
                }
                ForEach(matches, id: \.language) { option in Button("\(option.flag) \(option.name)") { choose(option.language) } }
                if matches.isEmpty { Text(empty).foregroundStyle(tokens.muted) }
            }
            .searchable(text: $search, prompt: searchPlaceholder)
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
        }
    }
}

/// The system document picker, with the types core's `picker_accept` names. Picked files
/// are copied into the app, so a security-scoped URL never outlives the picker.
struct DocumentPicker: UIViewControllerRepresentable {
    let accept: PickerAccept
    let done: ([URL]) -> Void

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        var types = accept.mimeTypes.compactMap { UTType(mimeType: $0) } + accept.extensions.compactMap { UTType(filenameExtension: $0) }
        if types.isEmpty { types = [.audio, .movie] }
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: true)
        picker.allowsMultipleSelection = true
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(done: done) }

    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let done: ([URL]) -> Void
        init(done: @escaping ([URL]) -> Void) { self.done = done }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { done(urls) }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { done([]) }
    }
}
