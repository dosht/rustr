<script lang="ts" generics="Value extends string">
  interface Props {
    label: string;
    value: Value;
    options: readonly { value: Value; label: string; disabled?: boolean }[];
    onchange: (value: Value) => void;
    disabled?: boolean;
    /** Hides the visible label (it stays the accessible name). */
    hideLabel?: boolean;
  }

  let { label, value, options, onchange, disabled = false, hideLabel = false }: Props = $props();
  const id = $props.id();
</script>

<div class="flex flex-col gap-1">
  <label for={id} class={hideLabel ? "sr-only" : "font-medium"}>{label}</label>
  <select
    {id}
    {value}
    {disabled}
    onchange={(event) => onchange(event.currentTarget.value as Value)}
    class="min-h-11 rounded-md border border-border bg-bg px-3 text-fg disabled:opacity-50"
  >
    {#each options as option (option.value)}
      <option value={option.value} disabled={option.disabled}>{option.label}</option>
    {/each}
  </select>
</div>
