<script lang="ts">
  // Source LectureAnalysis: topics, concepts, review questions, study guide.
  import type { LectureAnalysis } from "$lib/core/types/LectureAnalysis";
  import type { SectionId } from "$lib/core/types/SectionId";
  import { i18n } from "$lib/i18n/index.svelte";
  import { clock, duration } from "../../format";
  import SectionHeader from "./SectionHeader.svelte";

  let { data, oncopy }: { data: LectureAnalysis; oncopy: (section: SectionId) => void } = $props();
  const topics = $derived(data.topics ?? []);
  const concepts = $derived(data.concepts ?? []);
  const questions = $derived(data.reviewQuestions ?? []);
  const guide = $derived(data.studyGuide ?? []);
</script>

<div data-testid="lecture-analysis" class="space-y-6">
  {#if topics.length > 0}
    <section data-testid="topics-list">
      <SectionHeader title="Topics Covered" count={topics.length} section="topics" {oncopy} />
      <ul class="space-y-2">
        {#each topics as topic, i (i)}
          <li class="flex items-start gap-3 text-sm">
            <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-cursor-bg text-xs font-medium text-accent">{i + 1}</span>
            <div class="min-w-0 flex-1">
              <p class="break-words font-medium">{topic.title}</p>
              <div class="flex gap-3 text-xs text-muted">
                {#if topic.timestamp != null}<span>{i18n.tk("ai", "Starts:")} {clock(topic.timestamp)}</span>{/if}
                {#if topic.duration != null}<span>{i18n.tk("ai", "Duration:")} {duration(topic.duration)}</span>{/if}
              </div>
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
  {#if concepts.length > 0}
    <section data-testid="concepts-list">
      <SectionHeader title="Key Concepts" count={concepts.length} section="concepts" {oncopy} />
      <div class="space-y-3">
        {#each concepts as concept, i (i)}
          <div class="rounded-md border border-border p-3 text-sm">
            <h4 class="break-words font-medium">{concept.concept}</h4>
            <p class="mt-1 break-words text-muted">{concept.explanation}</p>
            {#if concept.examples && concept.examples.length > 0}
              <div class="mt-2">
                <span class="text-xs font-medium">{i18n.tk("ai", "Examples:")}</span>
                <ul class="mt-1 list-disc pl-5 text-xs text-muted">
                  {#each concept.examples as example, j (j)}<li>{example}</li>{/each}
                </ul>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/if}
  {#if questions.length > 0}
    <section data-testid="review-questions">
      <SectionHeader title="Review Questions" count={questions.length} section="review_questions" {oncopy} />
      <ol class="space-y-2">
        {#each questions as question, i (i)}
          <li class="flex gap-2 text-sm"><span class="shrink-0 font-medium text-accent">{i + 1}.</span><span class="break-words">{question}</span></li>
        {/each}
      </ol>
    </section>
  {/if}
  {#if guide.length > 0}
    <section data-testid="study-guide">
      <SectionHeader title="Study Guide" count={guide.length} section="study_guide" {oncopy} />
      <ul class="space-y-2">
        {#each guide as item, i (i)}
          <li class="flex gap-2 text-sm"><span aria-hidden="true">📚</span><span class="break-words">{item}</span></li>
        {/each}
      </ul>
    </section>
  {/if}
</div>
