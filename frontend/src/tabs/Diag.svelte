<script lang="ts">
  import { commands, type Answer, type DiagProfile, type Fix, type Group, type Mode, type Summary } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshDiag } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { faultText, fixApplies, fixText, fixWarnings, patternTitle, quickSummary, runDate } from "$lib/format";
  import { S } from "$lib/strings";
  import CopyButton from "../components/CopyButton.svelte";
  import PathTable from "../components/PathTable.svelte";

  /** The «Любая программа» choice of the profile list. */
  const ANY = "";
  const MODES: Mode[] = ["ask", "auto"];
  const ANSWERS: [Answer, string][] = [
    ["Yes", S.diag.yes],
    ["No", S.diag.no],
    ["Unknown", S.diag.unknown],
  ];

  let profiles = $state<DiagProfile[]>([]);
  let profile = $state(ANY);
  let exe = $state("");
  let mode = $state<Mode>("ask");
  let reports = $state<[string, string][]>([]);
  let shown = $state<Summary | null>(null);
  let details = $state(false);
  let status = $state("");
  /** The culprits left ticked while the diagnostics ask. */
  let keys = $state<string[]>([]);
  // The profile is chosen once; later reloads keep what the user picked.
  let picked = false;

  const live = $derived(app.diagLive);
  const quick = $derived(app.overview?.quick ?? null);
  const locked = $derived(app.busy || Boolean(app.overview?.testing) || live !== null);
  // A new set of culprits ticks all of them again; the same set read again keeps the user's choice.
  const choosing = $derived(live?.choosing?.map((group) => group.key).join("\n") ?? "");

  $effect(() => {
    keys = choosing ? choosing.split("\n") : [];
  });

  async function openReport(id: string): Promise<void> {
    const next = (await call(S.failed.diagReport, commands.diagReport(id))) ?? null;
    if (next?.id !== shown?.id) details = false;
    status = "";
    shown = next;
  }

  async function load(): Promise<void> {
    profiles = (await call(S.failed.diagProfiles, commands.diagProfiles())) ?? [];
    reports = (await call(S.failed.diagReports, commands.diagReports())) ?? [];
    if (!picked && profiles[0]) {
      profile = profiles[0].id;
      picked = true;
    }
    if (!shown && reports[0]) await openReport(reports[0][0]);
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  $effect(() => {
    const id = app.diagFinished;
    if (id) {
      app.diagFinished = null;
      void openReport(id);
    }
  });

  async function begin(): Promise<void> {
    const any = profile === ANY;
    const request = { profile: any ? null : profile, app: any ? exe.trim() : null, mode, observe_secs: null, budget_min: null };
    if ((await change(S.failed.diagStart, () => commands.startDiag(request))) !== undefined) await refreshDiag();
  }

  /** «Проверить снова»: the report's profile again, or the form's choice for a program. */
  async function again(report: Summary): Promise<void> {
    const same = profiles.find((item) => item.name === report.profile);
    if (same) profile = same.id;
    await begin();
  }

  async function apply(report: Summary, fix: Fix, index: number): Promise<void> {
    const text = [fixText(fix), ...fixWarnings(fix, report.fixes, report.warnings)].join("\n\n");
    if (!(await confirmation.ask({ title: S.diag.applyTitle, text, yes: S.common.apply }))) return;
    status = "";
    const check = await change(S.failed.diagApply, () => commands.diagApply(report.id, index));
    if (check === undefined) return;
    status = check ? `${check.ok ? S.editor.appliedOk : S.editor.appliedFailed}\n${quickSummary(check)}` : S.diag.applied;
  }

  const title = (id: string | null, path: string | null) =>
    profiles.find((item) => item.id === id)?.name ?? path ?? S.diag.anyProgram;
</script>

<section class="card">
  <h2>{S.overview.quick}</h2>
  <p>
    {#if quick}
      <span class={quick.ok ? "ok" : "bad"}>{quick.ok ? S.overview.quickOk : S.overview.quickFailed}</span>
    {:else}
      <span class="muted">{S.overview.quickNever}</span>
    {/if}
  </p>
  {#if quick}<pre>{quickSummary(quick)}</pre>{/if}
  <div class="buttons">
    <button disabled={locked} onclick={() => change(S.failed.quick, () => commands.quickCheck())}>{S.overview.quick}</button>
  </div>
</section>

{#if live}
  {@const step = live.progress}
  <section class="card">
    <h2>{title(live.request.profile, live.request.app)} · {S.diag.modes[live.request.mode]}</h2>
    {#if step === null}
      <p>{S.diag.preparing}</p>
    {:else if step.step === "Observing"}
      <p>{S.diag.observing(step.secs_left)}</p>
      {#if step.flows.length === 0}
        <p class="muted">{S.diag.noFlows}</p>
      {:else}
        <table>
          <thead><tr><th>{S.diag.address}</th><th>{S.diag.protocol}</th><th>{S.diag.packets}</th></tr></thead>
          <tbody>
            {#each step.flows as flow (`${flow.protocol} ${flow.local_port} ${flow.dst}`)}
              <tr>
                <td>{flow.dst}{#if flow.domain ?? flow.sni}<span class="muted"> · {flow.domain ?? flow.sni}</span>{/if}</td>
                <td>{S.diag.protocols[flow.protocol]}</td>
                <td>{flow.up_packets}/{flow.down_packets}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {:else if step.step === "Culprits"}
      <h3>{S.diag.culprits}</h3>
      {#if live.choosing}
        <p class="muted">{S.diag.chooseHint}</p>
        {#each live.choosing as group (group.key)}
          <label class="inline">
            <input type="checkbox" bind:group={keys} value={group.key} />
            {@render culprit(group)}
          </label>
        {/each}
        <div class="buttons">
          <button
            class="primary"
            disabled={app.busy || keys.length === 0}
            onclick={() => change(S.failed.diagChoose, () => commands.diagChoose(keys))}
          >
            {S.diag.choose}
          </button>
        </div>
      {:else}
        {#each step.groups as group (group.key)}<p>{@render culprit(group)}</p>{/each}
      {/if}
    {:else if step.step === "Path"}
      <p>{S.diag.path(step.group, step.index, step.total, step.label)}</p>
      <progress class="wide" max={step.total} value={step.index}></progress>
    {:else if step.step === "Question"}
      <p>{step.label}</p>
    {:else if step.step === "Rechecking"}
      <p>{S.diag.rechecking}</p>
    {:else}
      <p>{S.diag.rollingBack}</p>
    {/if}
    <!-- The paths of this culprit tried so far (diag spec 10); a new culprit starts an empty table. -->
    {#if live.results?.length && (step?.step === "Path" || step?.step === "Question")}
      <PathTable results={live.results} />
    {/if}
    <div class="buttons">
      <button class="danger" disabled={step?.step === "RollingBack"} onclick={() => commands.cancelDiag()}>{S.common.cancel}</button>
    </div>
  </section>

  {#if live.question}
    <section class="card question">
      <p><strong>{live.question}</strong></p>
      <div class="buttons">
        {#each ANSWERS as [answer, label] (answer)}
          <button class:primary={answer === "Yes"} disabled={app.busy} onclick={() => change(S.failed.diagAnswer, () => commands.diagAnswer(answer))}>
            {label}
          </button>
        {/each}
      </div>
    </section>
  {/if}
{:else}
  <section class="card">
    <h2>{S.diag.newTitle}</h2>
    <div class="row">
      <label for="diag-profile">{S.diag.what}</label>
      <select id="diag-profile" bind:value={profile}>
        {#each profiles as item (item.id)}<option value={item.id}>{item.name}</option>{/each}
        <option value={ANY}>{S.diag.anyProgram}</option>
      </select>
    </div>
    {#if profile === ANY}
      <div class="row">
        <label for="diag-exe">{S.diag.exe}</label>
        <input id="diag-exe" class="grow" bind:value={exe} placeholder={S.diag.exeHint} />
      </div>
    {:else}
      {@const hint = profiles.find((item) => item.id === profile)?.hint}
      {#if hint}<p class="muted">{hint}</p>{/if}
    {/if}
    <div class="row">
      <span>{S.diag.mode}</span>
      {#each MODES as value (value)}
        <label class="inline"><input type="radio" bind:group={mode} {value} /> {S.diag.modes[value]}</label>
      {/each}
    </div>
    <p class="muted">{S.diag.startHint}</p>
    <div class="buttons">
      <button class="primary" disabled={locked || (profile === ANY && !exe.trim())} onclick={begin}>{S.diag.start}</button>
    </div>
  </section>

  {#if shown}
    {@const report = shown}
    <section class="card">
      <h2>{S.diag.reportTitle(runDate(report.id), report.profile)}</h2>
      {#if report.canceled || report.budget_hit}
        <p class="muted">{[report.canceled && S.diag.canceled, report.budget_hit && S.diag.budgetHit].filter(Boolean).join(" · ")}</p>
      {/if}
      <p><strong>{report.verdict_text}</strong></p>
      {#if report.rollback_error}<pre class="bad">{report.rollback_error}</pre>{/if}
      {#if report.sources_missing.length > 0}<p class="muted">{S.diag.sourcesMissing(report.sources_missing)}</p>{/if}
      {#each report.explanations as text, i (i)}<p>{text}</p>{/each}

      {#if report.system_checks.length > 0}
        {@const failed = report.system_checks.filter((check) => check.level !== "Ok")}
        <h3>{S.checks.title}</h3>
        {#if failed.length === 0}<p class="muted">{S.diag.checksOk}</p>{/if}
        {#each failed as check (check.id)}
          <p>
            <strong>{S.checks.names[check.id]}</strong>
            <span class="muted">— {S.checks.level[check.level]}{#if check.detail}: {check.detail}{/if}</span>
            <br />{S.checks.hints[check.id]}
          </p>
        {/each}
      {/if}

      {#if report.patterns.length > 0}
        <h3>{S.diag.patterns}</h3>
        {#each report.patterns as pattern, i (i)}<p>{patternTitle(pattern)}</p>{/each}
      {/if}

      {#if report.fixes.length > 0}
        <h3>{S.diag.fixes}</h3>
        {#each report.fixes as fix, i (i)}
          <div class="fix">
            <span>{fixText(fix)}{#if !fixApplies(fix)}<span class="muted"> ({S.diag.manual})</span>{/if}</span>
            {#if fixApplies(fix)}
              <button class="primary" disabled={locked} onclick={() => apply(report, fix, i)}>{S.common.apply}</button>
            {:else if fix.kind === "Manual"}
              <button disabled={locked} onclick={() => again(report)}>{S.diag.recheck}</button>
            {/if}
          </div>
        {/each}
        {#each report.warnings as warning, i (i)}<p class="warn">{warning}</p>{/each}
        {#if status}<pre>{status}</pre>{/if}
      {/if}

      {#each report.groups as summary (summary.group.key)}
        <h3>{summary.group.key}</h3>
        <p class="muted">{summary.seen}</p>
        <p>{summary.explanation}</p>
        {#if summary.results.length > 0}<PathTable results={summary.results} best={summary.best} />{/if}
        {#if summary.notes.length > 0}
          <ul class="muted">
            {#each summary.notes as note, i (i)}<li>{note}</li>{/each}
          </ul>
        {/if}
      {/each}

      <div class="buttons">
        <button onclick={() => (details = !details)}>{details ? S.diag.hideDetails : S.diag.details}</button>
        <CopyButton text={() => JSON.stringify(report, null, 2)} />
      </div>
      {#if details}<pre>{JSON.stringify(report, null, 2)}</pre>{/if}
    </section>
  {/if}
{/if}

<section class="card">
  <h2>{S.diag.history}</h2>
  {#if reports.length === 0}<p class="muted">{S.diag.noReports}</p>{/if}
  <table>
    <tbody>
      {#each reports as [id, name] (id)}
        <tr class:selected={shown?.id === id}>
          <td><button class="link" onclick={() => openReport(id)}>{runDate(id)}</button></td>
          <td>{name}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

{#snippet culprit(group: Group)}
  <span>
    {group.key} · {S.diag.protocols[group.protocol]}{#if group.ports.length > 0}&nbsp;{group.ports.join(",")}{/if}
    <span class="muted">— {faultText(group.fault)}</span>
  </span>
{/snippet}

<style>
  /* The question waits for the user: it stands out from the progress above it. */
  .question {
    border-color: var(--warn);
  }

  .fix {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
  }

  .fix > span {
    flex: 1;
  }

  /* An address or a long explanation has no space to wrap at: it breaks anywhere rather than spill out. */
  p,
  td {
    overflow-wrap: anywhere;
  }
</style>
