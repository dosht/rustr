package dev.transgate.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.transgate.core.ActionStatus
import dev.transgate.core.Analysis
import dev.transgate.core.AnalysisIntent
import dev.transgate.core.AppIntent
import dev.transgate.core.Assessment
import dev.transgate.core.CallMetrics
import dev.transgate.core.CoachingCategory
import dev.transgate.core.CustomerInsights
import dev.transgate.core.GenericAnalysis
import dev.transgate.core.InterviewAnalysis
import dev.transgate.core.LectureAnalysis
import dev.transgate.core.MeetingAnalysis
import dev.transgate.core.Ns
import dev.transgate.core.ResponseAssessment
import dev.transgate.core.SalesAnalysis
import dev.transgate.core.SectionId
import dev.transgate.core.SentimentKind
import dev.transgate.core.SupportAnalysis

private typealias AnalysisSend = (AppIntent) -> Unit

/** The typed analysis, one titled block per section core named (web `AnalysisContent`). */
internal fun LazyListScope.analysisBlocks(analysis: Analysis, counter: SectionCounter, send: (AppIntent) -> Unit) {
    when (analysis) {
        is Analysis.Generic -> genericBlocks(analysis.v1, counter, send)
        is Analysis.Lecture -> lectureBlocks(analysis.v1, counter, send)
        is Analysis.Meeting -> meetingBlocks(analysis.v1, counter, send)
        is Analysis.Sales -> salesBlocks(analysis.v1, counter, send)
        is Analysis.Support -> supportBlocks(analysis.v1, counter, send)
        is Analysis.Interview -> interviewBlocks(analysis.v1, counter, send)
    }
}

/** One titled block. [id] adds the section's own copy button; [count] follows the title. */
private fun LazyListScope.block(
    counter: SectionCounter,
    title: String,
    count: Int?,
    id: SectionId?,
    send: AnalysisSend,
    content: @Composable ColumnScope.() -> Unit,
) {
    val n = counter.n++
    item(key = "analysis-section-$n") {
        Column(Modifier.fillMaxWidth().testTag("analysis-section-$n"), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Text(
                    tk(Ns.AI, title) + (count?.let { " ($it)" } ?: ""),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.weight(1f),
                )
                if (id != null) {
                    TextButton(
                        onClick = { send(AppIntent.Analysis(AnalysisIntent.CopySectionRequested(id))) },
                        modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-section-copy-$n"),
                    ) { Text(tk(Ns.AI, "Copy to clipboard")) }
                }
            }
            SelectionContainer { Column(verticalArrangement = Arrangement.spacedBy(8.dp)) { content() } }
        }
    }
}

private fun clock(seconds: Double): String {
    val total = seconds.toLong()
    return "${total / 60}:${(total % 60).toString().padStart(2, '0')}"
}

@Composable
private fun Bullets(items: List<String>, glyph: String = "•") {
    items.forEach { Text("$glyph $it", style = MaterialTheme.typography.bodyMedium) }
}

@Composable
private fun Numbered(items: List<String>) {
    items.forEachIndexed { index, item -> Text("${index + 1}. $item", style = MaterialTheme.typography.bodyMedium) }
}

@Composable
private fun Muted(text: String) = Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)

@Composable
private fun Outlined(content: @Composable ColumnScope.() -> Unit) {
    Card(
        Modifier.fillMaxWidth(),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) { Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp), content = content) }
}

private fun LazyListScope.genericBlocks(data: GenericAnalysis, counter: SectionCounter, send: AnalysisSend) {
    val highlights = data.highlights.orEmpty()
    if (highlights.isNotEmpty()) block(counter, "Key Highlights", null, null, send) { Bullets(highlights) }
}

private fun LazyListScope.lectureBlocks(data: LectureAnalysis, counter: SectionCounter, send: AnalysisSend) {
    val topics = data.topics.orEmpty()
    val concepts = data.concepts.orEmpty()
    val questions = data.reviewQuestions.orEmpty()
    val guide = data.studyGuide.orEmpty()
    if (topics.isNotEmpty()) block(counter, "Topics Covered", topics.size, SectionId.TOPICS, send) {
        topics.forEachIndexed { index, topic ->
            Column {
                Text("${index + 1}. ${topic.title}", style = MaterialTheme.typography.bodyMedium)
                topic.timestamp?.let { Muted(tk(Ns.AI, "Starts:") + " " + clock(it)) }
                topic.duration?.let { Muted(tk(Ns.AI, "Duration:") + " " + clock(it)) }
            }
        }
    }
    if (concepts.isNotEmpty()) block(counter, "Key Concepts", concepts.size, SectionId.CONCEPTS, send) {
        concepts.forEach { concept ->
            Outlined {
                Text(concept.concept, style = MaterialTheme.typography.titleSmall)
                Muted(concept.explanation)
                val examples = concept.examples.orEmpty()
                if (examples.isNotEmpty()) {
                    Text(tk(Ns.AI, "Examples:"), style = MaterialTheme.typography.labelMedium)
                    Bullets(examples)
                }
            }
        }
    }
    if (questions.isNotEmpty()) block(counter, "Review Questions", questions.size, SectionId.REVIEW_QUESTIONS, send) { Numbered(questions) }
    if (guide.isNotEmpty()) block(counter, "Study Guide", guide.size, SectionId.STUDY_GUIDE, send) { Bullets(guide, "\uD83D\uDCDA") }
}

private fun LazyListScope.meetingBlocks(data: MeetingAnalysis, counter: SectionCounter, send: AnalysisSend) {
    val actionItems = data.actionItems.orEmpty()
    val decisions = data.decisions.orEmpty()
    val points = data.discussionPoints.orEmpty()
    val steps = data.nextSteps.orEmpty()
    if (actionItems.size + decisions.size + points.size + steps.size == 0) {
        item(key = "meeting-empty") {
            Text(tk(Ns.AI, "No meeting insights available for this transcript."), color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("meeting-analysis-empty"))
        }
        return
    }
    if (actionItems.isNotEmpty()) block(counter, "Action Items", actionItems.size, SectionId.ACTION_ITEMS, send) {
        actionItems.forEach { item ->
            val mark = when (item.status) {
                ActionStatus.DONE -> "☑"
                ActionStatus.IN_PROGRESS -> "◐"
                ActionStatus.PENDING -> "☐"
            }
            Column {
                Text("$mark " + (item.owner?.let { "$it: " } ?: "") + item.task, style = MaterialTheme.typography.bodyMedium)
                item.deadline?.let { Muted(tk(Ns.AI, "Due:") + " " + it) }
            }
        }
    }
    if (decisions.isNotEmpty()) block(counter, "Decisions Made", decisions.size, SectionId.DECISIONS, send) {
        decisions.forEach { item ->
            Outlined {
                Text(item.decision, style = MaterialTheme.typography.bodyMedium)
                val participants = item.participants.orEmpty()
                if (participants.isNotEmpty()) Muted(tk(Ns.AI, "Participants:") + " " + participants.joinToString(", "))
                item.timestamp?.let { Muted(tk(Ns.AI, "At:") + " " + clock(it)) }
            }
        }
    }
    if (points.isNotEmpty()) block(counter, "Discussion Points", points.size, SectionId.DISCUSSION_POINTS, send) { Bullets(points) }
    if (steps.isNotEmpty()) block(counter, "Next Steps", steps.size, SectionId.NEXT_STEPS, send) { Numbered(steps) }
}

private fun LazyListScope.salesBlocks(data: SalesAnalysis, counter: SectionCounter, send: AnalysisSend) {
    val metrics = data.callMetrics
    val tips = data.coachingTips.orEmpty()
    val insights = data.customerInsights
    val steps = data.nextSteps.orEmpty()
    if (metrics != null && metrics.hasAny()) block(counter, "Call Metrics", null, SectionId.CALL_METRICS, send) { MetricsRows(metrics) }
    if (tips.isNotEmpty()) block(counter, "Coaching Tips", tips.size, SectionId.COACHING_TIPS, send) {
        val strengths = tips.filter { it.category == CoachingCategory.STRENGTH }
        val improvements = tips.filter { it.category == CoachingCategory.IMPROVEMENT }
        if (strengths.isNotEmpty()) {
            Text(tk(Ns.AI, "Strengths"), style = MaterialTheme.typography.labelLarge)
            Bullets(strengths.map { it.tip }, "✓")
        }
        if (improvements.isNotEmpty()) {
            Text(tk(Ns.AI, "Areas for Improvement"), style = MaterialTheme.typography.labelLarge)
            Bullets(improvements.map { it.tip }, "→")
        }
    }
    if (insights != null && insights.groups().isNotEmpty()) block(counter, "Customer Insights", null, SectionId.CUSTOMER_INSIGHTS, send) {
        insights.groups().forEach { (title, items) ->
            Text(tk(Ns.AI, title), style = MaterialTheme.typography.labelLarge)
            Bullets(items)
        }
    }
    if (steps.isNotEmpty()) block(counter, "Recommended Next Steps", steps.size, SectionId.SALES_NEXT_STEPS, send) { Numbered(steps) }
}

private fun CallMetrics.hasAny() = !talkListenRatio.isNullOrEmpty() || questionsAsked != null || objectionsHandled != null || nextStepsConfirmed != null

private fun CustomerInsights.groups(): List<Pair<String, List<String>>> = listOf(
    "Pain Points" to painPoints.orEmpty(),
    "Budget Signals" to budgetSignals.orEmpty(),
    "Objections" to objections.orEmpty(),
    "Buying Signals" to buyingSignals.orEmpty(),
).filter { it.second.isNotEmpty() }

@Composable
private fun MetricsRows(metrics: CallMetrics) {
    metrics.talkListenRatio?.takeIf { it.isNotEmpty() }?.let { Outlined { Muted(tk(Ns.AI, "Talk:Listen Ratio")); Text(it) } }
    metrics.questionsAsked?.let { Outlined { Muted(tk(Ns.AI, "Questions Asked")); Text(it.toLong().toString()) } }
    metrics.objectionsHandled?.let { Outlined { Muted(tk(Ns.AI, "Objections Handled")); Text(it.toLong().toString()) } }
    metrics.nextStepsConfirmed?.let { confirmed ->
        Outlined {
            Muted(tk(Ns.AI, "Next Steps Confirmed"))
            Text(if (confirmed) "✓ " + tk(Ns.COMMON, "Yes") else "✗ " + tk(Ns.COMMON, "No"))
        }
    }
}

private fun LazyListScope.supportBlocks(data: SupportAnalysis, counter: SectionCounter, send: AnalysisSend) {
    val issue = data.issueResolution
    val sentiment = data.sentiment
    val strengths = data.insights?.strengths.orEmpty()
    val improvements = data.insights?.improvements.orEmpty()
    val kb = data.kbUpdates.orEmpty()
    val followUps = data.followUpActions.orEmpty()
    if (issue != null) block(counter, "Issue Resolution", null, null, send) {
        Outlined {
            Muted(tk(Ns.AI, "Problem"))
            Text(issue.problem, style = MaterialTheme.typography.bodyMedium)
            Text(issue.status.name.lowercase(), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
            issue.rootCause?.let { Muted(tk(Ns.AI, "Root Cause")); Text(it) }
            issue.resolution?.let { Muted(tk(Ns.AI, "Resolution")); Text(it) }
            issue.resolutionTimeMinutes?.let { Muted(tk(Ns.AI, "Resolution Time")); Text("${it.toLong()} " + tk(Ns.AI, "minutes")) }
        }
    }
    if (sentiment != null) block(counter, "Customer Sentiment", null, null, send) {
        Outlined {
            Muted(tk(Ns.AI, "Initial"))
            Text(sentiment.initial)
            Muted(tk(Ns.AI, "Final"))
            Text(sentiment.final)
        }
        val moments = sentiment.keyMoments.orEmpty()
        if (moments.isNotEmpty()) {
            Text(tk(Ns.AI, "Key Moments"), style = MaterialTheme.typography.labelLarge)
            moments.forEach { moment ->
                val glyph = when (moment.sentiment) {
                    SentimentKind.FRUSTRATED -> "\uD83D\uDE24"
                    SentimentKind.NEUTRAL -> "\uD83D\uDE10"
                    SentimentKind.SATISFIED -> "\uD83D\uDE0A"
                    SentimentKind.RELIEVED -> "\uD83D\uDE0C"
                }
                Text("${clock(moment.timestamp)} $glyph " + (moment.reason ?: moment.sentiment.name.lowercase()), style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
    if (strengths.isNotEmpty() || improvements.isNotEmpty()) block(counter, "Agent Performance", null, SectionId.SUPPORT_INSIGHTS, send) {
        if (strengths.isNotEmpty()) {
            Text(tk(Ns.AI, "Strengths"), style = MaterialTheme.typography.labelLarge)
            Bullets(strengths, "✓")
        }
        if (improvements.isNotEmpty()) {
            Text(tk(Ns.AI, "Areas for Improvement"), style = MaterialTheme.typography.labelLarge)
            Bullets(improvements, "→")
        }
    }
    if (kb.isNotEmpty()) block(counter, "Knowledge Base Updates", kb.size, SectionId.KB_UPDATES, send) { Bullets(kb, "\uD83D\uDCDD") }
    if (followUps.isNotEmpty()) block(counter, "Follow-Up Actions", followUps.size, SectionId.FOLLOW_UP_ACTIONS, send) { Bullets(followUps, "☐") }
}

private fun Assessment.shown(): Boolean =
    !position.isNullOrEmpty() || overallRating != null || strengths.orEmpty().isNotEmpty() || concerns.orEmpty().isNotEmpty()

private fun LazyListScope.interviewBlocks(data: InterviewAnalysis, counter: SectionCounter, send: AnalysisSend) {
    val assessment = data.assessment
    val skills = data.skillsAssessment.orEmpty()
    val responses = data.notableResponses.orEmpty()
    val steps = data.nextSteps.orEmpty()
    if (assessment != null && assessment.shown()) block(counter, "Candidate Assessment", null, SectionId.CANDIDATE_ASSESSMENT, send) {
        assessment.position?.takeIf { it.isNotEmpty() }?.let { Muted(tk(Ns.AI, "Position")); Text(it) }
        assessment.overallRating?.let { rating ->
            Muted(tk(Ns.AI, "Overall Rating"))
            Text("★".repeat(rating.toInt().coerceIn(0, 5)) + "☆".repeat(5 - rating.toInt().coerceIn(0, 5)) + " (${rating.toInt()}/5)")
        }
        val strengths = assessment.strengths.orEmpty()
        if (strengths.isNotEmpty()) {
            Text(tk(Ns.AI, "Strengths"), style = MaterialTheme.typography.labelLarge)
            Bullets(strengths, "✓")
        }
        val concerns = assessment.concerns.orEmpty()
        if (concerns.isNotEmpty()) {
            Text(tk(Ns.AI, "Concerns"), style = MaterialTheme.typography.labelLarge)
            Bullets(concerns, "⚠")
        }
    }
    if (skills.isNotEmpty()) block(counter, "Skills Assessment", skills.size, SectionId.SKILLS_ASSESSMENT, send) {
        skills.forEach { skill ->
            val filled = skill.rating.toInt().coerceIn(0, 5)
            Column {
                Text("${skill.skill} · ${skill.level.name.lowercase()}", style = MaterialTheme.typography.bodyMedium)
                Muted("★".repeat(filled) + "☆".repeat(5 - filled) + " ($filled/5)")
            }
        }
    }
    if (responses.isNotEmpty()) block(counter, "Notable Responses", responses.size, SectionId.NOTABLE_RESPONSES, send) {
        responses.forEach { response ->
            val glyph = when (response.assessment) {
                ResponseAssessment.POSITIVE -> "\uD83D\uDC4D"
                ResponseAssessment.NEUTRAL -> "➖"
                ResponseAssessment.NEGATIVE -> "\uD83D\uDC4E"
            }
            Outlined {
                Muted(response.question)
                Text("${response.answer} $glyph", style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
    if (steps.isNotEmpty()) block(counter, "Recommended Next Steps", steps.size, SectionId.INTERVIEW_NEXT_STEPS, send) {
        steps.forEach { step -> Text("${step.priority.name.lowercase()} · ${step.action}", style = MaterialTheme.typography.bodyMedium) }
    }
}
