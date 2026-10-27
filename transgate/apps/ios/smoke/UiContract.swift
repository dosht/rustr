// The SwiftUI layer needs Xcode, so Linux cannot compile it. What can break there without a
// Mac is the shell's use of the generated bindings: an intent case or a view-model field that
// the UI names but core no longer has. This file names every intent the UI sends and every
// field and case it reads, so check-swift.sh turns such drift into a Linux compile error.
// It is compiled, and the constructors run once by `uiContract()`; it is not behavior.
import Foundation

func everyIntentTheUiSends() -> [AppIntent] {
    let job: JobId = 1
    let line: LineId = 2
    let tab = LanguageTab(language: "en", translationStatus: .original)
    return [
        .started(url: "/home", viewport: Viewport(touch: true, width: 390), utcOffsetMinutes: 0),
        .viewportChanged(viewport: Viewport(touch: true, width: 390)),
        .visibilityChanged(visible: true),
        .signOutRequested, .sessionRetried,
        .navigate(route: .home(.job(job))), .navigate(route: .home(.allTranscripts)), .navigate(route: .home(.new)),
        .auth(.googleChosen), .auth(.emailChanged(text: "a")), .auth(.passwordChanged(text: "b")), .auth(.submitted),
        .auth(.modeChosen(mode: .magicLink)),
        .newPassword(.passwordChanged(text: "x")), .newPassword(.submitted),
        .upload(.picked(files: [PickedFile(handle: 1, name: "a.mp3", size: 1, mime: "audio/mpeg")])),
        .upload(.removed(file: 1)), .upload(.tabChanged(mode: .translation)), .upload(.speakersToggled(enabled: true)),
        .upload(.startPressed), .upload(.dismissAlert(alert: .balance)), .upload(.youTubeDialogOpened), .upload(.youTubeDialogClosed),
        .upload(.youTubeUrlChanged(url: "u")), .upload(.youTubeSubmitted),
        .upload(.languageChosen(choice: .autoDetect)), .upload(.languageChosen(choice: .language("en"))),
        .upload(.sourceLanguageChosen(language: nil)), .upload(.targetLanguageChosen(language: "de")),
        .jobs(.refresh), .jobs(.loadMore), .jobs(.deleteRequested(job: job)), .jobs(.deleteConfirmed), .jobs(.deleteCanceled),
        .jobs(.downloadRequested(job: job)),
        .feedback(.opened(job: job)), .feedback(.closed), .feedback(.selected(rating: .satisfied)), .feedback(.commentChanged(text: "c")),
        .feedback(.submitted),
        .transcript(.tabSelected(tab: tab)), .transcript(.editStarted(line: line)), .transcript(.editCanceled),
        .transcript(.editSubmitted(text: "t")), .transcript(.speakerChosen(line: line, speaker: "s")),
        .transcript(.speakerRenamed(from: "a", to: "b")), .transcript(.showTimeToggled(on: true)),
        .transcript(.showSpeakersToggled(on: true)), .transcript(.timeClicked(line: line)),
        .transcript(.downloadPressed(allFormats: true)), .transcript(.copyPressed), .transcript(.playerState(playing: true)),
        .transcript(.noAudioAlertDismissed),
        .consent(.enableRequested), .consent(.declineRequested), .consent(.disableConfirmed), .consent(.errorDismissed),
        .analysis(.shown), .analysis(.hidden), .analysis(.retryRequested), .analysis(.copyRequested),
        .analysis(.exportRequested(format: .pdf)), .analysis(.exportErrorDismissed),
        .chat(.sendRequested(text: "q")), .chat(.retryRequested(index: 0)), .chat(.copyMessageRequested(index: 0)),
        .chat(.rateRequested(index: 0, rating: .like)), .chat(.copyChatRequested), .chat(.downloadRequested),
        .chat(.errorDismissed), .chat(.noticeDismissed),
        .settings(.opened), .settings(.closed), .settings(.tabSelected(tab: .manageData)),
        .manageData(.cleanupToggled(on: true)), .manageData(.deleteMonthlyToggled(on: true)), .manageData(.deleteAllPressed),
        .manageData(.deleteAccountPressed), .manageData(.confirmed), .manageData(.canceled),
        .support(.opened), .support(.closed), .support(.messageChanged(text: "m")), .support(.submitted),
    ]
}

/// Reads the fields and cases the UI uses. Never called; compiling it is the check.
func everyFieldTheUiReads(_ app: AppVm, _ vm: SignedInVm, _ out: SignedOutVm) -> Int {
    var count = 0
    let transcript = vm.transcript
    count += vm.headerTitleText?.count ?? 0
    count += vm.profile?.displayName.count ?? 0
    count += (vm.profile?.email.count ?? 0) + vm.capabilities.youtube.hashValue + vm.capabilities.billingHistory.hashValue
    count += vm.signingOut.hashValue + (vm.sessionError == nil ? 0 : 1) + (vm.joinedOnText?.count ?? 0)
    count += vm.consent.hasConsented.hashValue + vm.consent.canRevoke.hashValue + vm.consent.cleanupPending.hashValue
    count += (vm.consent.providerName?.count ?? 0) + vm.consent.privacyPolicyUrl.count + (vm.consent.busy == .enable ? 1 : 0)
    count += vm.consent.gate == .required ? 1 : 0
    count += vm.manageData.cleanupOn.hashValue + vm.manageData.deleteMonthlyOn.hashValue + vm.manageData.privacyInfo.count
    count += (vm.manageData.confirm?.kind == .disableCleanup ? 1 : 0) + (vm.manageData.saveError == nil ? 0 : 1)
    count += vm.subscription.loading.hashValue + Int(vm.subscription.plan?.minutesIncluded ?? 0) + (vm.subscription.plan?.name.count ?? 0)
    count += vm.feedback.options.count + vm.feedback.comment.count + vm.feedback.thanked.hashValue + vm.feedback.canSubmit.hashValue
    count += vm.feedback.submitting.hashValue + vm.feedback.open.hashValue
    count += vm.support.message.count + vm.support.sending.hashValue + vm.support.open.hashValue
    count += (vm.settings?.open == true ? 1 : 0) + (vm.settings?.tabs.count ?? 0) + (vm.settings?.profile.email.count ?? 0)
    count += vm.jobs.rows.count + vm.jobs.recent.count + vm.jobs.canLoadMore.hashValue + (vm.jobs.confirmDelete == nil ? 0 : 1)
    for row in vm.jobs.rows {
        count += row.id.hashValue + (row.displayName?.count ?? 0) + row.tone.hashValue + row.spinner.hashValue + row.isTranslation.hashValue
        count += row.canDownload.hashValue + row.canFeedback.hashValue + row.downloading.hashValue + row.dateText.count
        count += (row.duration?.count ?? 0) + (row.language?.count ?? 0) + (row.flag?.count ?? 0)
    }
    for row in vm.jobs.recent { count += row.id.hashValue + (row.fileName?.count ?? 0) + (row.duration?.count ?? 0) + row.selected.hashValue }
    for row in vm.queue.rows {
        count += row.name.count + row.phoneSize.count + Int(row.id) + (row.durationText == nil ? 0 : 1)
        if case .uploading(let progress) = row.chip { count += Int(progress) }
    }
    count += vm.queue.alerts.count + vm.queue.pickerAccept.count + vm.queue.maxFilesReached.hashValue + vm.queue.startDisabled.hashValue
    count += vm.queue.transcribing.hashValue + vm.queue.filesReady.hashValue + vm.queue.initializing.hashValue + vm.queue.speakers.hashValue
    count += vm.queue.available.count + vm.queue.sourceLanguages.count + vm.queue.targetLanguages.count + vm.queue.phonePreferred.count
    count += vm.queue.preferredTargets.count + vm.queue.autoDetectOffered.hashValue + (vm.queue.sourceLanguage?.count ?? 0)
    count += (vm.queue.targetLanguage?.count ?? 0) + (vm.queue.mode == .translation ? 1 : 0)
    count += vm.queue.youtubeDialog.url.count + vm.queue.youtubeDialog.busy.hashValue + vm.queue.youtubeDialog.open.hashValue
    count += vm.queue.youtubeError == nil ? 0 : 1
    if case .language(let code)? = vm.queue.language { count += code.count }
    if let t = transcript {
        count += t.title.count + Int(t.durationMinutes ?? 0) + t.canCopy.hashValue + t.downloadEnabled.hashValue + t.showTabs.hashValue
        count += t.tabs.count + t.rtl.hashValue + t.showTime.hashValue + t.showSpeakers.hashValue + t.timeToggle.hashValue
        count += t.phoneSpeakerToggle.hashValue + t.speakerOptions.count + (t.freemium?.percent == nil ? 0 : 1) + (t.job == nil ? 0 : 1)
        count += (t.selected == nil ? 0 : 1) + (t.translationFlag?.count ?? 0) + (t.loadErrorDetail?.count ?? 0)
        count += (t.statusBadge?.spinning == true ? 1 : 0) + t.fades.reduce(0) { $0 + Int($1.row) + Int($1.opacityPercent) }
        for option in t.tabs { count += option.name.count + option.flag.count + (option.tab == t.selected ? 1 : 0) }
        for option in t.speakerOptions { count += option.value.count; _ = option.label }
        for row in t.rows {
            count += row.id.hashValue + row.timeText.count + row.text.count + Int(row.startSeconds) + (row.speakerValue?.count ?? 0)
            count += row.speakerPending.hashValue + row.editing.hashValue
            switch row.speaker {
            case .numbered(let number)?: count += Int(number)
            case .named(let name)?: count += name.count
            case SpeakerLabel.none?: count += 1
            case nil: break
            }
        }
        switch t.media {
        case .none: break
        case .audio(let url, let loading, let unavailable): count += (url?.count ?? 0) + loading.hashValue + unavailable.hashValue
        case .youTube(let id): count += id.count
        }
    }
    if let analysis = vm.analysis {
        count += analysis.hasData.hashValue + analysis.noTranscript.hashValue + analysis.canRetry.hashValue + (analysis.summary?.count ?? 0)
        count += Int(analysis.progress?.percentage ?? 0) + (analysis.exportBusy == nil ? 0 : 1) + (analysis.errorDetail?.count ?? 0)
        switch analysis.state { case .idle, .checkingCache, .loading, .ready, .interrupted, .cancelled, .error: break }
        if case .generic(let generic)? = analysis.analysis { count += generic.highlights?.count ?? 0 }
    }
    if let chat = vm.chat {
        count += chat.enabled.hashValue + chat.inputLocked.hashValue + chat.loadingHistory.hashValue + chat.showWelcome.hashValue
        count += chat.typing.hashValue + chat.canSend.hashValue + (chat.error?.detail?.count ?? 0)
        for line in chat.lines { count += (line.role == .user ? 1 : 0) + line.content.count + line.showActions.hashValue + line.streaming.hashValue }
    }
    switch vm.route {
    case .home(.allTranscripts), .home(.job), .home(.new), .home(.default), .newPassword, .billing, .apiKeys, .signIn, .checkoutConfirm: break
    }
    switch app {
    case .restoring, .checkoutConfirm, .signedOut, .signedIn, .erased: break
    }
    count += out.auth.email.count + out.auth.password.count + out.auth.busy.hashValue + out.auth.canSubmit.hashValue
    count += (out.route == .newPassword ? 1 : 0) + out.newPassword.password.count + out.newPassword.busy.hashValue
    return count
}

func uiContract() {
    let intents = everyIntentTheUiSends()
    check(intents.count > 70, "the UI sends \(intents.count) intents")
    pass("ui-contract-intents-and-fields-compile")
}
