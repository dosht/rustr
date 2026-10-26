<script lang="ts">
  // Source InterviewAnalysis: assessment, skills, notable responses and next
  // steps. Core already orders next steps high to low.
  import type { InterviewAnalysis } from "$lib/core/types/InterviewAnalysis";
  import type { SectionId } from "$lib/core/types/SectionId";
  import { i18n } from "$lib/i18n/index.svelte";
  import SectionHeader from "./SectionHeader.svelte";

  let { data, oncopy }: { data: InterviewAnalysis; oncopy: (section: SectionId) => void } = $props();
  const assessment = $derived(data.assessment);
  const strengths = $derived(assessment?.strengths ?? []);
  const concerns = $derived(assessment?.concerns ?? []);
  const showAssessment = $derived(!!assessment && (!!assessment.position || assessment.overallRating != null || strengths.length + concerns.length > 0));
  const skills = $derived(data.skillsAssessment ?? []);
  const responses = $derived(data.notableResponses ?? []);
  const steps = $derived(data.nextSteps ?? []);
  const icon = { positive: "👍", neutral: "➖", negative: "👎" } as const;
  const priorityTone = { high: "border-error text-error", medium: "border-progress text-progress", low: "border-border text-muted" } as const;
  const stars = [1, 2, 3, 4, 5];
</script>

<div data-testid="interview-analysis" class="space-y-6">
  {#if assessment && showAssessment}
    <section data-testid="candidate-assessment">
      <SectionHeader title="Candidate Assessment" section="candidate_assessment" {oncopy} />
      <div class="space-y-3 text-sm">
        {#if assessment.position || assessment.overallRating != null}
          <div class="flex flex-wrap items-start justify-between gap-3 rounded-md border border-border p-3">
            {#if assessment.position}
              <div><p class="text-xs text-muted">{i18n.tk("ai", "Position")}</p><p class="font-medium">{assessment.position}</p></div>
            {/if}
            {#if assessment.overallRating != null}
              {@const rating = assessment.overallRating}
              <div>
                <p class="text-xs text-muted">{i18n.tk("ai", "Overall Rating")}</p>
                <div class="flex items-center gap-0.5">
                  {#each stars as star (star)}
                    <span aria-hidden="true" class="text-lg {star <= rating ? 'text-progress' : 'text-border'}">★</span>
                  {/each}
                  <span class="ml-1 text-xs text-muted">({rating}/5)</span>
                </div>
              </div>
            {/if}
          </div>
        {/if}
        {#if strengths.length > 0}
          <div>
            <h4 class="mb-1 font-medium text-success">{i18n.tk("ai", "Strengths")}</h4>
            <ul class="space-y-1">{#each strengths as s, i (i)}<li class="flex gap-2"><span aria-hidden="true" class="text-success">✓</span><span>{s}</span></li>{/each}</ul>
          </div>
        {/if}
        {#if concerns.length > 0}
          <div>
            <h4 class="mb-1 font-medium text-progress">{i18n.tk("ai", "Concerns")}</h4>
            <ul class="space-y-1">{#each concerns as c, i (i)}<li class="flex gap-2"><span aria-hidden="true" class="text-progress">⚠</span><span>{c}</span></li>{/each}</ul>
          </div>
        {/if}
      </div>
    </section>
  {/if}
  {#if skills.length > 0}
    <section data-testid="skills-assessment">
      <SectionHeader title="Skills Assessment" count={skills.length} section="skills_assessment" {oncopy} />
      <div class="space-y-3">
        {#each skills as skill, i (i)}
          <div class="text-sm">
            <div class="mb-1 flex items-center justify-between gap-2">
              <span class="font-medium">{skill.skill}</span>
              <span class="text-xs text-muted">{skill.level}</span>
            </div>
            <div class="flex gap-1" role="img" aria-label={`${skill.rating}/5`}>
              {#each stars as star (star)}
                <div class="h-2 flex-1 rounded {star <= skill.rating ? 'bg-accent' : 'bg-cursor-bg'}"></div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  {#if responses.length > 0}
    <section data-testid="notable-responses">
      <SectionHeader title="Notable Responses" count={responses.length} section="notable_responses" {oncopy} />
      <div class="space-y-3">
        {#each responses as response, i (i)}
          <div class="flex items-start justify-between gap-2 rounded-md border border-border p-3 text-sm">
            <div class="min-w-0">
              <p class="break-words font-medium text-muted">Q: {response.question}</p>
              <p class="mt-1 break-words">{response.answer}</p>
            </div>
            <span aria-label={response.assessment} class="shrink-0">{icon[response.assessment]}</span>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  {#if steps.length > 0}
    <section data-testid="interview-next-steps">
      <SectionHeader title="Recommended Next Steps" count={steps.length} section="interview_next_steps" {oncopy} />
      <ul class="space-y-2">
        {#each steps as step, i (i)}
          <li class="flex items-start gap-2 text-sm">
            <span class="shrink-0 rounded border px-2 py-0.5 text-xs font-medium {priorityTone[step.priority]}">{step.priority}</span>
            <span class="break-words">{step.action}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>
