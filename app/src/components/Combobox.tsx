import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from 'react';
import { createPortal } from 'react-dom';
import { useLanguage } from '../i18n/LanguageProvider';
import { ChevronDownIcon } from './icons';

export interface ComboboxOption {
  id: string;
  label: string;
  meta?: string;
  tag?: string;
}

export type ComboboxLoader = (
  query: string,
  signal: AbortSignal,
) => Promise<ComboboxOption[]>;

interface ComboboxProps {
  id: string;
  label: string;
  value: string | null;
  options?: ComboboxOption[];
  loadOptions?: ComboboxLoader;
  selectedOption?: ComboboxOption | null;
  onChange: (id: string | null) => void;
  placeholder?: string;
  hint?: string;
  error?: string;
  required?: boolean;
  autoFocus?: boolean;
  disabled?: boolean;
}

const LOAD_DELAY_MS = 220;

export function Combobox({
  id,
  label,
  value,
  options = [],
  loadOptions,
  selectedOption,
  onChange,
  placeholder,
  hint,
  error,
  required = false,
  autoFocus = false,
  disabled = false,
}: ComboboxProps) {
  const { t } = useLanguage();
  const [asyncOptions, setAsyncOptions] = useState<ComboboxOption[]>([]);
  const [asyncLoaded, setAsyncLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [loadedQuery, setLoadedQuery] = useState<string | null>(null);

  const selected =
    selectedOption ??
    options.find((option) => option.id === value) ??
    asyncOptions.find((option) => option.id === value) ??
    null;
  const selectedLabel = selected?.label ?? '';
  const [query, setQuery] = useState(selectedLabel);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [popoverStyle, setPopoverStyle] = useState<CSSProperties | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const abortRef = useRef<AbortController | null>(null);
  const requestSeqRef = useRef(0);
  const openRef = useRef(false);
  const openQueryRef = useRef<string | null>(null);
  const async = Boolean(loadOptions);

  useEffect(() => {
    if (autoFocus) {
      inputRef.current?.focus();
    }
  }, [autoFocus]);

  useEffect(
    () => () => {
      requestSeqRef.current += 1;
      abortRef.current?.abort();
    },
    [],
  );

  useEffect(() => {
    setQuery(selectedLabel);
  }, [selectedLabel]);

  const normalized = query.trim().toLowerCase();
  const queryIsSelection =
    selectedLabel !== '' && normalized === selectedLabel.toLowerCase();
  const listOptions = async
    ? asyncOptions
    : normalized === '' || queryIsSelection
      ? options
      : options.filter(
          (option) =>
            option.label.toLowerCase().includes(normalized) ||
            (option.meta ? option.meta.toLowerCase().includes(normalized) : false),
        );
  const showLoading = async && (loading || !asyncLoaded);
  const filtered = showLoading ? [] : listOptions;

  const load = useCallback(
    async (queryText: string) => {
      if (!loadOptions) {
        return;
      }

      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      const seq = ++requestSeqRef.current;

      setLoading(true);
      setLoadFailed(false);
      try {
        const next = await loadOptions(queryText, controller.signal);
        if (seq !== requestSeqRef.current || controller.signal.aborted) {
          return;
        }
        setAsyncOptions(next);
        setLoadedQuery(queryText);
        setAsyncLoaded(true);
      } catch {
        if (seq !== requestSeqRef.current || controller.signal.aborted) {
          return;
        }
        setAsyncOptions([]);
        setLoadedQuery(queryText);
        setAsyncLoaded(true);
        setLoadFailed(true);
      } finally {
        if (seq === requestSeqRef.current) {
          setLoading(false);
        }
      }
    },
    [loadOptions],
  );

  useEffect(() => {
    if (!open) {
      openRef.current = false;
      openQueryRef.current = null;
      abortRef.current?.abort();
      return;
    }
    if (!loadOptions) {
      return;
    }

    const justOpened = !openRef.current;
    openRef.current = true;
    const normalizedQuery = query.trim();
    const effectiveQuery =
      justOpened || queryIsSelection ? '' : normalizedQuery;

    if (!justOpened) {
      if (query === openQueryRef.current) {
        return;
      }
      openQueryRef.current = null;
    } else {
      openQueryRef.current = query;
    }

    if (!justOpened && effectiveQuery === loadedQuery) {
      return;
    }

    const timer = window.setTimeout(
      () => {
        void load(effectiveQuery);
      },
      justOpened ? 0 : LOAD_DELAY_MS,
    );

    return () => window.clearTimeout(timer);
  }, [open, query, queryIsSelection, loadedQuery, loadOptions, load]);

  useEffect(() => {
    setActiveIndex((index) => Math.min(index, Math.max(filtered.length - 1, 0)));
  }, [filtered.length]);

  useEffect(() => {
    if (!open) {
      setPopoverStyle(null);
      return;
    }

    const update = () => {
      const rect = inputRef.current?.getBoundingClientRect();
      if (!rect) {
        return;
      }

      const spaceBelow = window.innerHeight - rect.bottom - 8;
      const spaceAbove = rect.top - 8;
      const placeAbove = spaceBelow < 160 && spaceAbove > spaceBelow;
      const maxHeight = Math.max(
        Math.min(220, placeAbove ? spaceAbove : spaceBelow),
        0,
      );

      setPopoverStyle(
        placeAbove
          ? {
              position: 'fixed',
              left: rect.left,
              bottom: window.innerHeight - rect.top + 4,
              width: rect.width,
              maxHeight,
            }
          : {
              position: 'fixed',
              left: rect.left,
              top: rect.bottom + 4,
              width: rect.width,
              maxHeight,
            },
      );
    };

    update();
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);

    return () => {
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    const option = listRef.current?.querySelector<HTMLElement>(
      `[data-index="${activeIndex}"]`,
    );
    option?.scrollIntoView?.({ block: 'nearest' });
  }, [activeIndex, open, popoverStyle]);

  const describedBy = error ? `${id}-error` : hint ? `${id}-hint` : undefined;
  const listboxId = `${id}-listbox`;
  const activeOption = open ? filtered[activeIndex] : undefined;

  function select(option: ComboboxOption) {
    onChange(option.id);
    setQuery(option.label);
    setOpen(false);
    setActiveIndex(0);
  }

  function handleChange(next: string) {
    setQuery(next);
    setOpen(true);
    setActiveIndex(0);
  }

  function handleBlur() {
    setOpen(false);
    if (selectedLabel) {
      setQuery(selectedLabel);
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (disabled) {
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (!open) {
        setOpen(true);
        setActiveIndex(0);
      } else {
        setActiveIndex((index) =>
          Math.min(index + 1, Math.max(filtered.length - 1, 0)),
        );
      }
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) {
        setOpen(true);
        setActiveIndex(Math.max(filtered.length - 1, 0));
      } else {
        setActiveIndex((index) => Math.max(index - 1, 0));
      }
      return;
    }

    if (event.key === 'Home' && open) {
      event.preventDefault();
      setActiveIndex(0);
      return;
    }

    if (event.key === 'End' && open) {
      event.preventDefault();
      setActiveIndex(Math.max(filtered.length - 1, 0));
      return;
    }

    if (event.key === 'Enter' && open) {
      event.preventDefault();
      const option = filtered[activeIndex];
      if (option) {
        select(option);
      }
      return;
    }

    if (event.key === 'Escape' && open) {
      event.stopPropagation();
      setOpen(false);
      if (selectedLabel) {
        setQuery(selectedLabel);
      }
    }
  }

  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <div className="combo">
        <input
          ref={inputRef}
          id={id}
          className="text-input combo-input"
          role="combobox"
          aria-expanded={open}
          aria-controls={listboxId}
          aria-autocomplete="list"
          aria-activedescendant={
            activeOption ? `${id}-option-${activeIndex}` : undefined
          }
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          aria-required={required || undefined}
          autoComplete="off"
          spellCheck={false}
          disabled={disabled}
          value={query}
          placeholder={placeholder}
          onChange={(event) => handleChange(event.target.value)}
          onClick={() => {
            if (!disabled) {
              setOpen(true);
            }
          }}
          onBlur={handleBlur}
          onKeyDown={handleKeyDown}
        />
        <span className="combo-caret" aria-hidden="true">
          <ChevronDownIcon />
        </span>
      </div>

      {open && popoverStyle
        ? createPortal(
            <div className="combo-popover" style={popoverStyle}>
              <ul
                ref={listRef}
                className="combo-list"
                id={listboxId}
                role="listbox"
                aria-label={t('combobox.optionsLabel', { label })}
                aria-busy={async ? showLoading : undefined}
              >
                {filtered.map((option, index) => (
                  <li
                    key={option.id}
                    id={`${id}-option-${index}`}
                    data-index={index}
                    className={
                      index === activeIndex ? 'combo-option is-active' : 'combo-option'
                    }
                    role="option"
                    aria-selected={option.id === value}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => select(option)}
                  >
                    <span className="combo-label-row">
                      <span>{option.label}</span>
                      {option.tag ? (
                        <span className="sample-tag">{option.tag}</span>
                      ) : null}
                    </span>
                    {option.meta ? (
                      <span className="combo-meta">{option.meta}</span>
                    ) : null}
                  </li>
                ))}
              </ul>
              {showLoading ? (
                <p className="combo-empty" role="status">
                  {t('combobox.loading')}
                </p>
              ) : loadFailed ? (
                <p className="combo-empty" role="alert">
                  {t('combobox.loadFailed')}
                </p>
              ) : filtered.length === 0 ? (
                <p className="combo-empty" role="status">
                  {t('combobox.noMatches')}
                </p>
              ) : (
                <p className="sr-only" role="status">
                  {t(
                    filtered.length === 1
                      ? 'combobox.optionCountOne'
                      : 'combobox.optionCountMany',
                    { count: filtered.length },
                  )}
                </p>
              )}
            </div>,
            document.body,
          )
        : null}

      {error ? (
        <p className="field-error" id={`${id}-error`} role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="field-hint" id={`${id}-hint`}>
          {hint}
        </p>
      ) : null}
    </div>
  );
}
