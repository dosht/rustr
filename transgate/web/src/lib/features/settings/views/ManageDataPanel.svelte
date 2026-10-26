<script lang="ts">
  // Settings "Manage Data" tab (source ManageDataView.tsx:118-280), the same on
  // desktop and phone inside the settings dialog. Every decision (which toggle
  // asks first, erasure honesty) is core's DataManagementVm.
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Info from "@lucide/svelte/icons/info";
  import type { DataManagementVm } from "$lib/core/types/DataManagementVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import Switch from "$lib/ui/Switch.svelte";

  interface Props {
    vm: DataManagementVm;
    kind: "dialog" | "sheet";
    oncleanup: (on: boolean) => void;
    ondeletemonthly: (on: boolean) => void;
    ondeleteall: () => void;
    ondeleteaccount: () => void;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { vm, kind, oncleanup, ondeletemonthly, ondeleteall, ondeleteaccount, onconfirm, oncancel }: Props = $props();
  const t = (key: string) => i18n.tk("common", key);
</script>

<div class="flex flex-col gap-5 text-sm">
  <section>
    <p class="mb-3"><span class="font-bold text-accent">Transgate</span> {t("Privacy Information:")}</p>
    <ul class="list-disc pl-5">
      {#each vm.privacy_info as line (line.key)}
        <li>{i18n.t(line)}</li>
      {/each}
    </ul>
  </section>

  <section class="flex flex-col gap-3">
    <h3 class="text-base font-semibold">{t("Data Retention")}</h3>
    <div class="flex items-start gap-2 rounded-md border border-border bg-notice-bg p-3">
      <Info size={20} aria-hidden="true" class="mt-0.5 shrink-0 text-accent" />
      <div>
        <p class="mb-1 font-medium">{t("Automatic Transcript Cleanup Policy")}</p>
        <p>{i18n.t(vm.cleanup_policy)}</p>
        {#if vm.cleanup_warning}
          <p class="mt-2 font-medium text-error">{i18n.t(vm.cleanup_warning)}</p>
        {/if}
      </div>
    </div>
    <div class="border-t border-border pt-2">
      <Switch
        checked={vm.cleanup_on}
        label={t("Automatic transcript cleanup (30 days)")}
        description={`${t("Automatically delete transcripts older than 30 days to save storage. You can disable this to keep transcripts indefinitely.")}${
          vm.cleanup_on
            ? t("Transcripts older than 30 days will be automatically deleted daily. Disable to keep transcripts indefinitely.")
            : t("Automatic cleanup disabled. Your transcripts will be kept indefinitely, which may increase storage usage.")
        }`}
        onchange={oncleanup}
      />
    </div>
  </section>

  <section class="flex flex-col gap-3">
    <h3 class="text-base font-semibold">{t("Legacy Settings")}</h3>
    <div class="border-t border-border pt-2">
      <Switch checked={vm.delete_monthly_on} label={t("Delete my data automatically")} onchange={ondeletemonthly} />
    </div>
  </section>

  <section class="flex flex-col gap-2">
    <h3 class="text-base font-semibold">{t("Manual Actions")}</h3>
    <div class="flex items-center gap-3 border-t border-border pt-2">
      <p class="grow">{t("Delete all Transcripted Files")}</p>
      <Button variant="danger" size="sm" onclick={ondeleteall}>{t("Delete All")}</Button>
    </div>
    <div class="flex items-center gap-3 border-t border-border pt-2">
      <p class="grow">{t("Delete my Account")}</p>
      <Button variant="danger" size="sm" onclick={ondeleteaccount}>{t("Delete Account")}</Button>
    </div>
    <div class="flex items-center gap-2 border-t border-border pt-2 text-error">
      <TriangleAlert size={24} aria-hidden="true" class="shrink-0" />
      <p>{i18n.t(vm.files_gone_note)}</p>
    </div>
  </section>

  {#if vm.erasure}
    <Alert tone="info">{i18n.t(vm.erasure)}</Alert>
  {/if}
  {#if vm.error}
    <Alert tone="error">{i18n.t(vm.error)}</Alert>
  {/if}
</div>

<ConfirmDialog
  open={vm.confirm !== null}
  {kind}
  title={vm.confirm ? i18n.t(vm.confirm.title) : ""}
  description={vm.confirm ? i18n.t(vm.confirm.description) : ""}
  confirmLabel={vm.confirm ? i18n.t(vm.confirm.confirm_label) : ""}
  cancelLabel={t("No")}
  tone={vm.confirm?.kind === "disable_cleanup" ? "neutral" : "danger"}
  {onconfirm}
  {oncancel}
/>
