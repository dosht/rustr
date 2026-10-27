import Foundation

/// Display formatting the web's AI views did inline (`features/ai/format.ts`) and the
/// player's active-line rule (`platform/media/player.ts`). Copy text for sections is
/// formatted in core; these only draw on-screen values. Pure, so Linux tests them.
enum Formatting {
    /// `m:ss` from seconds, as the source's `formatTimestamp`.
    static func clock(_ seconds: Double) -> String {
        let whole = seconds.isFinite && seconds > 0 ? Int(seconds) : 0
        return "\(whole / 60):\(String(format: "%02d", whole % 60))"
    }

    /// Seconds under a minute, otherwise whole minutes (`formatDuration`).
    static func duration(_ seconds: Double) -> String {
        seconds < 60 ? "\(Int(seconds))s" : "\(Int(seconds) / 60) min"
    }

    /// The player's `HH:MM:SS` readout, whole seconds rounded down (source AudioPlayer).
    static func clockTime(_ seconds: Double) -> String {
        let whole = seconds.isFinite && seconds > 0 ? Int(seconds) : 0
        return String(format: "%02d:%02d:%02d", whole / 3600, (whole % 3600) / 60, whole % 60)
    }

    /// A local date for epoch microseconds, as `toLocaleDateString()` showed.
    static func localDate(micros: Int64, locale: String) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: locale)
        formatter.dateStyle = .short
        formatter.timeStyle = .none
        return formatter.string(from: Date(timeIntervalSince1970: Double(micros) / 1_000_000))
    }

    /// Index of the line being played: the first whose end is not before `time`, -1 for none
    /// (UI mechanics; the player's clock is the shell's, the rows are core's).
    static func activeLine(ends: [Double], time: Double) -> Int {
        guard time > 0 else { return -1 }
        return ends.firstIndex { time <= $0 } ?? -1
    }
}

/// What the mounted player reports: its clock and whether it is playing.
struct PlayerSnapshot: Equatable, Sendable {
    var time: Double
    var playing: Bool
}

extension Labels {
    // MARK: AI analysis sections (ai namespace unless noted)

    static let actionItems = Label(.ai, "Action Items")
    static let decisionsMade = Label(.ai, "Decisions Made")
    static let discussionPoints = Label(.ai, "Discussion Points")
    static let nextSteps = Label(.ai, "Next Steps")
    static let due = Label(.ai, "Due:")
    static let participants = Label(.ai, "Participants:")
    static let at = Label(.ai, "At:")
    static let noMeetingInsights = Label(.ai, "No meeting insights available for this transcript.")
    static let topicsCovered = Label(.ai, "Topics Covered")
    static let keyConcepts = Label(.ai, "Key Concepts")
    static let reviewQuestions = Label(.ai, "Review Questions")
    static let studyGuide = Label(.ai, "Study Guide")
    static let starts = Label(.ai, "Starts:")
    static let durationLabel = Label(.ai, "Duration:")
    static let examples = Label(.ai, "Examples:")
    static let callMetrics = Label(.ai, "Call Metrics")
    static let talkListen = Label(.ai, "Talk:Listen Ratio")
    static let questionsAsked = Label(.ai, "Questions Asked")
    static let objectionsHandled = Label(.ai, "Objections Handled")
    static let nextStepsConfirmed = Label(.ai, "Next Steps Confirmed")
    static let coachingTips = Label(.ai, "Coaching Tips")
    static let strengths = Label(.ai, "Strengths")
    static let improvements = Label(.ai, "Areas for Improvement")
    static let customerInsights = Label(.ai, "Customer Insights")
    static let painPoints = Label(.ai, "Pain Points")
    static let budgetSignals = Label(.ai, "Budget Signals")
    static let objections = Label(.ai, "Objections")
    static let buyingSignals = Label(.ai, "Buying Signals")
    static let recommendedNextSteps = Label(.ai, "Recommended Next Steps")
    static let issueResolution = Label(.ai, "Issue Resolution")
    static let problem = Label(.ai, "Problem")
    static let rootCause = Label(.ai, "Root Cause")
    static let resolution = Label(.ai, "Resolution")
    static let resolutionTime = Label(.ai, "Resolution Time")
    static let minutesWord = Label(.ai, "minutes")
    static let customerSentiment = Label(.ai, "Customer Sentiment")
    static let initialWord = Label(.ai, "Initial")
    static let finalWord = Label(.ai, "Final")
    static let keyMoments = Label(.ai, "Key Moments")
    static let agentPerformance = Label(.ai, "Agent Performance")
    static let kbUpdates = Label(.ai, "Knowledge Base Updates")
    static let followUps = Label(.ai, "Follow-Up Actions")
    static let candidateAssessment = Label(.ai, "Candidate Assessment")
    static let position = Label(.ai, "Position")
    static let overallRating = Label(.ai, "Overall Rating")
    static let concerns = Label(.ai, "Concerns")
    static let skillsAssessment = Label(.ai, "Skills Assessment")
    static let notableResponses = Label(.ai, "Notable Responses")

    // MARK: Chat limits and the special offer

    static let usageLimits = Label(.ai, "Usage Limits")
    static let questions = Label(.ai, "Questions")
    static let tokens = Label(.ai, "Tokens")
    static let resetsLessThanHour = Label(.ai, "Resets in less than 1 hour")
    static let resetsIn = Label(.ai, "Resets in")
    static let resetsOn = Label(.ai, "Resets on")
    static let hour = Label(.ai, "hour")
    static let hours = Label(.ai, "hours")
    static let day = Label(.ai, "day")
    static let days = Label(.ai, "days")
    static let specialOffer = Label(.common, "Special Offer for Valued Clients!")
    static let useCode = Label(.common, "Use this code at checkout to get your discount!")
    static let copyCode = Label(.common, "copyCode")
    static let copied = Label(.common, "copied")

    static var analysisAll: [Label] {
        [actionItems, decisionsMade, discussionPoints, nextSteps, due, participants, at, noMeetingInsights, topicsCovered, keyConcepts,
         reviewQuestions, studyGuide, starts, durationLabel, examples, callMetrics, talkListen, questionsAsked, objectionsHandled,
         nextStepsConfirmed, coachingTips, strengths, improvements, customerInsights, painPoints, budgetSignals, objections,
         buyingSignals, recommendedNextSteps, issueResolution, problem, rootCause, resolution, resolutionTime, minutesWord,
         customerSentiment, initialWord, finalWord, keyMoments, agentPerformance, kbUpdates, followUps, candidateAssessment, position,
         overallRating, concerns, skillsAssessment, notableResponses, usageLimits, questions, tokens, resetsLessThanHour, resetsIn,
         resetsOn, hour, hours, day, days, specialOffer, useCode, copyCode, copied]
    }
}
