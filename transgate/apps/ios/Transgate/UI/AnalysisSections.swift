import SwiftUI

/// The summary, then the shape core typed by the detected kind (source AnalysisContent and
/// its five section components). Core decides which sections exist and formats the text a
/// section's copy button puts on the clipboard (`copy_section_requested`); the shell words
/// the labels and draws the lists.
struct AnalysisContent: View {
    let summary: String?
    let analysis: Analysis?
    let copy: Copy
    let send: (AppIntent) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if let summary { Text(summary).fixedSize(horizontal: false, vertical: true) }
            if let analysis {
                if summary != nil { Divider() }
                switch analysis {
                case .meeting(let data): MeetingSections(data: data, copy: copy, section: section)
                case .lecture(let data): LectureSections(data: data, copy: copy, section: section)
                case .sales(let data): SalesSections(data: data, copy: copy, section: section)
                case .support(let data): SupportSections(data: data, copy: copy, section: section)
                case .interview(let data): InterviewSections(data: data, copy: copy, section: section)
                case .generic(let data):
                    if let highlights = data.highlights, !highlights.isEmpty {
                        SectionBlock(title: copy.text(Labels.keyHighlights), count: nil, copyLabel: nil, copied: {}) {
                            ForEach(Array(highlights.enumerated()), id: \.offset) { _, item in Bullet(item) }
                        }
                    }
                }
            }
        }
    }

    private var section: (SectionId) -> Void { { send(.analysis(.copySectionRequested(section: $0))) } }
}

/// A titled section with an optional count and its own copy button. The check mark after a
/// copy is view mechanics; core formatted what was copied.
struct SectionBlock<Content: View>: View {
    let title: String
    let count: Int?
    let copyLabel: String?
    let copied: () -> Void
    @ViewBuilder let content: () -> Content
    @State private var done = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(count.map { "\(title) (\($0))" } ?? title).font(.subheadline.weight(.semibold))
                Spacer()
                if let copyLabel {
                    Button {
                        copied()
                        done = true
                        Task { try? await Task.sleep(nanoseconds: 2_000_000_000); done = false }
                    } label: { Image(systemName: done ? "checkmark" : "doc.on.doc").frame(minWidth: 44, minHeight: 44) }
                        .accessibilityLabel(copyLabel)
                }
            }
            content()
        }
        .accessibilityElement(children: .contain)
    }
}

struct Bullet: View {
    let text: String
    var mark = "•"
    var tone: Color = tokens.accent
    init(_ text: String, mark: String = "•", tone: Color = tokens.accent) { self.text = text; self.mark = mark; self.tone = tone }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Text(mark).foregroundStyle(tone).accessibilityHidden(true)
            Text(text).fixedSize(horizontal: false, vertical: true)
        }.font(.subheadline)
    }
}

struct Numbered: View {
    let items: [String]
    var body: some View {
        ForEach(Array(items.enumerated()), id: \.offset) { index, item in
            HStack(alignment: .top, spacing: 8) {
                Text("\(index + 1).").fontWeight(.medium).foregroundStyle(tokens.accent)
                Text(item).fixedSize(horizontal: false, vertical: true)
            }.font(.subheadline)
        }
    }
}

private struct Card<Content: View>: View {
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 6, content: content)
            .padding(10).frame(maxWidth: .infinity, alignment: .leading)
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(tokens.border))
    }
}

private func block<Content: View>(_ copy: Copy, _ title: Label, count: Int? = nil, copyable: SectionId? = nil, _ section: @escaping (SectionId) -> Void,
                                  @ViewBuilder _ content: @escaping () -> Content) -> some View {
    SectionBlock(title: copy.text(title), count: count, copyLabel: copyable == nil ? nil : copy.text(Labels.copyToClipboard),
                 copied: { if let copyable { section(copyable) } }, content: content)
}

// MARK: Meeting

struct MeetingSections: View {
    let data: MeetingAnalysis
    let copy: Copy
    let section: (SectionId) -> Void

    var body: some View {
        let items = data.actionItems ?? [], decisions = data.decisions ?? [], points = data.discussionPoints ?? [], steps = data.nextSteps ?? []
        if items.count + decisions.count + points.count + steps.count == 0 {
            Text(copy.text(Labels.noMeetingInsights)).font(.subheadline).foregroundStyle(tokens.muted)
        } else {
            VStack(alignment: .leading, spacing: 20) {
                if !items.isEmpty {
                    block(copy, Labels.actionItems, count: items.count, copyable: .actionItems, section) {
                        ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                            HStack(alignment: .top, spacing: 8) {
                                Text(item.status == .done ? "☑" : item.status == .inProgress ? "◐" : "☐").foregroundStyle(tokens.muted).accessibilityHidden(true)
                                VStack(alignment: .leading) {
                                    Text((item.owner.map { "\($0): " } ?? "") + item.task).strikethrough(item.status == .done)
                                    if let deadline = item.deadline { Text("\(copy.text(Labels.due)) \(deadline)").font(.caption).foregroundStyle(tokens.muted) }
                                }
                            }.font(.subheadline)
                        }
                    }
                }
                if !decisions.isEmpty {
                    block(copy, Labels.decisionsMade, count: decisions.count, copyable: .decisions, section) {
                        ForEach(Array(decisions.enumerated()), id: \.offset) { _, item in
                            Card {
                                Text(item.decision).fontWeight(.medium)
                                if let who = item.participants, !who.isEmpty { Text("\(copy.text(Labels.participants)) \(who.joined(separator: ", "))").font(.caption).foregroundStyle(tokens.muted) }
                                if let at = item.timestamp { Text("\(copy.text(Labels.at)) \(Formatting.clock(at))").font(.caption).foregroundStyle(tokens.muted) }
                            }
                        }
                    }
                }
                if !points.isEmpty {
                    block(copy, Labels.discussionPoints, count: points.count, copyable: .discussionPoints, section) { ForEach(Array(points.enumerated()), id: \.offset) { _, point in Bullet(point) } }
                }
                if !steps.isEmpty {
                    block(copy, Labels.nextSteps, count: steps.count, copyable: .nextSteps, section) { Numbered(items: steps) }
                }
            }
        }
    }
}

// MARK: Lecture

struct LectureSections: View {
    let data: LectureAnalysis
    let copy: Copy
    let section: (SectionId) -> Void

    var body: some View {
        let topics = data.topics ?? [], concepts = data.concepts ?? [], questions = data.reviewQuestions ?? [], guide = data.studyGuide ?? []
        VStack(alignment: .leading, spacing: 20) {
            if !topics.isEmpty {
                block(copy, Labels.topicsCovered, count: topics.count, copyable: .topics, section) {
                    ForEach(Array(topics.enumerated()), id: \.offset) { index, topic in
                        HStack(alignment: .top, spacing: 10) {
                            Text("\(index + 1)").font(.caption.weight(.medium)).foregroundStyle(tokens.accent)
                                .frame(width: 24, height: 24).background(tokens.cursorBg, in: Circle())
                            VStack(alignment: .leading) {
                                Text(topic.title).fontWeight(.medium)
                                HStack(spacing: 12) {
                                    if let start = topic.timestamp { Text("\(copy.text(Labels.starts)) \(Formatting.clock(start))") }
                                    if let length = topic.duration { Text("\(copy.text(Labels.durationLabel)) \(Formatting.duration(length))") }
                                }.font(.caption).foregroundStyle(tokens.muted)
                            }
                        }.font(.subheadline)
                    }
                }
            }
            if !concepts.isEmpty {
                block(copy, Labels.keyConcepts, count: concepts.count, copyable: .concepts, section) {
                    ForEach(Array(concepts.enumerated()), id: \.offset) { _, concept in
                        Card {
                            Text(concept.concept).fontWeight(.medium)
                            Text(concept.explanation).font(.subheadline).foregroundStyle(tokens.muted)
                            if let examples = concept.examples, !examples.isEmpty {
                                Text(copy.text(Labels.examples)).font(.caption.weight(.medium))
                                ForEach(Array(examples.enumerated()), id: \.offset) { _, example in Bullet(example, tone: tokens.muted) }
                            }
                        }
                    }
                }
            }
            if !questions.isEmpty { block(copy, Labels.reviewQuestions, count: questions.count, copyable: .reviewQuestions, section) { Numbered(items: questions) } }
            if !guide.isEmpty {
                block(copy, Labels.studyGuide, count: guide.count, copyable: .studyGuide, section) { ForEach(Array(guide.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "📚") } }
            }
        }
    }
}

// MARK: Sales

struct SalesSections: View {
    let data: SalesAnalysis
    let copy: Copy
    let section: (SectionId) -> Void

    private func metric(_ label: Label, _ value: String, tone: Color = tokens.fg) -> some View {
        Card { Text(copy.text(label)).font(.caption).foregroundStyle(tokens.muted); Text(value).fontWeight(.medium).foregroundStyle(tone) }
    }

    var body: some View {
        let metrics = data.callMetrics
        let tips = data.coachingTips ?? []
        let strengths = tips.filter { $0.category == .strength }, improvements = tips.filter { $0.category == .improvement }
        let insights = data.customerInsights
        let groups: [(Label, [String], Color)] = [
            (Labels.painPoints, insights?.painPoints ?? [], tokens.error), (Labels.budgetSignals, insights?.budgetSignals ?? [], tokens.accent),
            (Labels.objections, insights?.objections ?? [], tokens.progress), (Labels.buyingSignals, insights?.buyingSignals ?? [], tokens.success),
        ].filter { !$0.1.isEmpty }
        let steps = data.nextSteps ?? []
        VStack(alignment: .leading, spacing: 20) {
            if let metrics, metrics.talkListenRatio != nil || metrics.questionsAsked != nil || metrics.objectionsHandled != nil || metrics.nextStepsConfirmed != nil {
                block(copy, Labels.callMetrics, copyable: .callMetrics, section) {
                    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 8) {
                        if let ratio = metrics.talkListenRatio { metric(Labels.talkListen, ratio) }
                        if let asked = metrics.questionsAsked { metric(Labels.questionsAsked, "\(Int(asked))") }
                        if let handled = metrics.objectionsHandled { metric(Labels.objectionsHandled, "\(Int(handled))") }
                        if let confirmed = metrics.nextStepsConfirmed {
                            metric(Labels.nextStepsConfirmed, (confirmed ? "✓ " : "✗ ") + copy.text(confirmed ? Labels.yes : Labels.no), tone: confirmed ? tokens.success : tokens.error)
                        }
                    }
                }
            }
            if !tips.isEmpty {
                block(copy, Labels.coachingTips, count: tips.count, copyable: .coachingTips, section) {
                    if !strengths.isEmpty {
                        Text(copy.text(Labels.strengths)).font(.subheadline.weight(.medium)).foregroundStyle(tokens.success)
                        ForEach(Array(strengths.enumerated()), id: \.offset) { _, tip in Bullet(tip.tip, mark: "✓", tone: tokens.success) }
                    }
                    if !improvements.isEmpty {
                        Text(copy.text(Labels.improvements)).font(.subheadline.weight(.medium)).foregroundStyle(tokens.progress)
                        ForEach(Array(improvements.enumerated()), id: \.offset) { _, tip in Bullet(tip.tip, mark: "→", tone: tokens.progress) }
                    }
                }
            }
            if !groups.isEmpty {
                block(copy, Labels.customerInsights, copyable: .customerInsights, section) {
                    ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                        Text(copy.text(group.0)).font(.subheadline.weight(.medium)).foregroundStyle(group.2)
                        ForEach(Array(group.1.enumerated()), id: \.offset) { _, item in Bullet(item, tone: group.2) }
                    }
                }
            }
            if !steps.isEmpty { block(copy, Labels.recommendedNextSteps, count: steps.count, copyable: .salesNextSteps, section) { Numbered(items: steps) } }
        }
    }
}

// MARK: Support

struct SupportSections: View {
    let data: SupportAnalysis
    let copy: Copy
    let section: (SectionId) -> Void

    private func emoji(_ sentiment: String) -> String {
        ["frustrated": "😤", "neutral": "😐", "satisfied": "😊", "relieved": "😌"][sentiment] ?? "😐"
    }

    var body: some View {
        let strengths = data.insights?.strengths ?? [], improvements = data.insights?.improvements ?? []
        let kb = data.kbUpdates ?? [], followUps = data.followUpActions ?? []
        VStack(alignment: .leading, spacing: 20) {
            if let issue = data.issueResolution {
                block(copy, Labels.issueResolution, section) {
                    Card {
                        HStack(alignment: .top) {
                            VStack(alignment: .leading) { Text(copy.text(Labels.problem)).font(.caption).foregroundStyle(tokens.muted); Text(issue.problem).fontWeight(.medium) }
                            Spacer()
                            Text(String(describing: issue.status).capitalized).font(.caption.weight(.medium)).padding(.horizontal, 8).padding(.vertical, 4)
                                .overlay(RoundedRectangle(cornerRadius: 4).stroke(tokens.border))
                        }
                        if let cause = issue.rootCause { Text(copy.text(Labels.rootCause)).font(.caption).foregroundStyle(tokens.muted); Text(cause) }
                        if let fix = issue.resolution { Text(copy.text(Labels.resolution)).font(.caption).foregroundStyle(tokens.muted); Text(fix) }
                        if let minutes = issue.resolutionTimeMinutes {
                            Text(copy.text(Labels.resolutionTime)).font(.caption).foregroundStyle(tokens.muted)
                            Text("\(minutes.rounded() == minutes ? String(Int(minutes)) : String(minutes)) \(copy.text(Labels.minutesWord))")
                        }
                    }.font(.subheadline)
                }
            }
            if let sentiment = data.sentiment {
                block(copy, Labels.customerSentiment, section) {
                    Card {
                        HStack {
                            Spacer()
                            VStack { Text(copy.text(Labels.initialWord)).font(.caption).foregroundStyle(tokens.muted); Text(emoji(sentiment.initial)).font(.title); Text(sentiment.initial.capitalized) }
                            Spacer(); Text("→").foregroundStyle(tokens.muted); Spacer()
                            VStack { Text(copy.text(Labels.finalWord)).font(.caption).foregroundStyle(tokens.muted); Text(emoji(sentiment.final)).font(.title); Text(sentiment.final.capitalized) }
                            Spacer()
                        }.font(.subheadline)
                        if let moments = sentiment.keyMoments, !moments.isEmpty {
                            Text(copy.text(Labels.keyMoments)).font(.caption.weight(.medium)).foregroundStyle(tokens.muted)
                            ForEach(Array(moments.enumerated()), id: \.offset) { _, moment in
                                HStack(spacing: 8) {
                                    Text(Formatting.clock(moment.timestamp)).font(.caption.monospacedDigit()).foregroundStyle(tokens.muted)
                                    Text(emoji(String(describing: moment.sentiment)))
                                    Text(moment.reason ?? String(describing: moment.sentiment))
                                }.font(.subheadline)
                            }
                        }
                    }
                }
            }
            if !strengths.isEmpty || !improvements.isEmpty {
                block(copy, Labels.agentPerformance, copyable: .supportInsights, section) {
                    if !strengths.isEmpty {
                        Text(copy.text(Labels.strengths)).font(.subheadline.weight(.medium)).foregroundStyle(tokens.success)
                        ForEach(Array(strengths.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "✓", tone: tokens.success) }
                    }
                    if !improvements.isEmpty {
                        Text(copy.text(Labels.improvements)).font(.subheadline.weight(.medium)).foregroundStyle(tokens.progress)
                        ForEach(Array(improvements.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "→", tone: tokens.progress) }
                    }
                }
            }
            if !kb.isEmpty { block(copy, Labels.kbUpdates, count: kb.count, copyable: .kbUpdates, section) { ForEach(Array(kb.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "📝") } } }
            if !followUps.isEmpty {
                block(copy, Labels.followUps, count: followUps.count, copyable: .followUpActions, section) { ForEach(Array(followUps.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "☐") } }
            }
        }
    }
}

// MARK: Interview

struct InterviewSections: View {
    let data: InterviewAnalysis
    let copy: Copy
    let section: (SectionId) -> Void

    private func stars(_ rating: Double) -> some View {
        HStack(spacing: 2) {
            ForEach(1...5, id: \.self) { star in Text("★").foregroundStyle(Double(star) <= rating ? tokens.progress : tokens.border) }
            Text("(\(rating.rounded() == rating ? String(Int(rating)) : String(rating))/5)").font(.caption).foregroundStyle(tokens.muted)
        }.accessibilityElement(children: .ignore).accessibilityLabel("\(rating)/5")
    }

    var body: some View {
        let assessment = data.assessment
        let strengths = assessment?.strengths ?? [], concerns = assessment?.concerns ?? []
        let skills = data.skillsAssessment ?? [], responses = data.notableResponses ?? [], steps = data.nextSteps ?? []
        VStack(alignment: .leading, spacing: 20) {
            if let assessment, assessment.position != nil || assessment.overallRating != nil || !strengths.isEmpty || !concerns.isEmpty {
                block(copy, Labels.candidateAssessment, copyable: .candidateAssessment, section) {
                    if assessment.position != nil || assessment.overallRating != nil {
                        Card {
                            if let position = assessment.position { Text(copy.text(Labels.position)).font(.caption).foregroundStyle(tokens.muted); Text(position).fontWeight(.medium) }
                            if let rating = assessment.overallRating { Text(copy.text(Labels.overallRating)).font(.caption).foregroundStyle(tokens.muted); stars(rating) }
                        }
                    }
                    if !strengths.isEmpty {
                        Text(copy.text(Labels.strengths)).font(.subheadline.weight(.medium)).foregroundStyle(tokens.success)
                        ForEach(Array(strengths.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "✓", tone: tokens.success) }
                    }
                    if !concerns.isEmpty {
                        Text(copy.text(Labels.concerns)).font(.subheadline.weight(.medium)).foregroundStyle(tokens.progress)
                        ForEach(Array(concerns.enumerated()), id: \.offset) { _, item in Bullet(item, mark: "⚠", tone: tokens.progress) }
                    }
                }
            }
            if !skills.isEmpty {
                block(copy, Labels.skillsAssessment, count: skills.count, copyable: .skillsAssessment, section) {
                    ForEach(Array(skills.enumerated()), id: \.offset) { _, skill in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack { Text(skill.skill).fontWeight(.medium); Spacer(); Text(String(describing: skill.level)).font(.caption).foregroundStyle(tokens.muted) }
                            HStack(spacing: 4) {
                                ForEach(1...5, id: \.self) { star in
                                    RoundedRectangle(cornerRadius: 3).fill(Double(star) <= skill.rating ? tokens.accent : tokens.cursorBg).frame(height: 8)
                                }
                            }.accessibilityElement(children: .ignore).accessibilityLabel("\(skill.rating)/5")
                        }.font(.subheadline)
                    }
                }
            }
            if !responses.isEmpty {
                block(copy, Labels.notableResponses, count: responses.count, copyable: .notableResponses, section) {
                    ForEach(Array(responses.enumerated()), id: \.offset) { _, response in
                        Card {
                            HStack(alignment: .top) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text("Q: \(response.question)").fontWeight(.medium).foregroundStyle(tokens.muted)
                                    Text(response.answer)
                                }
                                Spacer()
                                Text(response.assessment == .positive ? "👍" : response.assessment == .negative ? "👎" : "➖").accessibilityLabel(String(describing: response.assessment))
                            }.font(.subheadline)
                        }
                    }
                }
            }
            if !steps.isEmpty {
                block(copy, Labels.recommendedNextSteps, count: steps.count, copyable: .interviewNextSteps, section) {
                    ForEach(Array(steps.enumerated()), id: \.offset) { _, step in
                        HStack(alignment: .top, spacing: 8) {
                            Text(String(describing: step.priority)).font(.caption.weight(.medium)).padding(.horizontal, 6).padding(.vertical, 2)
                                .foregroundStyle(step.priority == .high ? tokens.error : step.priority == .medium ? tokens.progress : tokens.muted)
                                .overlay(RoundedRectangle(cornerRadius: 4).stroke(step.priority == .high ? tokens.error : step.priority == .medium ? tokens.progress : tokens.border))
                            Text(step.action)
                        }.font(.subheadline)
                    }
                }
            }
        }
    }
}

// MARK: Chat limits (source ChatLimitsInfo)

/// Core picks the counters, bar widths, tones and the reset label; the view words them.
struct ChatLimitsCard: View {
    let limits: LimitsVm
    let copy: Copy

    private func usage(_ label: Label, _ value: UsageVm) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack { Text(copy.text(label)).foregroundStyle(tokens.muted); Spacer(); Text("\(value.current) / \(value.max)").fontWeight(.medium) }.font(.caption)
            ProgressView(value: Double(value.widthPercent), total: 100).tint(tokens.tone(value.tone)).accessibilityLabel(copy.text(label))
        }
    }

    private var reset: String? {
        switch limits.reset {
        case nil: return nil
        case .lessThanHour?: return copy.text(Labels.resetsLessThanHour)
        case .inHours(let hours)?: return "\(copy.text(Labels.resetsIn)) \(hours) \(copy.text(hours > 1 ? Labels.hours : Labels.hour))"
        case .inDays(let days)?: return "\(copy.text(Labels.resetsIn)) \(days) \(copy.text(days > 1 ? Labels.days : Labels.day))"
        case .on(let at)?: return "\(copy.text(Labels.resetsOn)) \(Formatting.localDate(micros: at, locale: copy.locale))"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(copy.text(Labels.usageLimits)).font(.caption.weight(.semibold))
            if let questions = limits.questions { usage(Labels.questions, questions) }
            if let tokenUsage = limits.tokens { usage(Labels.tokens, tokenUsage) }
            if let reset { Text(reset).font(.caption).foregroundStyle(tokens.muted) }
        }
        .padding(10)
        .background(tokens.noticeBg, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(tokens.border))
    }
}
