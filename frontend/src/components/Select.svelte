<script lang="ts" generics="T">
  /** A choice to show; a bare value is shown as itself. */
  interface Option<T> {
    value: T;
    label: string;
  }

  import { moveActive } from "$lib/select";
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

  // Focus stays on the trigger while the menu is open: the highlighted option is its active descendant.
  const id = $props.id();
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let open = $state(false);
  /** The highlighted option, which Enter or Space picks. */
  let active = $state(0);

  const isOption = <X,>(item: Option<X> | X): item is Option<X> =>
    typeof item === "object" && item !== null && "label" in item;
  const items = $derived(options.map((item) => (isOption(item) ? item : { value: item, label: String(item) })));
  const selected = $derived(items.findIndex((item) => Object.is(item.value, value)));
  const current = $derived(items[selected] ?? null);

  function show(): void {
    active = Math.max(selected, 0);
    open = true;
  }

  function pick(item: Option<T>): void {
    value = item.value;
    open = false;
    trigger?.focus();
  }

  function onKey(event: KeyboardEvent): void {
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
        event.preventDefault();
        show();
      }
      return;
    }
    const item = items[active];
    if (event.key === "Escape") {
      event.preventDefault();
      open = false;
    } else if (event.key === "Tab") {
      open = false;
    } else if ((event.key === "Enter" || event.key === " ") && item) {
      event.preventDefault();
      pick(item);
    } else {
      const next = moveActive(event.key, active, items.map((option) => option.label));
      if (next !== null) {
        event.preventDefault();
        active = next;
        document.getElementById(`${id}-${next}`)?.scrollIntoView({ block: "nearest" });
      }
    }
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

<div
  class="select"
  bind:this={root}
  onfocusout={(event) => {
    // Focus gone elsewhere (Tab, a click outside): the menu goes with it.
    if (!root?.contains(event.relatedTarget as Node | null)) open = false;
  }}
>
  <button
    bind:this={trigger}
    type="button"
    class="trigger"
    role="combobox"
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={open ? `${id}-menu` : undefined}
    aria-activedescendant={open ? `${id}-${active}` : undefined}
    aria-label={ariaLabel || undefined}
    onclick={() => (open ? (open = false) : show())}
    onkeydown={onKey}
  >
    <span class="text" class:placeholder={!current}>{current?.label ?? placeholder}</span>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <path d="m6 9 6 6 6-6" />
    </svg>
  </button>
  {#if open}
    <ul class="menu" id="{id}-menu" role="listbox" aria-label={ariaLabel || undefined}>
      {#each items as item, i (i)}
        <!-- Options take no focus (it stays on the trigger); a press does not move it either. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="{id}-{i}"
          role="option"
          aria-selected={i === selected}
          class:active={i === selected}
          class:highlight={i === active}
          onpointerdown={(event) => event.preventDefault()}
          onpointerenter={() => (active = i)}
          onclick={() => pick(item)}
        >
          {item.label}
        </li>
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

  .menu li.highlight {
    background: var(--hover);
  }

  .menu li.active {
    color: var(--accent);
    font-weight: 600;
  }
</style>
