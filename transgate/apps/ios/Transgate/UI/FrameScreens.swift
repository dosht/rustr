import SwiftUI

/// The signed-in phone frame (WEB-052..076): a header with the recent-transcripts drawer
/// and the account sheet, and one content screen chosen by core's route. Core decides the
/// route, title, balance, every dialog's content and which dialogs are open. The shell does
/// not render purchases, invoices or API keys: balance is shown as text, and billing and
/// key administration open in the browser (D6).
struct SignedInFrame: View {
    let vm: SignedInVm
    let model: AppModel
    let settings: AppSettings
    let player: AVPlayerControl
    let openURL: (URL) -> Void

    @State private var recent = false
    @State private var account = false
    @State private var offerDismissed = false

    private var copy: Copy { model.copy }
    private func send(_ intent: AppIntent) { model.send(intent) }

    var body: some View {
        VStack(spacing: 0) {
            header
            if let error = copy.text(vm.sessionError) {
                Banner(text: error, kind: .error).padding(8)
                SecondaryButton(title: copy.text(Labels.tryAgain)) { send(.sessionRetried) }.padding(.horizontal, 8)
            }
            if let offer = vm.specialClientAlert, !offerDismissed { SpecialOfferCard(offer: offer, copy: copy) { offerDismissed = true }.padding(8) }
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .screenBackground()
        .sheet(isPresented: $recent) { RecentSheet(vm: vm, copy: copy, send: send) { recent = false } }
        .sheet(isPresented: $account) { accountSheet }
        .sheet(isPresented: coreFlag(vm.settings?.open == true) { if !$0 { send(.settings(.closed)) } }) {
            if let settings = vm.settings { SettingsSheet(frame: vm, settings: settings, copy: copy, send: send, openURL: openURL, handoffs: self.settings) }
        }
        .sheet(isPresented: coreFlag(vm.feedback.open) { if !$0 { send(.feedback(.closed)) } }) { FeedbackSheet(vm: vm.feedback, copy: copy, send: send) }
        .sheet(isPresented: coreFlag(vm.support.open) { if !$0 { send(.support(.closed)) } }) { SupportSheet(vm: vm.support, copy: copy, send: send) }
    }

    private var header: some View {
        HStack {
            Button { recent = true } label: { Image(systemName: "line.3.horizontal").frame(minWidth: 44, minHeight: 44) }
                .accessibilityLabel(copy.text(Labels.menu))
            Text(vm.headerTitleText ?? copy.text(vm.headerTitle)).font(.headline).lineLimit(1).frame(maxWidth: .infinity)
            Button { account = true } label: { Image(systemName: "ellipsis.circle").frame(minWidth: 44, minHeight: 44) }
                .accessibilityLabel(copy.text(Labels.moreOptions))
        }
        .padding(.horizontal, 8)
        .background(tokens.surface)
    }

    @ViewBuilder private var content: some View {
        switch vm.route {
        case .home(.allTranscripts):
            HistoryScreen(vm: vm.jobs, copy: copy, send: send) { send(.navigate(route: vm.newTranscriptRoute)) }
        case .home(.job):
            if let transcript = vm.transcript {
                TranscriptScreen(vm: vm, transcript: transcript, copy: copy, player: player, send: send, openURL: openURL)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).accessibilityLabel(copy.text(Labels.loading))
            }
        case .newPassword:
            NewPasswordScreen(vm: vm.newPassword, copy: copy, send: send)
        case .billing:
            HandoffScreen(title: copy.text(Labels.billingHistory), button: copy.text(Labels.billingHistory), url: settings.webPage(.billing, locale: vm.locale), copy: copy, openURL: openURL) {
                send(.navigate(route: vm.newTranscriptRoute))
            }
        case .apiKeys:
            HandoffScreen(title: copy.text(Labels.apiKeys), button: copy.text(Labels.apiKeys), url: settings.webPage(.apiKeys, locale: vm.locale), copy: copy, openURL: openURL) {
                send(.navigate(route: vm.newTranscriptRoute))
            }
        default:
            NewTranscriptScreen(vm: vm.queue, youtube: vm.capabilities.youtube, copy: copy, send: send) { model.picked($0) }
        }
    }

    private var accountSheet: some View {
        NavigationStack {
            List {
                Section {
                    VStack(alignment: .leading) {
                        Text(vm.profile?.displayName ?? "").font(.headline)
                        Text(vm.profile?.email ?? "").font(.subheadline).foregroundStyle(tokens.muted)
                    }
                    // Balance only (D6): text, no purchase action.
                    HStack { Text(copy.text(Labels.balance)); Spacer(); Text(copy.text(vm.balance)).foregroundStyle(tokens.muted) }
                }
                Section {
                    Button(copy.text(Labels.support)) { account = false; send(.support(.opened)) }
                    Button(copy.text(Labels.settings)) { account = false; send(.settings(.opened)) }
                    if let url = settings.webPage(.billing, locale: vm.locale), vm.capabilities.billingHistory {
                        Button(copy.text(Labels.billingHistory)) { account = false; openURL(url) }
                    }
                    if let url = settings.webPage(.apiKeys, locale: vm.locale) { Button(copy.text(Labels.apiKeys)) { account = false; openURL(url) } }
                }
                Section {
                    LocaleMenu(locales: vm.locales, copy: copy, send: send)
                }
                Section {
                    Button(copy.text(Labels.logout), role: .destructive) { account = false; send(.signOutRequested) }.disabled(vm.signingOut)
                }
            }
            .navigationTitle(copy.text(Labels.account))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(copy.text(Labels.close)) { account = false } } }
        }
        .presentationDetents([.medium, .large])
    }
}

/// A web-only screen (D6): the app shows a way out to the browser instead of the screen.
struct HandoffScreen: View {
    let title: String
    let button: String
    let url: URL?
    let copy: Copy
    let openURL: (URL) -> Void
    let back: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Text(title).font(.headline)
            if let url { PrimaryButton(title: button) { openURL(url) } }
            SecondaryButton(title: copy.text(Labels.newTranscript), action: back)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .screenBackground()
    }
}

// MARK: - Settings (WEB-074)

struct SettingsSheet: View {
    let frame: SignedInVm
    let settings: SettingsVm
    let copy: Copy
    let send: (AppIntent) -> Void
    let openURL: (URL) -> Void
    let handoffs: AppSettings

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                Picker("", selection: coreTab(settings.active) { send(.settings(.tabSelected(tab: $0))) }) {
                    ForEach(settings.tabs, id: \.tab) { Text(copy.text($0.label)).tag($0.tab) }
                }
                .pickerStyle(.segmented).padding(12)
                ScrollView { tab.padding(16) }
            }
            .navigationTitle(copy.text(settings.title))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(copy.text(Labels.close)) { send(.settings(.closed)) } } }
            .screenBackground()
        }
    }

    private func coreTab(_ value: Tab, _ set: @escaping (Tab) -> Void) -> Binding<Tab> { Binding(get: { value }, set: set) }

    @ViewBuilder private var tab: some View {
        switch settings.active {
        case .profile:
            VStack(alignment: .leading, spacing: 8) {
                Text(settings.profile.displayName).font(.headline)
                Text(settings.profile.email).foregroundStyle(tokens.muted)
                if let joined = frame.joinedOnText { Text(joined).font(.caption).foregroundStyle(tokens.muted) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        case .aiFeatures:
            AIFeaturesSettings(vm: frame.consent, copy: copy, send: send)
        case .subscription:
            VStack(alignment: .leading, spacing: 12) {
                if frame.subscription.loading { ProgressView().accessibilityLabel(copy.text(Labels.loading)) }
                if let plan = frame.subscription.plan {
                    Text(plan.name).font(.headline)
                    Text(copy.text(Labels.minutes(plan.minutesIncluded))).foregroundStyle(tokens.muted)
                }
                if let url = handoffs.webPage(.billing, locale: frame.locale) { SecondaryButton(title: copy.text(Labels.billingHistory)) { openURL(url) } }
            }.frame(maxWidth: .infinity, alignment: .leading)
        case .manageData:
            ManageDataPanel(vm: frame.manageData, copy: copy, send: send)
        }
    }
}

struct AIFeaturesSettings: View {
    let vm: ConsentPanelVm
    let copy: Copy
    let send: (AppIntent) -> Void
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(copy.text(Labels.aiConsentTitle)).font(.headline)
            if vm.gate == .loading { ProgressView().accessibilityLabel(copy.text(Labels.loading)) }
            if let provider = vm.providerName { Text("\(copy.text(Labels.aiProvider)) \(provider)") }
            if vm.cleanupPending { Banner(text: copy.text(Labels.cleanupPending)) }
            if let error = copy.text(vm.loadError) ?? copy.text(vm.actionError) {
                Banner(text: error, kind: .error, dismissLabel: copy.text(Labels.dismiss)) { send(.consent(.errorDismissed)) }
            }
            if let url = URL(string: vm.privacyPolicyUrl) { Link(copy.text(Labels.aiPrivacy), destination: url).frame(minHeight: 44) }
            if vm.hasConsented {
                if vm.canRevoke { SecondaryButton(title: copy.text(Labels.turnOffConfirm), destructive: true, enabled: vm.busy == nil) { confirming = true } }
            } else if vm.gate != .loading {
                PrimaryButton(title: copy.text(Labels.aiAccept), busy: vm.busy == .enable) { send(.consent(.enableRequested)) }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .confirmationDialog(copy.text(Labels.turnOffTitle), isPresented: $confirming, titleVisibility: .visible) {
            Button(copy.text(Labels.turnOffConfirm), role: .destructive) { send(.consent(.disableConfirmed)) }
            Button(copy.text(Labels.cancel), role: .cancel) {}
        } message: { Text(copy.text(Labels.turnOffBody)) }
    }
}

struct ManageDataPanel: View {
    let vm: DataManagementVm
    let copy: Copy
    let send: (AppIntent) -> Void

    private func act(_ intent: ManageDataIntent) { send(.manageData(intent)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(copy.text(Labels.privacyInformation)).font(.subheadline.weight(.semibold))
            ForEach(Array(vm.privacyInfo.enumerated()), id: \.offset) { _, line in Text("• " + copy.text(line)).font(.subheadline) }
            Text(copy.text(Labels.dataRetention)).font(.headline)
            Text(copy.text(vm.cleanupPolicy)).font(.subheadline)
            if let warning = copy.text(vm.cleanupWarning) { Text(warning).font(.subheadline).foregroundStyle(tokens.error) }
            Toggle(copy.text(Labels.cleanupSwitch), isOn: coreFlag(vm.cleanupOn) { act(.cleanupToggled(on: $0)) })
            Text(copy.text(vm.cleanupOn ? Labels.cleanupHintOn : Labels.cleanupHintOff)).font(.caption).foregroundStyle(tokens.muted)
            Toggle(copy.text(Labels.deleteMonthly), isOn: coreFlag(vm.deleteMonthlyOn) { act(.deleteMonthlyToggled(on: $0)) })
            Text(copy.text(Labels.manualActions)).font(.headline)
            SecondaryButton(title: copy.text(Labels.deleteAll), destructive: true) { act(.deleteAllPressed) }
            SecondaryButton(title: copy.text(Labels.deleteAccount), destructive: true) { act(.deleteAccountPressed) }
            Text(copy.text(vm.filesGoneNote)).font(.caption).foregroundStyle(tokens.error)
            if let erasure = copy.text(vm.erasure) { Banner(text: erasure) }
            if let error = copy.text(vm.error) { Banner(text: error, kind: .error) }
            if let error = copy.text(vm.saveError) { Banner(text: error, kind: .error) }
        }
        .tint(tokens.accent)
        .frame(maxWidth: .infinity, alignment: .leading)
        .confirmationDialog(vm.confirm.map { copy.text($0.title) } ?? "", isPresented: coreFlag(vm.confirm != nil) { if !$0 { act(.canceled) } }, titleVisibility: .visible) {
            if let confirm = vm.confirm {
                Button(copy.text(confirm.confirmLabel), role: confirm.kind == .disableCleanup ? nil : .destructive) { act(.confirmed) }
            }
            Button(copy.text(Labels.no), role: .cancel) { act(.canceled) }
        } message: { Text(vm.confirm.map { copy.text($0.description) } ?? "") }
    }
}

// MARK: - Feedback and support (WEB-076)

struct FeedbackSheet: View {
    let vm: FeedbackVm
    let copy: Copy
    let send: (AppIntent) -> Void

    private func act(_ intent: FeedbackIntent) { send(.feedback(intent)) }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 14) {
                if vm.thanked {
                    Text(copy.text(Labels.feedbackThanksTitle)).font(.headline)
                    Text(copy.text(Labels.feedbackThanks))
                } else {
                    Text(copy.text(Labels.feedbackPrompt)).font(.subheadline)
                    ForEach(vm.options, id: \.rating) { option in
                        Button { act(.selected(rating: option.rating)) } label: {
                            HStack { Text(copy.text(option.label)); Spacer(); if option.selected { Image(systemName: "checkmark") } }
                                .frame(minHeight: 44)
                        }
                        .accessibilityAddTraits(option.selected ? .isSelected : [])
                    }
                    LabeledField(label: copy.text(Labels.feedbackPlaceholder), text: coreText(vm.comment) { act(.commentChanged(text: $0)) }, multiline: true, enabled: !vm.submitting)
                    if let notice = copy.text(vm.notice) { Banner(text: notice) }
                    PrimaryButton(title: copy.text(vm.submitting ? Labels.submitting : Labels.submit), busy: vm.submitting, enabled: vm.canSubmit) { act(.submitted) }
                }
                Spacer()
            }
            .padding(20)
            .navigationTitle(copy.text(Labels.feedbackTitle))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(copy.text(Labels.close)) { act(.closed) } } }
            .screenBackground()
        }
        .presentationDetents([.large])
    }
}

struct SupportSheet: View {
    let vm: SupportVm
    let copy: Copy
    let send: (AppIntent) -> Void

    private func act(_ intent: SupportIntent) { send(.support(intent)) }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 14) {
                LabeledField(label: copy.text(Labels.supportMessage), text: coreText(vm.message) { act(.messageChanged(text: $0)) }, multiline: true, enabled: !vm.sending)
                if let notice = copy.text(vm.notice) { Banner(text: notice) }
                PrimaryButton(title: copy.text(Labels.send), busy: vm.sending) { act(.submitted) }
                Spacer()
            }
            .padding(20)
            .navigationTitle(copy.text(Labels.supportTitle))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(copy.text(Labels.close)) { act(.closed) } } }
            .screenBackground()
        }
        .presentationDetents([.medium, .large])
    }
}

/// The configured special offer (source SpecialOffer): message and code are host data shown
/// as given. Dismissal is local to the frame; copying the code is the system clipboard.
struct SpecialOfferCard: View {
    let offer: SpecialClientAlert
    let copy: Copy
    let dismiss: () -> Void
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(copy.text(Labels.specialOffer)).font(.headline)
                Spacer()
                Button(action: dismiss) { Image(systemName: "xmark").frame(minWidth: 44, minHeight: 44) }.accessibilityLabel(copy.text(Labels.close))
            }
            Text(offer.message)
            HStack {
                Text(offer.code).font(.body.monospaced()).textSelection(.enabled).padding(8).overlay(RoundedRectangle(cornerRadius: 6).stroke(tokens.border))
                Button(copy.text(copied ? Labels.copied : Labels.copyCode)) { UIPasteboard.general.string = offer.code; copied = true }.buttonStyle(.bordered)
            }
            Text(copy.text(Labels.useCode)).font(.caption).foregroundStyle(tokens.muted)
        }
        .padding(12)
        .background(tokens.noticeBg, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tokens.border))
    }
}
