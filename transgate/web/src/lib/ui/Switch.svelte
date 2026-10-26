<script lang="ts">
  interface Props {
    checked: boolean;
    label: string;
    description?: string;
    disabled?: boolean;
    /** Reports the requested value; the owner decides (often after a confirmation). */
    onchange: (checked: boolean) => void;
  }

  let { checked, label, description, disabled = false, onchange }: Props = $props();
  const id = $props.id();
</script>

<div class="flex items-center justify-between gap-4">
  <div class="flex flex-col">
    <span id="{id}-label" class="font-medium">{label}</span>
    {#if description}<span id="{id}-description" class="text-sm text-muted">{description}</span>{/if}
  </div>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-labelledby="{id}-label"
    aria-describedby={description ? `${id}-description` : undefined}
    {disabled}
    onclick={() => onchange(!checked)}
    class={[
      "relative inline-flex h-7 w-12 shrink-0 items-center rounded-full border border-border transition-colors",
      "disabled:cursor-not-allowed disabled:opacity-50",
      checked ? "bg-accent" : "bg-cursor-bg",
    ]}
  >
    <span
      aria-hidden="true"
      class={["size-5 rounded-full bg-surface shadow transition-transform", checked ? "translate-x-6" : "translate-x-1"]}
    ></span>
  </button>
</div>
