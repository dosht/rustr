import Foundation

/// A view-owned static label: a catalog key the shell asks for itself (the web's
/// `i18n.tk`). Everything dynamic comes from core as a `Msg`. Copy is only ever
/// produced by `translate()`; the smoke test translates every label below in all
/// sixteen locales, so a missing key is found on Linux, not on a device.
struct Label: Hashable, Sendable {
    let ns: Ns
    let key: String
    let args: [String: ArgWire]

    init(_ ns: Ns, _ key: String, args: [String: ArgWire] = [:]) {
        self.ns = ns
        self.key = key
        self.args = args
    }

    var message: Msg { Msg(ns: ns, key: key, args: args) }
}

extension Copy {
    func text(_ label: Label) -> String { text(label.message) }
}

enum Labels {
    // MARK: Auth (source Supabase Auth UI keys, `auth` namespace, by mode)

    static func modePrefix(_ mode: AuthMode) -> String {
        switch mode {
        case .signIn: return "sign_in"
        case .signUp: return "sign_up"
        case .magicLink: return "magic_link"
        case .forgottenPassword: return "forgotten_password"
        }
    }

    static func auth(_ mode: AuthMode, _ suffix: String, args: [String: ArgWire] = [:]) -> Label {
        Label(.auth, "\(modePrefix(mode)).\(suffix)", args: args)
    }

    static func emailLabel(_ mode: AuthMode) -> Label {
        mode == .magicLink ? auth(mode, "email_input_label") : auth(mode, "email_label")
    }

    static func socialLabel(_ mode: AuthMode) -> Label { auth(mode, "social_provider_text", args: ["provider": .text(value: "Google")]) }

    static let newPassword = Label(.common, "New Password")
    static let changePassword = Label(.common, "Change Password")

    // MARK: Upload

    static let uploadFiles = Label(.common, "Upload files")
    static let addFromYouTube = Label(.common, "Add from YouTube")
    static let files = Label(.common, "Files")
    static let noFiles = Label(.common, "No files added yet")
    static let noFilesHint = Label(.common, "Upload audio files or record directly to get started")
    static let totalDuration = Label(.common, "Total Duration:")
    static let calculating = Label(.common, "Calculating...")
    static let translation = Label(.common, "Translation")
    static let audioLanguage = Label(.common, "Audio Language")
    static let translationLanguage = Label(.common, "Translation Language")
    static let speakerRecognition = Label(.common, "Speaker Recognition")
    static let speakerRecognitionHint = Label(.common, "Identify different speakers in the transcript")
    static let chooseLanguage = Label(.common, "Choose the language")
    static let searchLanguages = Label(.common, "Search languages...")
    static let noLanguages = Label(.common, "No languages found")
    static let starting = Label(.common, "Starting...")
    static let startTranslating = Label(.common, "Start Translating")
    static let startTranscribing = Label(.common, "Start Transcribing")
    static let preparingFiles = Label(.common, "Preparing files...")
    static let loading = Label(.common, "Loading...")
    static let ok = Label(.common, "OK")
    static let cancel = Label(.common, "Cancel")
    static let close = Label(.common, "Close")
    static let yes = Label(.common, "Yes")
    static let no = Label(.common, "No")
    static let notEnoughBalance = Label(.common, "Not enough balance.")
    static let notEnoughBalanceBody = Label(.common, "Your balance is not enough to run this job. Please top up your minutes and try again.")
    static let languageRequired = Label(.common, "Language Selection Required")
    static let languageRequiredBody = Label(.common, "Please select the audio language before starting transcription.")
    static let processingTimeout = Label(.common, "Processing Timeout")
    static let processingTimeoutBody = Label(.common, "File preparation took longer than expected. This could be due to file processing issues or upload problems.")
    static let processingTimeoutHint = Label(.common, "Please try uploading the file again. If the problem persists, contact support.")
    static let youTubeFailed = Label(.common, "YouTube Transcription Failed")
    static let youTubeTitle = Label(.common, "Add YouTube Video")
    static let youTubeBody = Label(.common, "Paste a YouTube link to transcribe. Videos with available captions are transcribed instantly.")
    static let youTubeUrl = Label(.common, "YouTube URL")
    static let adding = Label(.common, "Adding...")
    static let speaker = Label(.common, "Speaker")
    static let play = Label(.app, "p5.player.play")
    static let pause = Label(.app, "p5.player.pause")
    static let copyMessage = Label(.ai, "Copy message")
    static let helpful = Label(.ai, "Helpful")
    static let notHelpful = Label(.ai, "Not helpful")
    static let notAvailableShort = Label(.app, "N/A")
    static let minShort = Label(.common, "min")
    static let addVideo = Label(.common, "Add Video")
    static func minutes(_ count: Int64) -> Label { Label(.app, "p5.duration.minutes", args: ["minutes": .number(value: count)]) }

    static let configUnavailable = Label(.app, "p5.web.config_unavailable")
    // MARK: History and navigation

    static let delete = Label(.app, "Delete")
    static let loadingJobs = Label(.app, "Loading jobs...")
    static let loadingMore = Label(.app, "Loading more...")
    static let newTranscription = Label(.app, "New Transcription")
    static let noTranscripts = Label(.app, "No transcripts found")
    static let noTranscriptsHint = Label(.app, "Start by creating your first transcript")
    static let tryAgain = Label(.app, "Try Again")
    static let deleteFileTitle = Label(.common, "Are you sure you want to delete this file?")
    static let deleteFileBody = Label(.common, "Note that you can't access this file again.")
    static let allTranscripts = Label(.common, "mobile.navigation.allTranscripts")
    static let newTranscript = Label(.common, "mobile.navigation.newTranscript")
    static let recent = Label(.common, "mobile.navigation.recentSection")
    static let menu = Label(.app, "Main navigation menu")
    static let moreOptions = Label(.app, "More options")
    static let untitled = Label(.app, "Untitled")

    // MARK: Account sheet (balance only, D6) and browser handoffs

    static let account = Label(.common, "mobile.navigation.bottomSheet.account")
    static let balance = Label(.common, "mobile.navigation.bottomSheet.balance")
    static let support = Label(.common, "mobile.navigation.bottomSheet.support")
    static let settings = Label(.common, "mobile.navigation.bottomSheet.title")
    static let logout = Label(.common, "mobile.navigation.bottomSheet.logout")
    static let billingHistory = Label(.common, "Billing History")
    static let apiKeys = Label(.common, "API Keys")

    // MARK: Transcript

    static let showTime = Label(.app, "Show Time")
    static let showSpeakers = Label(.app, "Show Speaker")
    static let copyAll = Label(.app, "Copy all transcripts")
    static let download = Label(.app, "Download Transcript")
    static let downloadAll = Label(.common, "Download all formats")
    static let feedback = Label(.app, "Feedback")
    static let editLine = Label(.app, "p5.transcript.edit_line")
    static let saveLine = Label(.app, "p5.transcript.save_line")
    static let editSpeaker = Label(.common, "Edit speaker")
    static let audioUnavailable = Label(.app, "Audio is not available.")

    // MARK: AI (consent, analysis, chat)

    static let aiAnalysis = Label(.ai, "AI Analysis")
    static let askAi = Label(.ai, "Ask AI")
    static let aiConsentTitle = Label(.ai, "AI Features Consent")
    static let aiFeatures = Label(.ai, "AI Features include:")
    static let aiSummaries = Label(.ai, "Automatic transcript summaries")
    static let aiHighlights = Label(.ai, "Key highlights extraction")
    static let aiChatFeature = Label(.ai, "Interactive AI chat assistant")
    static let aiAccept = Label(.ai, "Accept & Enable AI Features")
    static let aiDecline = Label(.ai, "Decline")
    static let aiPrivacy = Label(.ai, "Privacy Policy")
    static let aiProcessing = Label(.ai, "AI features process your transcript content securely. You can disable them anytime in settings.")
    static let analysisIncomplete = Label(.ai, "Analysis incomplete.")
    static let analyzing = Label(.ai, "Analyzing...")
    static let preparingSummary = Label(.ai, "Preparing summary...")
    static let noTranscriptContent = Label(.ai, "No transcript content available")
    static let copyToClipboard = Label(.ai, "Copy to clipboard")
    static let retry = Label(.ai, "Retry")
    static let keyHighlights = Label(.ai, "Key Highlights")
    static let export = Label(.ai, "Export")
    static let chatPlaceholder = Label(.ai, "Ask about the transcript...")
    static let chatStart = Label(.ai, "Start a conversation")
    static let chatWelcome = Label(.ai, "Ask questions about your transcript and get AI-powered insights")
    static let chatThinking = Label(.ai, "AI is thinking...")
    static let chatFailed = Label(.ai, "Failed to get response")
    static let copyChat = Label(.ai, "Copy chat")
    static let downloadChat = Label(.ai, "Download chat")
    static let dismiss = Label(.ai, "Dismiss")
    static let sendMessage = Label(.common, "Send message")
    static let aiProvider = Label(.ai, "AI Provider:")
    static let consentGiven = Label(.ai, "Consent given:")
    static let consentRevoked = Label(.ai, "Consent revoked:")
    static let turnOffTitle = Label(.app, "p5.consent.turn_off_delete_title")
    static let turnOffBody = Label(.app, "p5.consent.turn_off_delete_action")
    static let turnOffConfirm = Label(.app, "p5.consent.turn_off_delete_confirm")
    static let cleanupPending = Label(.app, "p5.consent.cleanup_pending")

    // MARK: Settings, data, feedback and support

    static let privacyInformation = Label(.common, "Privacy Information:")
    static let dataRetention = Label(.common, "Data Retention")
    static let cleanupPolicyTitle = Label(.common, "Automatic Transcript Cleanup Policy")
    static let cleanupSwitch = Label(.common, "Automatic transcript cleanup (30 days)")
    static let cleanupHintOn = Label(.common, "Transcripts older than 30 days will be automatically deleted daily. Disable to keep transcripts indefinitely.")
    static let cleanupHintOff = Label(.common, "Automatic cleanup disabled. Your transcripts will be kept indefinitely, which may increase storage usage.")
    static let legacySettings = Label(.common, "Legacy Settings")
    static let deleteMonthly = Label(.common, "Delete my data automatically")
    static let manualActions = Label(.common, "Manual Actions")
    static let deleteAllFiles = Label(.common, "Delete all Transcripted Files")
    static let deleteAll = Label(.common, "Delete All")
    static let deleteAccountTitle = Label(.common, "Delete my Account")
    static let deleteAccount = Label(.common, "Delete Account")
    static let feedbackTitle = Label(.common, "Rate Your Experience")
    static let feedbackPrompt = Label(.common, "How was your experience?")
    static let feedbackPlaceholder = Label(.common, "Share your thoughts...")
    static let feedbackThanksTitle = Label(.common, "Thank You!")
    static let feedbackThanks = Label(.common, "Thank you for your feedback!")
    static let submit = Label(.common, "Submit")
    static let submitting = Label(.common, "Submitting...")
    static let supportTitle = Label(.common, "Support")
    static let supportMessage = Label(.common, "Message")
    static let supportPlaceholder = Label(.common, "Enter your message here")
    static let send = Label(.common, "Send")

    /// Every static label, for the smoke test (the mode-dependent ones are added per mode).
    static var all: [Label] {
        let modes: [AuthMode] = [.signIn, .signUp, .magicLink, .forgottenPassword]
        var labels = modes.flatMap { mode in
            [emailLabel(mode), auth(mode, "button_label"), auth(mode, "loading_button_label"), auth(mode, "email_input_placeholder"),
             auth(mode, "link_text")]
        }
        labels += [auth(.signIn, "password_label"), auth(.signIn, "password_input_placeholder"), auth(.signUp, "password_label"),
                   auth(.signUp, "password_input_placeholder"), socialLabel(.signIn), socialLabel(.signUp)]
        labels += [newPassword, changePassword, uploadFiles, addFromYouTube, files, noFiles, noFilesHint, totalDuration, calculating,
                   translation, audioLanguage, translationLanguage, speakerRecognition, speakerRecognitionHint, chooseLanguage,
                   searchLanguages, noLanguages, starting, startTranslating, startTranscribing, preparingFiles, loading, ok, cancel,
                   close, yes, no, notEnoughBalance, notEnoughBalanceBody, languageRequired, languageRequiredBody, processingTimeout,
                   processingTimeoutBody, processingTimeoutHint, youTubeFailed, youTubeTitle, youTubeBody, youTubeUrl, adding, addVideo,
                   minutes(7), delete, loadingJobs, loadingMore, newTranscription, noTranscripts, noTranscriptsHint, tryAgain,
                   deleteFileTitle, deleteFileBody, allTranscripts, newTranscript, recent, menu, moreOptions, untitled, account,
                   balance, support, settings, logout, billingHistory, apiKeys, showTime, showSpeakers, copyAll, download, downloadAll,
                   feedback, editLine, saveLine, editSpeaker, audioUnavailable, aiAnalysis, askAi, aiConsentTitle, aiFeatures,
                   aiSummaries, aiHighlights, aiChatFeature, aiAccept, aiDecline, aiPrivacy, aiProcessing, analysisIncomplete,
                   analyzing, preparingSummary, noTranscriptContent, copyToClipboard, retry, keyHighlights, export, chatPlaceholder,
                   chatStart, chatWelcome, chatThinking, chatFailed, copyChat, downloadChat, dismiss, sendMessage, aiProvider,
                   consentGiven, consentRevoked, turnOffTitle, turnOffBody, turnOffConfirm, cleanupPending, privacyInformation,
                   dataRetention, cleanupPolicyTitle, cleanupSwitch, cleanupHintOn, cleanupHintOff, legacySettings, deleteMonthly,
                   manualActions, deleteAllFiles, deleteAll, deleteAccountTitle, deleteAccount, feedbackTitle, feedbackPrompt,
                   feedbackPlaceholder, feedbackThanksTitle, feedbackThanks, submit, submitting, supportTitle, supportMessage,
                   supportPlaceholder, send, speaker, notAvailableShort, minShort, play, pause, copyMessage, helpful, notHelpful, configUnavailable]
        return labels
    }
}
