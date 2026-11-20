import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Question } from '../api/types';
import { QuestionsDialog } from './QuestionsDialog';

const question: Question = {
  id: 'q1',
  sessionId: 's1',
  sessionName: 'Fix login',
  text: 'Deploy now?',
  options: ['Yes', 'No'],
  status: 'pending',
  answer: null,
  createdAt: '2026-01-01T00:00:00Z',
};

describe('QuestionsDialog', () => {
  it('lists pending questions and answers one', async () => {
    const user = userEvent.setup();
    const onAnswer = vi.fn().mockResolvedValue(undefined);
    render(
      <QuestionsDialog
        questions={[question]}
        onAnswer={onAnswer}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText('Fix login')).toBeInTheDocument();
    expect(screen.getByText('Deploy now?')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Yes' }));
    expect(onAnswer).toHaveBeenCalledWith('q1', 'Yes');
  });

  it('starts on the first answer option instead of Close', () => {
    render(
      <QuestionsDialog
        questions={[question]}
        onAnswer={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole('button', { name: 'Yes' })).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Close' })).not.toHaveFocus();
  });

  it('shows an empty state', () => {
    render(
      <QuestionsDialog questions={[]} onAnswer={vi.fn()} onClose={vi.fn()} />,
    );
    expect(screen.getByText('No pending questions.')).toBeInTheDocument();
  });
});
