<script lang="ts">
  import type { HTMLInputAttributes, HTMLTextareaAttributes } from "svelte/elements";

  type Shared = {
    label: string;
    value: string;
    /** Visible hint under the field. */
    hint?: string;
    /** Validation message; marks the field invalid and is announced. */
    error?: string | null;
    oninput: (value: string) => void;
  };

  type Props =
    | (Shared & { multiline?: false } & Omit<HTMLInputAttributes, "value" | "oninput" | "children">)
    | (Shared & { multiline: true; rows?: number } & Omit<HTMLTextareaAttributes, "value" | "oninput" | "children">);

  let { label, value, hint, error = null, oninput, multiline = false, class: extra, ...rest }: Props = $props();
  const id = $props.id();
  const describedBy = $derived([hint && `${id}-hint`, error && `${id}-error`].filter(Boolean).join(" ") || undefined);
  const field = "w-full rounded-md border bg-bg px-3 py-2 text-fg disabled:opacity-50";
</script>

<div class={["flex flex-col gap-1", extra]}>
  <label for={id} class="font-medium">{label}</label>
  {#if multiline}
    <textarea
      {...rest as HTMLTextareaAttributes}
      {id}
      {value}
      aria-invalid={error ? true : undefined}
      aria-describedby={describedBy}
      oninput={(event) => oninput(event.currentTarget.value)}
      class={[field, error ? "border-error" : "border-border"]}
    ></textarea>
  {:else}
    <input
      {...rest as HTMLInputAttributes}
      {id}
      {value}
      aria-invalid={error ? true : undefined}
      aria-describedby={describedBy}
      oninput={(event) => oninput(event.currentTarget.value)}
      class={[field, "min-h-11", error ? "border-error" : "border-border"]}
    />
  {/if}
  {#if hint}<p id="{id}-hint" class="text-sm text-muted">{hint}</p>{/if}
  {#if error}<p id="{id}-error" class="text-sm text-error">{error}</p>{/if}
</div>
