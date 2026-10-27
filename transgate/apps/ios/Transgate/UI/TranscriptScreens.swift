import SwiftUI

/// One transcript or translation (WEB-064/068): lines with speakers and times, language
/// tabs, editing, playback, export and the AI panels. Core's `TranscriptVm` decides every
/// row, toggle, label, state and fade; the shell draws it and forwards intents.
struct TranscriptScreen: View {
    let vm: SignedInVm
    let transcript: TranscriptVm
    let copy: Copy
    let player: AVPlayerControl
    let send: (AppIntent) -> Void
    let openURL: (URL) -> Void

    @State private var draft = ""
    @State private var snapshot = PlayerSnapshot(time: 0, playing: false)
    @State private var renaming: String?
    @State private var newName = ""

    private func act(_ intent: TranscriptIntent) { send(.transcript(intent)) }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                header
                if let alert = copy.text(transcript.noAudioAlert) { Banner(text: alert, dismissLabel: copy.text(Labels.dismiss), dismiss: { act(.noAudioAlertDismissed) }) }
                mediaBar
                if transcript.showTabs { tabs }
                toggles
                content
                if let freemium = transcript.freemium { freemiumBar(freemium) }
                AIPanels(vm: vm, copy: copy, send: send)
            }
            .padding(16)
        }
        .screenBackground()
        .alert(copy.text(Labels.editSpeaker), isPresented: coreFlag(renaming != nil) { if !$0 { renaming = nil } }) {
            TextField(copy.text(Labels.editSpeaker), text: $newName)
            Button(copy.text(Labels.saveLine)) { if let from = renaming { act(.speakerRenamed(from: from, to: newName)) }; renaming = nil }
            Button(copy.text(Labels.cancel), role: .cancel) { renaming = nil }
        }
    }

    // MARK: Header, media, tabs, toggles

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(transcript.title).font(.title3.weight(.semibold))
            HStack(spacing: 8) {
                if let minutes = transcript.durationMinutes { Text("\(minutes) \(copy.text(transcript.minutesUnit))").font(.caption).foregroundStyle(tokens.muted) }
                if let badge = transcript.statusBadge { ToneBadge(text: copy.text(badge.label), tone: badge.tone, spinner: badge.spinning, icon: badge.icon) }
            }
            HStack(spacing: 4) {
                if transcript.canCopy {
                    Button { act(.copyPressed) } label: { Image(systemName: "doc.on.doc") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.copyAll))
                }
                if transcript.downloadEnabled {
                    Menu {
                        Button(copy.text(Labels.download)) { act(.downloadPressed(allFormats: false)) }
                        Button(copy.text(Labels.downloadAll)) { act(.downloadPressed(allFormats: true)) }
                    } label: { Image(systemName: "square.and.arrow.down").frame(minWidth: 44, minHeight: 44) }
                        .accessibilityLabel(copy.text(Labels.download))
                }
                if let job = transcript.job {
                    Button { send(.feedback(.opened(job: job))) } label: { Image(systemName: "star.bubble") }
                        .frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.feedback))
                }
            }.buttonStyle(.borderless)
        }
    }

    @ViewBuilder private var mediaBar: some View {
        switch transcript.media {
        case .none:
            EmptyView()
        case .audio(let url, let loading, let unavailable):
            if loading { ProgressView().accessibilityLabel(copy.text(Labels.loading)) }
            else if unavailable || url == nil { Banner(text: copy.text(Labels.audioUnavailable)) }
            else if let url, let source = URL(string: url) { PlayerBar(source: source, player: player, playLabel: copy.text(Labels.play), pauseLabel: copy.text(Labels.pause), snapshot: $snapshot, onState: { act(.playerState(playing: $0)) }) }
        case .youTube(let videoId):
            // D11: no SDK; the video opens in the YouTube app or the browser.
            Button { openURL(youTubeURL(videoId, seconds: 0)) } label: { Image(systemName: "play.rectangle.fill").font(.title2).frame(minWidth: 44, minHeight: 44) }
                .accessibilityLabel("YouTube")
        }
    }

    private var tabs: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack {
                ForEach(transcript.tabs, id: \.tab) { option in
                    let selected = option.tab == transcript.selected
                    Button("\(option.flag) \(option.name)") { act(.tabSelected(tab: option.tab)) }
                        .buttonStyle(.bordered).tint(selected ? tokens.accent : tokens.muted)
                        .accessibilityAddTraits(selected ? .isSelected : [])
                }
            }
        }
    }

    private var toggles: some View {
        HStack {
            if transcript.timeToggle { Toggle(copy.text(Labels.showTime), isOn: coreFlag(transcript.showTime) { act(.showTimeToggled(on: $0)) }) }
            if transcript.phoneSpeakerToggle { Toggle(copy.text(Labels.showSpeakers), isOn: coreFlag(transcript.showSpeakers) { act(.showSpeakersToggled(on: $0)) }) }
        }.tint(tokens.accent)
    }

    // MARK: Lines

    @ViewBuilder private var content: some View {
        if let loading = copy.text(transcript.loadingMessage) {
            HStack { ProgressView(); Text(loading).foregroundStyle(tokens.muted) }
        } else if let error = copy.text(transcript.loadError) {
            Banner(text: [error, transcript.loadErrorDetail].compactMap { $0 }.joined(separator: "\n"), kind: .error)
        } else if let empty = copy.text(transcript.empty) {
            Text(empty).foregroundStyle(tokens.muted)
        } else {
            let fades = Dictionary(uniqueKeysWithValues: transcript.fades.map { (Int($0.row), Double($0.opacityPercent) / 100) })
            let activeIndex = Formatting.activeLine(ends: transcript.rows.map(\.endSeconds), time: snapshot.time)
            VStack(alignment: .leading, spacing: 10) {
                ForEach(Array(transcript.rows.enumerated()), id: \.element.id) { index, row in
                    line(row, active: index == activeIndex).opacity(fades[index] ?? 1)
                }
            }
            .environment(\.layoutDirection, transcript.rtl ? .rightToLeft : .leftToRight)
        }
    }

    @ViewBuilder private func line(_ row: LineRow, active: Bool) -> some View {
        if row.editing {
            VStack(alignment: .leading, spacing: 8) {
                TextField(copy.text(Labels.editLine), text: $draft, axis: .vertical)
                    .padding(10).background(tokens.surface, in: RoundedRectangle(cornerRadius: 8))
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(tokens.focus))
                    .onAppear { draft = row.text }
                HStack {
                    SecondaryButton(title: copy.text(Labels.cancel)) { act(.editCanceled) }
                    PrimaryButton(title: copy.text(Labels.saveLine)) { act(.editSubmitted(text: draft)) }
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    if transcript.showTime { Text(row.timeText).font(.caption.monospacedDigit()).foregroundStyle(tokens.muted) }
                    if transcript.showSpeakers, let label = row.speaker { speakerControl(row, label) }
                    Spacer()
                    Button { act(.editStarted(line: row.id)) } label: { Image(systemName: "pencil") }
                        .foregroundStyle(tokens.muted).frame(minWidth: 44, minHeight: 44).accessibilityLabel(copy.text(Labels.editLine))
                }
                Text(row.text).frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                    .onTapGesture { tapped(row) }
            }
            .padding(6)
            .background(active ? tokens.cursorBg : Color.clear, in: RoundedRectangle(cornerRadius: 8))
            .accessibilityElement(children: .contain)
            .accessibilityAddTraits(active ? .isSelected : [])
        }
    }

    private func tapped(_ row: LineRow) {
        if case .youTube(let id) = transcript.media { openURL(youTubeURL(id, seconds: Int(row.startSeconds))) } else { act(.timeClicked(line: row.id)) }
    }

    private func speakerControl(_ row: LineRow, _ label: SpeakerLabel) -> some View {
        Menu {
            ForEach(transcript.speakerOptions, id: \.value) { option in
                Button(speakerText(option.label)) { act(.speakerChosen(line: row.id, speaker: option.value)) }
            }
            if let value = row.speakerValue {
                Button(copy.text(Labels.editSpeaker)) { renaming = value; newName = value }
            }
        } label: {
            HStack(spacing: 4) {
                Text(speakerText(label)).font(.caption.weight(.semibold))
                if row.speakerPending { ProgressView().scaleEffect(0.6) } else { Image(systemName: "chevron.down").font(.caption2) }
            }
            .padding(.horizontal, 8).padding(.vertical, 4)
            .overlay(Capsule().stroke(tokens.border))
        }
    }

    /// Core's speaker label as text: "Speaker N", the stored name, or "N/A" (EDT-004).
    private func speakerText(_ label: SpeakerLabel) -> String {
        switch label {
        case .numbered(let number): return "\(copy.text(Labels.speaker)) \(number)"
        case .named(let name): return name
        case .none: return copy.text(Labels.notAvailableShort)
        }
    }

    private func freemiumBar(_ banner: FreemiumBanner) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            ProgressView(value: Double(banner.percent), total: 100).tint(tokens.progress)
            Text("\(banner.currentMinutes) / \(banner.maxMinutes) \(copy.text(Labels.minShort))").font(.caption).foregroundStyle(tokens.muted)
        }
    }
}

private func youTubeURL(_ id: String, seconds: Int) -> URL {
    var components = URLComponents(string: "https://www.youtube.com/watch")!
    components.queryItems = [URLQueryItem(name: "v", value: id)] + (seconds > 0 ? [URLQueryItem(name: "t", value: "\(seconds)s")] : [])
    return components.url!
}

/// The mounted audio player. What plays and when it pauses or seeks is core's (through the
/// player port); the player's clock and play state are reported up, the intent only on a change.
struct PlayerBar: View {
    let source: URL
    let player: AVPlayerControl
    let playLabel: String
    let pauseLabel: String
    @Binding var snapshot: PlayerSnapshot
    let onState: (Bool) -> Void

    var body: some View {
        HStack {
            Button { player.toggle() } label: {
                Image(systemName: snapshot.playing ? "pause.circle.fill" : "play.circle.fill").font(.largeTitle).frame(minWidth: 44, minHeight: 44)
            }
            .accessibilityLabel(snapshot.playing ? pauseLabel : playLabel)
            Text(Formatting.clockTime(snapshot.time)).font(.body.monospacedDigit()).foregroundStyle(tokens.muted)
            Spacer()
        }
        .onAppear { mount(source) }
        .onChange(of: source) { _, new in mount(new) }
        .onDisappear { player.mount(nil); snapshot = PlayerSnapshot(time: 0, playing: false) }
    }

    private func mount(_ url: URL) {
        player.mount(url) { next in
            Task { @MainActor in
                let wasPlaying = snapshot.playing
                snapshot = next
                if next.playing != wasPlaying { onState(next.playing) }
            }
        }
    }
}
