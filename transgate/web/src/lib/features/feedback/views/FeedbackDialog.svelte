<script lang="ts">
  // The job feedback dialog (source FeedbackModal.tsx:120-248). Desktop and
  // phone views differ only in how the dialog is presented, so both render
  // this body. Pure: the view model in, intents out.
  import type { FeedbackVm } from "$lib/core/types/FeedbackVm";
  import type { FeedbackRating } from "$lib/core/types/FeedbackRating";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import disappointed from "../art/disappointed.svg?url";
  import satisfied from "../art/satisfied.svg?url";
  import outstanding from "../art/outstanding.svg?url";

  interface Props {
    vm: FeedbackVm;
    kind: "dialog" | "sheet";
    onselect: (rating: FeedbackRating) => void;
    oncomment: (text: string) => void;
    onsubmit: () => void;
    onclose: () => void;
  }

  let { vm, kind, onselect, oncomment, onsubmit, onclose }: Props = $props();

  const artwork: Record<FeedbackRating, string> = { 0: disappointed, 5: satisfied, 10: outstanding };
  const t = (key: string) => i18n.tk("common", key);
  const group = $props.id();
</script>

{#snippet closeAction()}
  <Button variant="secondary" onclick={onclose}>{t("Close")}</Button>
{/snippet}

<Dialog
  open={vm.open}
  {kind}
  title={vm.thanked ? t("Thank You!") : t("Rate Your Experience")}
  locked={vm.submitting}
  {onclose}
  actions={vm.thanked ? closeAction : undefined}
>
  {#if vm.thanked}
    <div class="py-4 text-center">
      <p class="mb-4 text-lg">{t("Thank you for your feedback!")}</p>
      <p class="text-muted">{t("Your response helps us improve our service.")}</p>
    </div>
  {:else}
    <form
      class="flex flex-col gap-4"
      onsubmit={(event) => {
        event.preventDefault();
        onsubmit();
      }}
    >
      <fieldset class="flex flex-col gap-3" disabled={vm.submitting}>
        <legend class="mb-3 text-muted">{t("How was your experience?")}</legend>
        <div class="flex gap-2 sm:gap-4">
          {#each vm.options as option (option.rating)}
            <label
              class={[
                "flex flex-1 cursor-pointer flex-col items-center rounded-lg px-2 py-4",
                "has-focus-visible:outline-3 has-focus-visible:outline-focus",
                option.selected ? "bg-cursor-bg ring-2 ring-accent" : "grayscale hover:bg-cursor-bg hover:grayscale-0",
              ]}
            >
              <input
                type="radio"
                name="{group}-rating"
                class="sr-only"
                value={option.rating}
                checked={option.selected}
                onchange={() => onselect(option.rating)}
              />
              <img class="mb-2 size-12 sm:size-16 md:size-20" src={artwork[option.rating]} alt="" />
              <span class="text-center text-sm font-medium">{i18n.t(option.label)}</span>
            </label>
          {/each}
        </div>
      </fieldset>
      <TextField
        multiline
        rows={4}
        label={t("Tell us more about your experience")}
        placeholder={t("Share your thoughts...")}
        value={vm.comment}
        disabled={vm.submitting}
        oninput={oncomment}
      />
      {#if vm.notice}
        <Alert tone="error">{i18n.t(vm.notice)}</Alert>
      {/if}
      <div class="flex flex-col-reverse justify-center gap-3 sm:flex-row">
        <Button variant="secondary" disabled={vm.submitting} onclick={onclose}>{t("Cancel")}</Button>
        <Button variant="primary" type="submit" busy={vm.submitting}>
          {vm.submitting ? t("Submitting...") : t("Submit")}
        </Button>
      </div>
    </form>
  {/if}
</Dialog>
