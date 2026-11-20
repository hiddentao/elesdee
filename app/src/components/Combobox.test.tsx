import { useState } from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Combobox, type ComboboxOption } from './Combobox';

const options: ComboboxOption[] = [
  { id: 'p1', label: 'Core', meta: '/dev/core' },
  { id: 'p2', label: 'Web', meta: '/dev/web' },
];

function Harness({ onChange }: { onChange: (id: string | null) => void }) {
  const [value, setValue] = useState<string | null>('p1');

  return (
    <Combobox
      id="project"
      label="Project"
      value={value}
      options={options}
      onChange={(id) => {
        setValue(id);
        onChange(id);
      }}
    />
  );
}

function NullHarness({ onChange }: { onChange: (id: string | null) => void }) {
  const [value, setValue] = useState<string | null>(null);

  return (
    <Combobox
      id="project"
      label="Project"
      value={value}
      options={options}
      onChange={(id) => {
        setValue(id);
        onChange(id);
      }}
    />
  );
}

describe('Combobox', () => {
  it('restores the previous selection when Escape closes the list', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await user.clear(input);
    await user.type(input, 'zzzz');
    expect(input).toHaveValue('zzzz');

    await user.keyboard('{Escape}');

    expect(input).toHaveValue('Core');
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('selects the first option with ArrowDown from a closed list', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<NullHarness onChange={onChange} />);
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();

    await user.keyboard('{ArrowDown}{Enter}');

    expect(onChange).toHaveBeenCalledWith('p1');
  });

  it('moves to the last option with End', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<NullHarness onChange={onChange} />);
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await user.keyboard('{End}{Enter}');

    expect(onChange).toHaveBeenCalledWith('p2');
  });

  it('leaves Home and End to the text caret when the list is closed', async () => {
    const user = userEvent.setup();
    render(<NullHarness onChange={vi.fn()} />);
    const input = screen.getByLabelText('Project') as HTMLInputElement;

    await user.click(input);
    await user.type(input, 'we');
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();

    input.setSelectionRange(1, 1);

    expect(fireEvent.keyDown(input, { key: 'Home' })).toBe(true);
    expect(fireEvent.keyDown(input, { key: 'End' })).toBe(true);
    expect(input).toHaveValue('we');
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('moves through the list with Home and End while it is open', async () => {
    const user = userEvent.setup();
    render(<NullHarness onChange={vi.fn()} />);
    const input = screen.getByLabelText('Project');

    await user.click(input);
    expect(screen.getByRole('listbox')).toBeInTheDocument();

    expect(fireEvent.keyDown(input, { key: 'End' })).toBe(false);
    expect(input).toHaveAttribute('aria-activedescendant', 'project-option-1');

    expect(fireEvent.keyDown(input, { key: 'Home' })).toBe(false);
    expect(input).toHaveAttribute('aria-activedescendant', 'project-option-0');
  });

  it('marks required comboboxes and announces their errors', () => {
    render(
      <Combobox
        id="project"
        label="Project"
        value={null}
        options={options}
        onChange={vi.fn()}
        required
        error="Choose a project."
      />,
    );

    expect(screen.getByLabelText('Project')).toHaveAttribute(
      'aria-required',
      'true',
    );
    expect(screen.getByRole('alert')).toHaveTextContent('Choose a project.');
  });

  it('clears the field when the selected value is cleared', async () => {
    function Clearable() {
      const [value, setValue] = useState<string | null>('p1');
      return (
        <>
          <Combobox
            id="project"
            label="Project"
            value={value}
            options={options}
            onChange={setValue}
          />
          <button type="button" onClick={() => setValue(null)}>
            Clear
          </button>
        </>
      );
    }

    const user = userEvent.setup();
    render(<Clearable />);
    const input = screen.getByLabelText('Project');
    expect(input).toHaveValue('Core');

    await user.click(screen.getByRole('button', { name: 'Clear' }));

    expect(input).toHaveValue('');
  });

  it('updates the selection when an option is chosen', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await user.clear(input);
    await user.type(input, 'Web');
    await user.click(screen.getByRole('option', { name: /Web/ }));

    expect(onChange).toHaveBeenCalledWith('p2');
    expect(input).toHaveValue('Web');
  });
});
