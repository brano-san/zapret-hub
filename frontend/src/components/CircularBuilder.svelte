<script lang="ts">
  import { commands, type Analysis, type CircularTarget, type Profile } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { CIRCULAR_PRESETS, CIRCULAR_START, blockSummary, circularFilter, listChoices, variantLabel } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    /** The strategy's text: read for its profiles, rewritten by «Записать в текст». */
    text: string;
  }

  let { text = $bindable() }: Props = $props();

  let analysis = $state<Analysis | null>(null);
  /** `new`, or the index of the profile the builder edits. */
  let target = $state("new");
  let ports = $state("443");
  let l7 = $state("tls");
  let list = $state("");
  let domains = $state("");
  let after = $state(0);
  let fails = $state<number | null>(null);
  let retrans = $state<number | null>(null);
  let extra = $state<string[]>([]);
  let variants = $state<{ id: number; text: string }[]>([]);
  let stop = $state(false);
  let status = $state("");
  let nextId = 0;

  const profiles = $derived((analysis?.profiles ?? []).filter((profile) => profile.number > 0));
  const choices = $derived(listChoices(profiles.map((profile) => profile.filter)));
  const chosen = $derived(target === "new" ? null : (profiles.find((profile) => String(profile.index) === target) ?? null));
  const listToken = $derived(list === "domains" ? (domains.trim() ? `--hostlist-domains=${domains.replace(/\s+/g, "")}` : "") : list);
  const newFilter = $derived(circularFilter(ports, l7, listToken));
  const tcp = $derived((chosen?.filter ?? newFilter).includes("--filter-tcp="));
  const ready = $derived(variants.length > 0 && (target !== "new" || ports.trim() !== ""));

  function variant(text: string): { id: number; text: string } {
    return { id: nextId++, text };
  }

  /** The builder holds `profile` (or a new one): circular's arguments and the variants, a plain profile's desync as variant 1. */
  function load(profile: Profile | null): void {
    status = "";
    fails = profile?.circular?.fails ?? null;
    retrans = profile?.circular?.retrans ?? null;
    extra = profile?.circular?.extra ?? [];
    const texts = !profile ? CIRCULAR_START : profile.variants.length > 0 ? profile.variants.map((v) => v.text) : [profile.plain];
    variants = texts.map(variant);
    stop = profile?.variants.at(-1)?.is_final ?? false;
  }

  async function read(): Promise<void> {
    analysis = await commands.zapret2Profiles(text);
  }

  // The first profile that already is circular, else a new one after the last profile.
  void read().then(() => {
    const first = profiles.find((profile) => profile.circular !== null);
    target = first ? String(first.index) : "new";
    after = profiles.at(-1)?.index ?? 0;
    load(first ?? null);
  });

  // Typing in the text keeps the list of profiles and the remarks current; the variants stay as they are.
  $effect(() => {
    void text;
    const timer = setTimeout(() => void read(), 400);
    return () => clearTimeout(timer);
  });

  function move(i: number, by: number): void {
    const next = [...variants];
    const [item] = next.splice(i, 1);
    if (item) next.splice(i + by, 0, item);
    variants = next;
  }

  async function write(): Promise<void> {
    const where: CircularTarget = chosen ? { Profile: chosen.index } : { New: { after, filter: newFilter } };
    const edit = { settings: { fails, retrans, extra }, variants: variants.map((v) => v.text), stop };
    const written = await call(S.failed.circular, commands.zapret2Circular(text, where, edit));
    if (written === undefined) return;
    text = written;
    await read();
    // A new profile is now in the text: the builder goes on with it, as the text has it.
    const index = chosen ? chosen.index : after + 1;
    target = String(index);
    load(profiles.find((profile) => profile.index === index) ?? null);
    status = S.circular.written;
  }
</script>

<section class="builder">
  <p class="muted">{S.circular.builderHint}</p>
  <div class="row">
    <label for="circular-target">{S.circular.target}</label>
    <select
      id="circular-target"
      class="grow"
      bind:value={target}
      onchange={() => load(chosen)}
    >
      {#each profiles as profile (profile.index)}
        <option value={String(profile.index)}>
          {S.circular.profile(profile.number, blockSummary(profile.filter))}{profile.circular ? " · circular" : ""}
        </option>
      {/each}
      <option value="new">{S.circular.newProfile}</option>
    </select>
  </div>

  {#if target === "new"}
    <div class="grid">
      <label for="circular-ports">{S.circular.ports}</label>
      <input id="circular-ports" bind:value={ports} placeholder="443" />
      <label for="circular-l7">{S.circular.l7}</label>
      <select id="circular-l7" bind:value={l7}>
        <option value="tls">tls</option>
        <option value="http">http</option>
        <option value="tls,http">tls, http</option>
        <option value="">{S.circular.l7Any}</option>
      </select>
      <label for="circular-list">{S.circular.list}</label>
      <select id="circular-list" bind:value={list}>
        <option value="">{S.circular.listNone}</option>
        {#each choices as [token, label] (token)}
          <option value={token}>{label}</option>
        {/each}
        <option value="domains">{S.circular.domains}</option>
      </select>
      {#if list === "domains"}
        <span></span>
        <input bind:value={domains} placeholder={S.circular.domainsPlaceholder} aria-label={S.circular.domains} />
      {/if}
      <label for="circular-after">{S.circular.after}</label>
      <select id="circular-after" bind:value={after}>
        {#each profiles as profile (profile.index)}
          <option value={profile.index}>{S.circular.profile(profile.number, blockSummary(profile.filter))}</option>
        {/each}
      </select>
    </div>
  {/if}
  {#if !tcp}<p class="warn">{S.circular.notTcp}</p>{/if}

  <div class="row settings">
    <label for="circular-fails">{S.circular.fails}</label>
    <input id="circular-fails" type="number" min="1" placeholder="3" bind:value={fails} />
    <label for="circular-retrans">{S.circular.retrans}</label>
    <input id="circular-retrans" type="number" min="1" placeholder="3" bind:value={retrans} />
  </div>
  {#if extra.length > 0}<p class="muted">{S.circular.extra(extra.join(":"))}</p>{/if}

  <ol class="variants">
    {#each variants as item, i (item.id)}
      <li>
        <div class="row">
          <strong>{S.circular.variant(i + 1)}</strong>
          <span class="muted grow">· {variantLabel(item.text)}</span>
          {#if stop && i === variants.length - 1}<span class="badge ok" title={S.circular.finalHint}>{S.circular.final}</span>{/if}
          <select
            aria-label={S.circular.addInstance}
            value=""
            onchange={(event) => {
              const line = event.currentTarget.value;
              event.currentTarget.value = "";
              if (line) item.text = item.text.trim() ? `${item.text.trimEnd()}\n${line}` : line;
            }}
          >
            <option value="">{S.circular.addInstance}</option>
            {#each CIRCULAR_PRESETS as preset (preset.label)}
              <option value={preset.line}>{preset.label}</option>
            {/each}
          </select>
          <button disabled={i === 0} onclick={() => move(i, -1)} title={S.editor.up}>↑</button>
          <button disabled={i === variants.length - 1} onclick={() => move(i, 1)} title={S.editor.down}>↓</button>
          <button class="danger" onclick={() => (variants = variants.filter((v) => v.id !== item.id))} title={S.editor.remove}>✕</button>
        </div>
        <textarea rows="2" spellcheck="false" bind:value={item.text} aria-label={S.circular.variant(i + 1)}></textarea>
      </li>
    {/each}
  </ol>

  <div class="buttons">
    <select
      aria-label={S.circular.addVariant}
      value="-"
      onchange={(event) => {
        const line = event.currentTarget.value;
        event.currentTarget.value = "-";
        if (line !== "-") variants = [...variants, variant(line)];
      }}
    >
      <option value="-">{S.circular.addVariant}…</option>
      <option value="">{S.circular.addEmpty}</option>
      {#each CIRCULAR_PRESETS as preset (preset.label)}
        <option value={preset.line}>{preset.label}</option>
      {/each}
    </select>
    <label class="inline"><input type="checkbox" bind:checked={stop} /> {S.circular.stop}</label>
    <span class="grow"></span>
    <button class="primary" disabled={!ready} onclick={write}>{S.circular.write}</button>
  </div>
  {#if status}<p class="ok">{status}</p>{/if}
  {#each analysis?.errors ?? [] as error (error)}<p class="bad">{error}</p>{/each}
  {#each analysis?.warnings ?? [] as warning (warning)}<p class="warn">{warning}</p>{/each}
</section>

<style>
  .builder {
    margin: 8px 0 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 10px;
    align-items: center;
    margin-top: 8px;
  }

  .settings {
    margin-top: 8px;
    flex-wrap: wrap;
  }

  .settings input {
    width: 70px;
  }

  .variants {
    margin: 10px 0 0;
    padding: 0;
    list-style: none;
  }

  .variants li {
    margin-bottom: 8px;
  }

  .variants button {
    padding: 2px 8px;
  }

  .variants textarea {
    margin-top: 4px;
  }
</style>
