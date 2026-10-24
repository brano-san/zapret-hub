<script lang="ts" generics="T">
  /** A choice to show; a bare value is shown as itself. */
  interface Option<T> {
    value: T;
    label: string;
  }

  import { S } from "$lib/strings";

  let {
    value = $bindable(),
    options,
    disabled = false,
    placeholder = S.common.choose,
    ariaLabel = "",
  }: {
    value: T;
    options: (Option<T> | T)[];
    disabled?: boolean;
    placeholder?: string;
    ariaLabel?: string;
  } = $props();

  let root: HTMLDivElement | undefined = $state();
  let open = $state(false);

  const isOption = <X,>(item: Option<X> | X): item is Option<X> =>
    typeof item === "object" && item !== null && "label" in item;
  const items = $derived(options.map((item) => (isOption(item) ? item : { value: item, label: String(item) })));
  const current = $derived(items.find((item) => Object.is(item.value, value)) ?? null);

  function pick(item: Option<T>): void {
    value = item.value;
    open = false;
  }

  $effect(() => {
    if (!open) return;
    // A click anywhere but the component closes the menu.
    const onDown = (event: PointerEvent) => {
      if (!root?.contains(event.target as Node)) open = false;
    };
    window.addEventListener("pointerdown", onDown);
    return () => window.removeEventListener("pointerdown", onDown);
  });
</script>

<div class="select" bind:this={root}>
  <button
    type="button"
    class="trigger"
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel || undefined}
    onclick={() => (open = !open)}
    onkeydown={(event) => event.key === "Escape" && (open = false)}
  >
    <span class="text" class:placeholder={!current}>{current?.label ?? placeholder}</span>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <path d="m6 9 6 6 6-6" />
    </svg>
  </button>
  {#if open}
    <ul class="menu" role="listbox" aria-label={ariaLabel || undefined}>
      {#each items as item, i (i)}
        <li
          role="option"
          aria-selected={Object.is(item.value, value)}
          class:active={Object.is(item.value, value)}
          tabindex="0"
          onclick={() => pick(item)}
          onkeydown={(event) => (event.key === "Enter" || event.key === " ") && pick(item)}>{item.label}</li
        >
      {/each}
    </ul>
  {/if}
</div>

<style>
  .select {
    position: relative;
    display: inline-block;
  }

  .trigger {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    min-width: 64px;
  }

  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text.placeholder {
    color: var(--muted);
  }

  svg {
    flex: none;
    transition: transform 0.1s;
  }

  .trigger[aria-expanded="true"] svg {
    transform: rotate(180deg);
  }

  .menu {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    z-index: 10;
    min-width: 100%;
    max-height: 260px;
    margin: 0;
    padding: 4px;
    overflow: auto;
    list-style: none;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 4px 14px rgb(0 0 0 / 0.15);
  }

  .menu li {
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    white-space: nowrap;
  }

  .menu li:hover {
    background: var(--hover);
  }

  .menu li.active {
    color: var(--accent);
    font-weight: 600;
  }
</style>
