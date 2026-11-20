import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { FileEditor } from './FileEditor';

describe('FileEditor', () => {
  it('names the code field with the open file', async () => {
    render(
      <FileEditor
        filename="note.txt"
        label="Editor: note.txt"
        value="hello"
        onChange={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole('textbox', { name: 'Editor: note.txt' }),
    ).toBeInTheDocument();
  });
});
