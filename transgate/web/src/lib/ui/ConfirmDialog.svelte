<script lang="ts">
  import Button from "./Button.svelte";
  import Dialog from "./Dialog.svelte";

  interface Props {
    open: boolean;
    title: string;
    description: string;
    confirmLabel: string;
    cancelLabel: string;
    /** `danger` for destructive and irreversible actions. */
    tone?: "danger" | "neutral";
    /** The confirmed action is in flight: no second confirm, no dismissal. */
    busy?: boolean;
    kind?: "dialog" | "sheet";
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { open, title, description, confirmLabel, cancelLabel, tone = "neutral", busy = false, kind = "dialog", onconfirm, oncancel }: Props = $props();
</script>

<Dialog {open} {title} {kind} locked={busy} onclose={oncancel}>
  <p class="text-muted">{description}</p>
  {#snippet actions()}
    <Button variant="ghost" disabled={busy} onclick={oncancel}>{cancelLabel}</Button>
    <Button variant={tone === "danger" ? "danger" : "primary"} {busy} onclick={onconfirm}>{confirmLabel}</Button>
  {/snippet}
</Dialog>
