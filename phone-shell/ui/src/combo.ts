/**
 * A filterable combo box for values that load as the user types.
 *
 * Focus stays on the input. The list follows the combobox pattern, so arrow
 * keys, Enter, Escape and screen-reader labels work without a native select.
 */

export interface ComboOption {
  id: string;
  label: string;
  meta?: string;
}

export interface ComboConfig {
  id: string;
  label: string;
  value: string | null;
  selectedLabel?: string;
  placeholder?: string;
  required?: boolean;
  loadingLabel: string;
  emptyLabel: string;
  failedLabel: string;
  onChange: (id: string | null) => void;
  load: (query: string, signal: AbortSignal) => Promise<ComboOption[]>;
}

export interface ComboHandle {
  element: HTMLElement;
  setValue: (id: string | null, label: string) => void;
}

const LOAD_DELAY_MS = 200;

export function combo(config: ComboConfig): ComboHandle {
  const wrapper = document.createElement('div');
  wrapper.className = 'field combo';

  const label = document.createElement('label');
  label.className = 'field-label';
  label.textContent = config.label;
  label.htmlFor = config.id;

  const box = document.createElement('div');
  box.className = 'combo-box';

  const input = document.createElement('input');
  input.id = config.id;
  input.className = 'text-input combo-input';
  input.type = 'text';
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.placeholder = config.placeholder ?? '';
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-expanded', 'false');
  input.setAttribute('aria-autocomplete', 'list');
  input.setAttribute('aria-controls', `${config.id}-list`);
  if (config.required) {
    input.required = true;
  }
  input.value = config.selectedLabel ?? '';

  const status = document.createElement('span');
  status.className = 'combo-status';
  status.setAttribute('role', 'status');
  status.setAttribute('aria-live', 'polite');

  const list = document.createElement('ul');
  list.id = `${config.id}-list`;
  list.className = 'combo-list';
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', config.label);
  list.hidden = true;

  box.append(input, status, list);
  wrapper.append(label, box);

  let options: ComboOption[] = [];
  let selectedId = config.value;
  let activeIndex = -1;
  let loading = false;
  let failed = false;
  let open = false;
  let token = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let controller: AbortController | null = null;

  function optionId(index: number): string {
    return `${config.id}-option-${index}`;
  }

  function setOpen(next: boolean): void {
    open = next;
    list.hidden = !next;
    input.setAttribute('aria-expanded', next ? 'true' : 'false');
    if (!next) {
      input.removeAttribute('aria-activedescendant');
      activeIndex = -1;
    }
  }

  function renderList(): void {
    list.replaceChildren();
    if (loading) {
      status.textContent = config.loadingLabel;
      list.append(note(config.loadingLabel));
      return;
    }
    status.textContent = '';
    if (failed) {
      list.append(note(config.failedLabel));
      return;
    }
    if (options.length === 0) {
      list.append(note(config.emptyLabel));
      return;
    }
    options.forEach((option, index) => {
      const item = document.createElement('li');
      item.id = optionId(index);
      item.className = index === activeIndex ? 'combo-option is-active' : 'combo-option';
      item.setAttribute('role', 'option');
      item.setAttribute('aria-selected', option.id === selectedId ? 'true' : 'false');
      const name = document.createElement('span');
      name.className = 'combo-option-label';
      name.textContent = option.label;
      item.append(name);
      if (option.meta) {
        const meta = document.createElement('span');
        meta.className = 'combo-option-meta';
        meta.textContent = option.meta;
        item.append(meta);
      }
      item.addEventListener('mousedown', (event) => {
        event.preventDefault();
      });
      item.addEventListener('click', () => choose(option));
      list.append(item);
    });
    if (activeIndex >= 0 && activeIndex < options.length) {
      input.setAttribute('aria-activedescendant', optionId(activeIndex));
    } else {
      input.removeAttribute('aria-activedescendant');
    }
  }

  function note(text: string): HTMLElement {
    const item = document.createElement('li');
    item.className = 'combo-note';
    item.setAttribute('role', 'presentation');
    item.textContent = text;
    return item;
  }

  function choose(option: ComboOption): void {
    selectedId = option.id;
    input.value = option.label;
    config.onChange(option.id);
    setOpen(false);
    renderList();
    input.focus();
  }

  function moveActive(step: number): void {
    if (!open) {
      void loadNow('');
      setOpen(true);
    }
    if (options.length === 0) {
      return;
    }
    activeIndex =
      activeIndex < 0
        ? step > 0
          ? 0
          : options.length - 1
        : (activeIndex + step + options.length) % options.length;
    renderList();
  }

  async function loadNow(query: string): Promise<void> {
    token += 1;
    const current = token;
    controller?.abort();
    controller = new AbortController();
    loading = true;
    failed = false;
    if (open) {
      renderList();
    }
    try {
      const next = await config.load(query, controller.signal);
      if (current !== token) {
        return;
      }
      options = next;
      activeIndex = next.length > 0 ? 0 : -1;
    } catch {
      if (current !== token) {
        return;
      }
      options = [];
      failed = true;
    } finally {
      if (current === token) {
        loading = false;
        if (open) {
          renderList();
        }
      }
    }
  }

  function scheduleLoad(query: string): void {
    if (timer) {
      clearTimeout(timer);
    }
    timer = setTimeout(() => {
      void loadNow(query);
    }, LOAD_DELAY_MS);
  }

  input.addEventListener('focus', () => {
    if (!open) {
      setOpen(true);
      scheduleLoad(input.value === (config.selectedLabel ?? '') ? '' : input.value);
    }
  });

  input.addEventListener('input', () => {
    selectedId = null;
    config.onChange(null);
    setOpen(true);
    scheduleLoad(input.value);
  });

  input.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveActive(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveActive(-1);
    } else if (event.key === 'Enter') {
      if (open && activeIndex >= 0 && options[activeIndex]) {
        event.preventDefault();
        choose(options[activeIndex]);
      }
    } else if (event.key === 'Home' && open && options.length > 0) {
      event.preventDefault();
      activeIndex = 0;
      renderList();
    } else if (event.key === 'End' && open && options.length > 0) {
      event.preventDefault();
      activeIndex = options.length - 1;
      renderList();
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      setOpen(false);
    } else if (event.key === 'Tab') {
      setOpen(false);
    }
  });

  input.addEventListener('blur', () => {
    // A click on an option cancels the blur at mousedown, so this closes only
    // when focus really leaves the field.
    setTimeout(() => {
      if (document.activeElement !== input) {
        setOpen(false);
      }
    }, 0);
  });

  return {
    element: wrapper,
    setValue(id: string | null, label: string): void {
      selectedId = id;
      input.value = id ? label : '';
      if (open) {
        renderList();
      }
    },
  };
}
